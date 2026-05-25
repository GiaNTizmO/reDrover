#include "socket_manager.h"

#include <chrono>

namespace redrover {

SocketManager g_socket_manager;

namespace {
std::uint64_t tick_now_ms() {
    using namespace std::chrono;
    return static_cast<std::uint64_t>(
        duration_cast<milliseconds>(steady_clock::now().time_since_epoch()).count());
}
} // namespace

void SocketManager::collect_garbage_locked() {
    auto now = tick_now_ms();
    auto cutoff = (now > 30'000) ? (now - 30'000) : 0;
    for (auto it = items_.begin(); it != items_.end();) {
        if (it->created_at_ms < cutoff) it = items_.erase(it);
        else                            ++it;
    }
}

void SocketManager::add(rr_socket_t sock, int sock_type, int protocol) {
    SocketEntry entry;
    entry.sock = sock;
    entry.is_tcp = (sock_type == SOCK_STREAM) &&
                   (protocol == IPPROTO_TCP || protocol == 0);
    entry.is_udp = (sock_type == SOCK_DGRAM) &&
                   (protocol == IPPROTO_UDP || protocol == 0);
    entry.created_at_ms = tick_now_ms();

    std::lock_guard<std::mutex> g(mutex_);
    collect_garbage_locked();
    for (auto& it : items_) {
        if (it.sock == sock) {
            it = entry;
            return;
        }
    }
    items_.push_back(entry);
}

bool SocketManager::is_first_send(rr_socket_t sock, SocketEntry& out) {
    std::lock_guard<std::mutex> g(mutex_);
    for (auto& it : items_) {
        if (it.sock == sock && !it.has_sent) {
            it.has_sent = true;
            out = it;
            return true;
        }
    }
    return false;
}

void SocketManager::set_fake_http_proxy_flag(rr_socket_t sock) {
    std::lock_guard<std::mutex> g(mutex_);
    for (auto& it : items_) {
        if (it.sock == sock) {
            it.fake_http_proxy_flag = true;
            return;
        }
    }
}

bool SocketManager::reset_fake_http_proxy_flag(rr_socket_t sock) {
    std::lock_guard<std::mutex> g(mutex_);
    for (auto& it : items_) {
        if (it.sock == sock && it.fake_http_proxy_flag) {
            it.fake_http_proxy_flag = false;
            return true;
        }
    }
    return false;
}

} // namespace redrover
