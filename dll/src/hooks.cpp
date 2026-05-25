// WinSock + process hooks. The bridge between the user-facing drover.ini
// knobs and the OS calls Discord actually makes.
//
// Hook surface (same shape as the Pascal drover.dpr plus two extra hooks
// for SOCKS5 UDP ASSOCIATE):
//
//   socket / WSASocket    — track which sockets are TCP vs UDP.
//   WSASend               — inject Proxy-Authorization: Basic for HTTP+auth.
//   WSASendTo             — per-packet SOCKS5 UDP wrap; first-send strategy.
//   send                  — convert outgoing HTTP CONNECT to SOCKS5 CONNECT.
//   recv                  — rewrite SOCKS5 success reply as HTTP/1.1 200.
//   recvfrom              — strip SOCKS5 UDP header (sync path).
//   WSARecvFrom           — same, sync path only; overlapped is a TODO.
//   GetCommandLineW       — append --proxy-server=... to Discord's cmdline.
//   GetEnvironmentVariableW — substitute http_proxy / https_proxy.
//   CreateProcessW        — copy version.dll/drover.ini into freshly-updated
//                           Discord folders so the next launch keeps working.
//
// Heavy lifting lives in the per-feature modules (`udp_strategy.cpp`,
// `socks5_udp.cpp`). If a feature here looks suspicious, cross-reference
// the original Pascal `drover.dpr` from upstream — that file is the
// canonical reference for the HTTP/SOCKS5 wire format we're targeting.

#include "hooks.h"

#include <windows.h>
#include <MinHook.h>

#include <atomic>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <mutex>
#include <regex>
#include <string>
#include <string_view>
#include <vector>

#include "config.h"
#include "discord_dirs.h"
#include "logging.h"
#include "real_io.h"
#include "socket_manager.h"
#include "socks5_udp.h"
#include "udp_strategy.h"

