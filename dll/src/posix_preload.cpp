// POSIX LD_PRELOAD / DYLD_INSERT_LIBRARIES shim for redrover.
//
// Build target:
//   Linux:  libredrover_preload.so
//   macOS:  libredrover_preload.dylib
//
// Usage:
//   Linux:
//       LD_PRELOAD=/path/to/libredrover_preload.so /path/to/Discord
//
//   macOS:
//       DYLD_INSERT_LIBRARIES=/path/to/libredrover_preload.dylib \
//           DYLD_FORCE_FLAT_NAMESPACE=1 \
//           /Applications/Discord.app/Contents/MacOS/Discord
//
//   Note for macOS: System Integrity Protection blocks dyld injection into
//   most system / Apple-signed binaries. Discord is a third-party app, so
//   it works — but the user must run from a TTY where SIP allows DYLD_
//   variables. See docs/PORTING.md.
//
// What this file does:
//   - Intercepts socket(2) to remember which sockets are UDP.
//   - Intercepts sendto(2) to run the configured UDP strategy on the
//     first send of each new UDP socket. Everything else is delegated
//     to libc through dlsym(RTLD_NEXT, ...).
//
// What's deliberately NOT here:
//   - TCP proxy. On Linux/macOS, the Chromium client honors http_proxy /
//     https_proxy and SOCKS5 env variables natively, so there's no need
//     to mangle TCP for Discord. The user just exports the env vars.
//   - SOCKS5 UDP ASSOCIATE. Same skeleton as on Windows
//     (see socks5_udp.cpp); fill it in when ready.

#ifndef _GNU_SOURCE
#define _GNU_SOURCE 1   // needed for RTLD_NEXT on glibc
#endif

#include "platform.h"

#if !RR_OS_POSIX
#error "posix_preload.cpp must only be compiled on Linux / macOS"
#endif

#include <dlfcn.h>
#include <pwd.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <unistd.h>

#include <atomic>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <memory>
#include <mutex>

#include "config.h"
#include "logging.h"
#include "real_io.h"
#include "socket_manager.h"
#include "socks5_udp.h"
#include "udp_strategy.h"

namespace {

using socket_fn   = int (*)(int, int, int);
using recvfrom_fn = ssize_t (*)(int, void*, size_t, int, struct sockaddr*, socklen_t*);

socket_fn   real_socket_p   = nullptr;
recvfrom_fn real_recvfrom_p = nullptr;

std::unique_ptr<redrover::IUdpStrategy> g_udp_strategy;
std::once_flag g_init_once;

std::filesystem::path detect_config_dir() {
    // Priority order:
    //   1. $REDROVER_DIR (explicit override)
    //   2. $XDG_CONFIG_HOME/redrover (Linux conventions)
    //   3. ~/.config/redrover (fallback)
    //   4. $HOME/Library/Application Support/redrover (macOS conventions)
    if (const char* env = std::getenv("REDROVER_DIR"); env && *env) {
        return std::filesystem::path(env);
    }
    if (const char* env = std::getenv("XDG_CONFIG_HOME"); env && *env) {
        return std::filesystem::path(env) / "redrover";
    }
    const char* home = std::getenv("HOME");
    if (!home || !*home) {
        struct passwd* pw = getpwuid(getuid());
        if (pw && pw->pw_dir) home = pw->pw_dir;
    }
    if (home && *home) {
#if RR_OS_MAC
        return std::filesystem::path(home) / "Library" / "Application Support" / "redrover";
#else
        return std::filesystem::path(home) / ".config" / "redrover";
#endif
    }
    return std::filesystem::current_path();
}

void resolve_real_symbols() {
    real_socket_p   = reinterpret_cast<socket_fn>(dlsym(RTLD_NEXT, "socket"));
    real_recvfrom_p = reinterpret_cast<recvfrom_fn>(dlsym(RTLD_NEXT, "recvfrom"));
    auto rs = reinterpret_cast<redrover::real_io::posix_sendto_t>(dlsym(RTLD_NEXT, "sendto"));
    redrover::real_io::real_sendto_posix = rs;
}

void initialize() {
    resolve_real_symbols();

    auto dir = detect_config_dir();
    std::error_code ec;
    std::filesystem::create_directories(dir, ec);

    redrover::g_current_dir = dir;
    try {
        redrover::g_options = redrover::Config::load(dir / "drover.ini");
    } catch (...) {
        // best-effort — proceed with defaults
    }
    redrover::Logger::init(dir);
    LOG_INFO("redrover preload loaded; config dir = {}", dir.string());

    g_udp_strategy = redrover::make_strategy(redrover::g_options.udp);
    LOG_INFO("UDP strategy: {}", std::string{redrover::udp_strategy_name(redrover::g_options.udp.strategy)});
}

void ensure_init() {
    std::call_once(g_init_once, initialize);
}

} // namespace

