// Platform abstraction layer.
//
// The portable parts of redrover (config, proxy URL parsing, UDP strategies,
// SOCKS5 framing, logging, socket bookkeeping) compile on Windows, Linux,
// and macOS. This header smooths over the differences in:
//   - Socket handle type (SOCKET = UINT_PTR on Win, int on POSIX)
//   - Address length type (int on Win, socklen_t on POSIX)
//   - Buffer length / return types (int on Win, size_t / ssize_t on POSIX)
//   - System header layout (winsock2.h vs sys/socket.h)
//   - localtime_s vs localtime_r
//
// Anything that lives behind `#ifdef RR_OS_*` belongs in the platform-
// specific shim (windows/hooks.cpp, posix/preload.cpp), not in core code.

#pragma once

#include <cstddef>
#include <cstdint>
#include <ctime>
#include <string>

#if defined(_WIN32)
#  define RR_OS_WIN  1
#elif defined(__APPLE__)
#  define RR_OS_MAC  1
#  define RR_OS_POSIX 1
#elif defined(__linux__)
#  define RR_OS_LINUX 1
#  define RR_OS_POSIX 1
#else
#  error "redrover: unsupported platform"
#endif

#if RR_OS_WIN
#  ifndef WIN32_LEAN_AND_MEAN
#    define WIN32_LEAN_AND_MEAN
#  endif
#  include <winsock2.h>
#  include <ws2tcpip.h>
#  include <windows.h>
namespace redrover {
    using rr_socket_t   = SOCKET;
    using rr_socklen_t  = int;
    using rr_ssize_t    = int;
    inline constexpr rr_socket_t RR_INVALID_SOCKET_V = INVALID_SOCKET;
}
#else
#  include <sys/socket.h>
#  include <netinet/in.h>
#  include <arpa/inet.h>
#  include <unistd.h>
namespace redrover {
    using rr_socket_t   = int;
    using rr_socklen_t  = socklen_t;
    using rr_ssize_t    = ssize_t;
    inline constexpr rr_socket_t RR_INVALID_SOCKET_V = -1;
}
#endif

namespace redrover::platform {

inline std::tm localtime_safe(std::time_t t) {
    std::tm out{};
#if RR_OS_WIN
    localtime_s(&out, &t);
#else
    localtime_r(&t, &out);
#endif
    return out;
}

// Write `line` to whatever the OS considers the standard debug sink:
//   - Windows: OutputDebugStringA (visible in DebugView / VS Output)
//   - POSIX:   stderr
void debug_write(const char* line);

// Spawn (or attach to) a console window for live log output. Returns
// true if the resulting stdout supports ANSI escape codes.
//   - Windows: AllocConsole + freopen(CONOUT$) + enable VT processing.
//   - POSIX:   no-op; stderr already exists and ANSI works in a terminal.
bool enable_console();

// Write a fully-formatted log line to the console attached by
// enable_console(). On Windows that's our AllocConsole; on POSIX
// it's stderr. Safe to call even if enable_console() was never invoked
// — falls back to OutputDebugString / stderr.
void console_write(const char* line);

// Format `sockaddr` (IPv4 only — Discord voice doesn't use IPv6 from
// the client) into "1.2.3.4:5678". Returns "?" on failure.
std::string format_sockaddr(const sockaddr* sa);

} // namespace redrover::platform