namespace redrover::hooks {

namespace {

// ---- Real function pointers, populated by MinHook --------------------------

using Socket_t  = SOCKET (WSAAPI*)(int, int, int);
using WSASocket_t = SOCKET (WSAAPI*)(int, int, int, LPWSAPROTOCOL_INFOW, GROUP, DWORD);
using WSASend_t = int (WSAAPI*)(SOCKET, LPWSABUF, DWORD, LPDWORD, DWORD,
                                LPWSAOVERLAPPED, LPWSAOVERLAPPED_COMPLETION_ROUTINE);
using WSASendTo_t = int (WSAAPI*)(SOCKET, LPWSABUF, DWORD, LPDWORD, DWORD,
                                  const sockaddr*, int,
                                  LPWSAOVERLAPPED, LPWSAOVERLAPPED_COMPLETION_ROUTINE);
using Send_t    = int (WSAAPI*)(SOCKET, const char*, int, int);
using Recv_t    = int (WSAAPI*)(SOCKET, char*, int, int);
using Sendto_t  = int (WSAAPI*)(SOCKET, const char*, int, int, const sockaddr*, int);
using Recvfrom_t = int (WSAAPI*)(SOCKET, char*, int, int, sockaddr*, int*);
using WSARecvFrom_t = int (WSAAPI*)(SOCKET, LPWSABUF, DWORD, LPDWORD, LPDWORD,
                                    sockaddr*, LPINT, LPWSAOVERLAPPED,
                                    LPWSAOVERLAPPED_COMPLETION_ROUTINE);
using GetCommandLineW_t = LPWSTR (WINAPI*)();
using GetEnvironmentVariableW_t = DWORD (WINAPI*)(LPCWSTR, LPWSTR, DWORD);
using CreateProcessW_t  = BOOL (WINAPI*)(LPCWSTR, LPWSTR, LPSECURITY_ATTRIBUTES,
                                         LPSECURITY_ATTRIBUTES, BOOL, DWORD, LPVOID,
                                         LPCWSTR, LPSTARTUPINFOW, LPPROCESS_INFORMATION);

Socket_t   real_socket = nullptr;
WSASocket_t real_wsa_socket = nullptr;
WSASend_t  real_wsa_send = nullptr;
WSASendTo_t real_wsa_sendto = nullptr;
Send_t     real_send = nullptr;
Recv_t     real_recv = nullptr;
Sendto_t   real_sendto = nullptr;
Recvfrom_t real_recvfrom = nullptr;
WSARecvFrom_t real_wsa_recvfrom = nullptr;
GetCommandLineW_t          real_get_cmdline = nullptr;
GetEnvironmentVariableW_t  real_get_env = nullptr;
CreateProcessW_t           real_create_process = nullptr;

std::unique_ptr<IUdpStrategy> g_udp_strategy;
std::wstring g_cmdline_cache;
std::once_flag g_strategy_once;

void ensure_strategy() {
    std::call_once(g_strategy_once, []() {
        g_udp_strategy = make_strategy(g_options.udp);
        LOG_INFO("UDP strategy initialized: {}",
                 std::string{udp_strategy_name(g_options.udp.strategy)});
    });
}

// ---- HTTP / SOCKS5 helpers (TCP-side proxy plumbing) -----------------------

// RFC 4648 base64. Tiny enough to keep inline; no need to drag in a library.
std::string base64_encode(std::string_view in) {
    static constexpr char tbl[] =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    std::string out;
    out.reserve(((in.size() + 2) / 3) * 4);
    int val = 0, bits = -6;
    for (unsigned char c : in) {
        val = (val << 8) | c;
        bits += 8;
        while (bits >= 0) {
            out.push_back(tbl[(val >> bits) & 0x3F]);
            bits -= 6;
        }
    }
    if (bits > -6) {
        out.push_back(tbl[((val << (bits + 8)) >> 2) & 0x3F]);
    }
    while (out.size() % 4) out.push_back('=');
    return out;
}

// Inject "Proxy-Authorization: Basic ..." into an outgoing HTTP request,
// in-place, preserving the original buffer length. Achieved by overwriting
// the User-Agent header with our auth header plus a junk filler header
// ("X: XXXXX...") so total bytes don't shift.
// Port of `AddHttpProxyAuthorizationHeader` from the Pascal drover.dpr.
bool add_http_proxy_auth_header(LPWSABUF buf) {
    if (!buf || !buf->buf || buf->len < 1) return false;

    auto* data = buf->buf;
    std::size_t len = buf->len;
    std::string_view pkt(data, len);

    // Don't double-inject. If a Proxy-Authorization is already present,
    // assume the caller knows what they're doing.
    if (pkt.find("\r\nProxy-Authorization: ") != std::string_view::npos) {
        return false;
    }

    auto ua_start = pkt.find("User-Agent:");
    if (ua_start == std::string_view::npos) return false;
    auto ua_end = pkt.find("\r\n", ua_start);
    if (ua_end == std::string_view::npos) return false;
    std::size_t ua_len = ua_end - ua_start;

    std::string injected = "Proxy-Authorization: Basic " +
        base64_encode(g_options.proxy.login + ":" + g_options.proxy.password);

    if (injected.size() > ua_len) return false; // doesn't fit
    std::size_t filler_len = ua_len - injected.size();
    // Need "\r\nX: " (5 chars) + at least one X.
    if (filler_len < 6) return false;

    injected += "\r\nX: ";
    injected.append(filler_len - 5, 'X');
    if (injected.size() != ua_len) return false; // sanity check

    std::memcpy(data + ua_start, injected.data(), ua_len);
    LOG_DEBUG("[http] injected Proxy-Authorization (ua_len={})", ua_len);
    return true;
}

// On the first TCP send to a SOCKS5 proxy, Chromium sends an HTTP CONNECT
// request because it thinks it's talking to an HTTP proxy. Translate that
// request on the fly to SOCKS5 wire format and mark the socket so MyRecv
// rewrites the SOCKS5 reply back into a fake HTTP/1.1 200.
// Port of `ConvertHttpToSocks5` from the Pascal drover.dpr.
bool convert_http_to_socks5(SOCKET sock, const char* buf, int len, int flags) {
    if (!g_options.proxy.is_socks5()) return false;
    if (len < 8) return false;
    if (std::memcmp(buf, "CONNECT ", 8) != 0) return false;

    // Parse "CONNECT host:port HTTP/1.1".
    static const std::regex re(R"(\ACONNECT ([A-Za-z0-9.\-]+):(\d+))",
                               std::regex::ECMAScript);
    std::cmatch m;
    if (!std::regex_search(buf, buf + len, m, re)) return false;
    std::string host(m[1].first, m[1].second);
    int port = std::atoi(std::string(m[2].first, m[2].second).c_str());
    if (port < 1 || port > 65535 || host.empty() || host.size() > 255) return false;

    // SOCKS5 method selection: VER=05, NMETHODS=01, METHOD=00 (NO AUTH).
    // Matches the Pascal version — SOCKS5+auth on TCP is not handled here.
    {
        const char hello[] = {0x05, 0x01, 0x00};
        if (real_send(sock, hello, 3, flags) != 3) return false;
    }

    // Wait up to 10s for the proxy reply.
    fd_set fds;
    FD_ZERO(&fds);
    FD_SET(sock, &fds);
    timeval tv = {10, 0};
    if (select(0, &fds, nullptr, nullptr, &tv) < 1) return false;
    if (!FD_ISSET(sock, &fds)) return false;

    char reply[2];
    if (real_recv(sock, reply, 2, 0) != 2) return false;
    if (reply[0] != 0x05 || reply[1] != 0x00) return false;

    // SOCKS5 CONNECT (CMD=01) to domain (ATYP=03).
    std::vector<char> req;
    req.reserve(7 + host.size());
    req.push_back(0x05);                  // VER
    req.push_back(0x01);                  // CMD = CONNECT
    req.push_back(0x00);                  // RSV
    req.push_back(0x03);                  // ATYP = DOMAINNAME
    req.push_back(static_cast<char>(host.size()));
    req.insert(req.end(), host.begin(), host.end());
    req.push_back(static_cast<char>((port >> 8) & 0xFF));
    req.push_back(static_cast<char>(port & 0xFF));
    if (real_send(sock, req.data(), static_cast<int>(req.size()), flags) !=
        static_cast<int>(req.size())) {
        return false;
    }

    // Tell MyRecv: the next read on this socket will be a SOCKS5 reply
    // (10 bytes for IPv4 BND or variable for domain) — rewrite it as
    // "HTTP/1.1 200 Connection Established" so Chromium thinks the HTTP
    // CONNECT succeeded.
    g_socket_manager.set_fake_http_proxy_flag(sock);
    LOG_INFO("[socks5-tcp] converted HTTP CONNECT {}:{} -> SOCKS5", host, port);
    return true;
}

// ---- Hook implementations --------------------------------------------------

SOCKET WSAAPI MySocket(int af, int type, int protocol) {
    SOCKET s = real_socket(af, type, protocol);
    if (s != INVALID_SOCKET) g_socket_manager.add(s, type, protocol);
    return s;
}

SOCKET WSAAPI MyWSASocket(int af, int type, int protocol, LPWSAPROTOCOL_INFOW info,
                          GROUP g, DWORD flags) {
    SOCKET s = real_wsa_socket(af, type, protocol, info, g, flags);
    if (s != INVALID_SOCKET) g_socket_manager.add(s, type, protocol);
    return s;
}

int WSAAPI MyWSASend(SOCKET s, LPWSABUF bufs, DWORD count, LPDWORD sent,
                     DWORD flags, LPWSAOVERLAPPED ov, LPWSAOVERLAPPED_COMPLETION_ROUTINE cr) {
    SocketEntry entry;
    if (g_socket_manager.is_first_send(s, entry) && entry.is_tcp &&
        g_options.proxy.is_http() && g_options.proxy.has_auth() &&
        bufs && count == 1 && bufs->len > 0) {
        add_http_proxy_auth_header(bufs);
    }
    return real_wsa_send(s, bufs, count, sent, flags, ov, cr);
}

int WSAAPI MyWSASendTo(SOCKET s, LPWSABUF bufs, DWORD count, LPDWORD sent, DWORD flags,
                       const sockaddr* to, int to_len, LPWSAOVERLAPPED ov,
                       LPWSAOVERLAPPED_COMPLETION_ROUTINE cr) {
    // PER-PACKET SOCKS5 wrap. Discord sends 50+ packets/sec on a voice
    // connection; every one of them needs the SOCKS5 header, not just the
    // first. The association itself is set up inside is_first_send below;
    // once that's done, this branch handles all subsequent packets.
    if (bufs && count == 1 && bufs->len > 0 && g_socks5_udp.has_association(s)) {
        rr_ssize_t rc = g_socks5_udp.send_via_relay(s, bufs->buf, bufs->len, to, to_len);
        if (rc >= 0) {
            if (sent) *sent = static_cast<DWORD>(rc);
            return 0;
        }
    }

    SocketEntry entry;
    if (g_socket_manager.is_first_send(s, entry) && entry.is_udp &&
        bufs && count > 0 && bufs->len > 0) {
        ensure_strategy();

        // Try to establish a SOCKS5 UDP association on the first send.
        // Once it's ready, subsequent sends use the per-packet branch above.
        if (g_socks5_udp.enabled() && g_socks5_udp.ensure_association(s)) {
            rr_ssize_t rc = g_socks5_udp.send_via_relay(s, bufs->buf, bufs->len, to, to_len);
            if (rc >= 0) {
                if (sent) *sent = static_cast<DWORD>(rc);
                return 0;
            }
            // Association exists but the wrap failed — fall through to
            // the local strategy. Rare edge case (e.g. IPv6 destination).
        }

        UdpFirstSendCtx ctx{
            s, to, to_len,
            std::span<const uint8_t>(reinterpret_cast<const uint8_t*>(bufs->buf), bufs->len),
        };
        bool let_original_through = g_udp_strategy->on_first_send(ctx);
        if (!let_original_through) {
            if (sent) *sent = bufs->len;
            return 0;
        }
    }
    return real_wsa_sendto(s, bufs, count, sent, flags, to, to_len, ov, cr);
}

int WSAAPI MySend(SOCKET s, const char* buf, int len, int flags) {
    SocketEntry entry;
    if (g_socket_manager.is_first_send(s, entry) && entry.is_tcp) {
        if (convert_http_to_socks5(s, buf, len, flags)) {
            return len; // pretend the original CONNECT was sent successfully
        }
    }
    return real_send(s, buf, len, flags);
}

int WSAAPI MyRecv(SOCKET s, char* buf, int len, int flags) {
    int n = real_recv(s, buf, len, flags);
    if (n >= 10 && g_socket_manager.reset_fake_http_proxy_flag(s)) {
        // After our SOCKS5 CONNECT, the proxy sends back a 10+ byte success
        // reply (VER=05, REP=00, RSV=00, ATYP, BND.ADDR, BND.PORT). Chromium
        // is expecting an HTTP/1.1 status line — fake it.
        if (static_cast<unsigned char>(buf[0]) == 0x05 &&
            static_cast<unsigned char>(buf[1]) == 0x00 &&
            static_cast<unsigned char>(buf[2]) == 0x00) {
            static constexpr char http_reply[] =
                "HTTP/1.1 200 Connection Established\r\n\r\n";
            constexpr int reply_len = static_cast<int>(sizeof(http_reply) - 1);
            if (reply_len <= len) {
                std::memcpy(buf, http_reply, reply_len);
                return reply_len;
            }
        }
    }
    return n;
}

// recvfrom hook — strips the SOCKS5 UDP header from inbound packets so
// Discord sees plain UDP voice data with the original source address
// rather than the relay's address.
int WSAAPI MyRecvfrom(SOCKET s, char* buf, int len, int flags,
                      sockaddr* from, int* fromlen) {
    int n = real_recvfrom(s, buf, len, flags, from, fromlen);
    if (n > 0 && g_socks5_udp.has_association(s)) {
        rr_socklen_t src_len = fromlen ? static_cast<rr_socklen_t>(*fromlen) : 0;
        rr_ssize_t unwrapped = g_socks5_udp.unwrap_reply(
            reinterpret_cast<std::uint8_t*>(buf), static_cast<std::size_t>(n),
            from, &src_len);
        if (unwrapped >= 0) {
            if (fromlen) *fromlen = static_cast<int>(src_len);
            return static_cast<int>(unwrapped);
        }
    }
    return n;
}

// WSARecvFrom hook — synchronous path only. Async/overlapped is logged
// as a warning once and then left untouched (Discord will see wrapped
// bytes). TODO: handle overlapped via completion-routine interposition.
int WSAAPI MyWSARecvFrom(SOCKET s, LPWSABUF buffers, DWORD count, LPDWORD recvd,
                         LPDWORD flags, sockaddr* from, LPINT fromlen,
                         LPWSAOVERLAPPED ov, LPWSAOVERLAPPED_COMPLETION_ROUTINE cr) {
    int rc = real_wsa_recvfrom(s, buffers, count, recvd, flags, from, fromlen, ov, cr);

    if (ov || cr) {
        // Async path — we can't easily intercept the completion. The
        // unwrap won't happen and Discord will see SOCKS5-wrapped bytes.
        if (g_socks5_udp.has_association(s)) {
            static std::atomic_flag warned = ATOMIC_FLAG_INIT;
            if (!warned.test_and_set()) {
                LOG_WARN("[socks5-udp] WSARecvFrom called with overlapped/completion "
                         "— async unwrap not yet implemented. Voice may not work.");
            }
        }
        return rc;
    }
    if (rc == 0 && recvd && *recvd > 0 && buffers && count == 1 &&
        g_socks5_udp.has_association(s)) {
        rr_socklen_t src_len = fromlen ? static_cast<rr_socklen_t>(*fromlen) : 0;
        rr_ssize_t unwrapped = g_socks5_udp.unwrap_reply(
            reinterpret_cast<std::uint8_t*>(buffers->buf), *recvd,
            from, &src_len);
        if (unwrapped >= 0) {
            *recvd = static_cast<DWORD>(unwrapped);
            if (fromlen) *fromlen = static_cast<int>(src_len);
        }
    }
    return rc;
}

LPWSTR WINAPI MyGetCommandLineW() {
    return const_cast<LPWSTR>(g_cmdline_cache.c_str());
}

DWORD WINAPI MyGetEnvironmentVariableW(LPCWSTR name, LPWSTR buf, DWORD size) {
    if (g_options.proxy.is_specified && name) {
        std::wstring n = name;
        for (auto& c : n) c = static_cast<wchar_t>(towlower(c));
        if (n == L"http_proxy" || n == L"https_proxy") {
            auto narrow = g_options.proxy.format_http_env();
            std::wstring wide(narrow.begin(), narrow.end());
            DWORD required = static_cast<DWORD>(wide.size()) + 1;
            if (!buf || size < required) return required;
            wcsncpy_s(buf, size, wide.c_str(), wide.size());
            return static_cast<DWORD>(wide.size());
        }
    }
    return real_get_env(name, buf, size);
}

BOOL WINAPI MyCreateProcessW(LPCWSTR app, LPWSTR cmd, LPSECURITY_ATTRIBUTES pa,
                             LPSECURITY_ATTRIBUTES ta, BOOL inherit, DWORD flags,
                             LPVOID env, LPCWSTR cwd, LPSTARTUPINFOW si,
                             LPPROCESS_INFORMATION pi) {
    if (app) {
        std::filesystem::path name(app);
        auto leaf = name.filename().wstring();
        if (discord_dirs::is_discord_executable_w(leaf) ||
            _wcsicmp(leaf.c_str(), L"reg.exe") == 0) {
            discord_dirs::copy_dll_into_all_app_dirs();
        }
    }
    return real_create_process(app, cmd, pa, ta, inherit, flags, env, cwd, si, pi);
}

template <typename T>
void create_hook(LPCSTR module, LPCSTR name, void* detour, T*& real) {
    HMODULE m = GetModuleHandleA(module);
    if (!m) m = LoadLibraryA(module);
    if (!m) return;
    auto target = GetProcAddress(m, name);
    if (!target) return;
    if (MH_CreateHook(target, detour, reinterpret_cast<LPVOID*>(&real)) == MH_OK) {
        MH_EnableHook(target);
    }
}

} // namespace

void build_command_line_cache() {
    LPWSTR original = GetCommandLineW();
    g_cmdline_cache = original ? original : L"";

    wchar_t exe[MAX_PATH] = {0};
    GetModuleFileNameW(nullptr, exe, MAX_PATH);
    std::filesystem::path p(exe);
    if (g_options.proxy.is_specified &&
        discord_dirs::is_discord_executable_w(p.filename().wstring())) {
        auto narrow = g_options.proxy.format_chrome_proxy();
        std::wstring wide(narrow.begin(), narrow.end());
        g_cmdline_cache += L" --proxy-server=";
        g_cmdline_cache += wide;
    }
}

void install() {
    if (MH_Initialize() != MH_OK) {
        LOG_ERROR("MinHook init failed");
        return;
    }

    create_hook("ws2_32.dll", "socket",      reinterpret_cast<void*>(&MySocket),      real_socket);
    create_hook("ws2_32.dll", "WSASocketW",  reinterpret_cast<void*>(&MyWSASocket),   real_wsa_socket);
    create_hook("ws2_32.dll", "WSASend",     reinterpret_cast<void*>(&MyWSASend),     real_wsa_send);
    create_hook("ws2_32.dll", "WSASendTo",   reinterpret_cast<void*>(&MyWSASendTo),   real_wsa_sendto);
    create_hook("ws2_32.dll", "send",        reinterpret_cast<void*>(&MySend),        real_send);
    create_hook("ws2_32.dll", "recv",        reinterpret_cast<void*>(&MyRecv),        real_recv);
    create_hook("ws2_32.dll", "recvfrom",    reinterpret_cast<void*>(&MyRecvfrom),    real_recvfrom);
    create_hook("ws2_32.dll", "WSARecvFrom", reinterpret_cast<void*>(&MyWSARecvFrom), real_wsa_recvfrom);
    // sendto is *not* hooked — we just need a direct pointer to the real
    // function so udp_strategy (via real_io) can call it without recursing
    // through our WSASendTo hook.
    if (HMODULE ws2 = GetModuleHandleA("ws2_32.dll")) {
        real_sendto = reinterpret_cast<Sendto_t>(GetProcAddress(ws2, "sendto"));
        real_io::real_sendto_win = real_sendto;
    }
    create_hook("kernel32.dll", "GetCommandLineW",        reinterpret_cast<void*>(&MyGetCommandLineW),        real_get_cmdline);
    create_hook("kernel32.dll", "GetEnvironmentVariableW", reinterpret_cast<void*>(&MyGetEnvironmentVariableW), real_get_env);
    create_hook("kernel32.dll", "CreateProcessW",          reinterpret_cast<void*>(&MyCreateProcessW),          real_create_process);

    LOG_INFO("hooks installed");
}

void uninstall() {
    MH_DisableHook(MH_ALL_HOOKS);
    MH_Uninitialize();
}

} // namespace redrover::hooks
