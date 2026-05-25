// Bookkeeping for application sockets. Mirrors SocketManager.pas: we need
// to remember whether a socket is TCP/UDP, whether it has sent its first
// packet (because our mangling logic runs only on the first send), and
// whether we're expecting a SOCKS5 reply that we have to rewrite as HTTP.
//
// Cross-platform: uses redrover::rr_socket_t which expands to SOCKET on
// Windows and int on POSIX.

#pragma once

#include <cstdint>
#include <mutex>
#include <vector>

#include "platform.h"

namespace redrover {

struct SocketEntry {
    rr_socket_t sock = RR_INVALID_SOCKET_V;
    bool is_tcp = false;
    bool is_udp = false;
    bool has_sent = false;
    bool fake_http_proxy_flag = false;
    std::uint64_t created_at_ms = 0;
};

class SocketManager {
public:
    void add(rr_socket_t sock, int sock_type, int protocol);

    // Returns true if this is the first send on the socket; copies the entry into `out`.
    bool is_first_send(rr_socket_t sock, SocketEntry& out);

    void set_fake_http_proxy_flag(rr_socket_t sock);
    bool reset_fake_http_proxy_flag(rr_socket_t sock);

private:
    void collect_garbage_locked();

    std::vector<SocketEntry> items_;
    std::mutex mutex_;
};

extern SocketManager g_socket_manager;

} // namespace redrover