extern "C" int socket(int domain, int type, int protocol) {
    ensure_init();
    int s = real_socket_p ? real_socket_p(domain, type, protocol) : -1;
    if (s >= 0) {
        redrover::g_socket_manager.add(s, type, protocol);
    }
    return s;
}

extern "C" ssize_t sendto(int sockfd, const void* buf, size_t len, int flags,
                          const struct sockaddr* dest_addr, socklen_t addrlen) {
    ensure_init();

    // Per-packet SOCKS5 wrap (every voice packet, not just the first).
    if (buf && len > 0 && redrover::g_socks5_udp.has_association(sockfd)) {
        auto rc = redrover::g_socks5_udp.send_via_relay(sockfd, buf, len, dest_addr, addrlen);
        if (rc >= 0) return static_cast<ssize_t>(rc);
    }

    redrover::SocketEntry entry;
    if (redrover::g_socket_manager.is_first_send(sockfd, entry) && entry.is_udp &&
        buf && len > 0) {

        // Establish SOCKS5 UDP association on the first send.
        if (redrover::g_socks5_udp.enabled() &&
            redrover::g_socks5_udp.ensure_association(sockfd)) {
            auto rc = redrover::g_socks5_udp.send_via_relay(sockfd, buf, len, dest_addr, addrlen);
            if (rc >= 0) return static_cast<ssize_t>(rc);
            // fall through to local strategy on failure
        }

        redrover::UdpFirstSendCtx ctx{
            sockfd, dest_addr, addrlen,
            std::span<const std::uint8_t>(static_cast<const std::uint8_t*>(buf), len),
        };
        bool let_through = g_udp_strategy->on_first_send(ctx);
        if (!let_through) {
            return static_cast<ssize_t>(len);
        }
    }

    if (redrover::real_io::real_sendto_posix) {
        return redrover::real_io::real_sendto_posix(sockfd, buf, len, flags, dest_addr, addrlen);
    }
    // Should be unreachable — initialize() resolves the pointer up-front.
    errno = ENOSYS;
    return -1;
}

extern "C" ssize_t recvfrom(int sockfd, void* buf, size_t len, int flags,
                            struct sockaddr* src_addr, socklen_t* addrlen) {
    ensure_init();
    ssize_t n = real_recvfrom_p ? real_recvfrom_p(sockfd, buf, len, flags, src_addr, addrlen)
                                : -1;
    if (n > 0 && redrover::g_socks5_udp.has_association(sockfd)) {
        redrover::rr_socklen_t src_len = addrlen ? *addrlen : 0;
        auto unwrapped = redrover::g_socks5_udp.unwrap_reply(
            static_cast<std::uint8_t*>(buf), static_cast<std::size_t>(n),
            src_addr, &src_len);
        if (unwrapped >= 0) {
            if (addrlen) *addrlen = src_len;
            return static_cast<ssize_t>(unwrapped);
        }
    }
    return n;
}
