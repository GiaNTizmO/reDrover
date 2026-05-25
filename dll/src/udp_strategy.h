// ⭐ Pluggable UDP-strategy interface.
//
// Every idea in §2 of ../discord-drover/ISSUE-65-IDEAS.md maps to a
// strategy here:
//   §2.1 ConfiguredPrefixStrategy   — prefix_packets / prefix_delay_ms from ini
//   §2.2 Named strategies           — see make_strategy()
//   §2.3 SplitFirstStrategy         — break the 74-byte packet into N parts
//   §2.4 Curated payloads           — UaeV1Strategy / UaeV2Strategy (load from dist/strategies/*.bin)
//   §2.6 ForceTcpFallbackStrategy   — intentionally drop the first UDP packet
//
// The hook layer (`hooks.cpp::MyWSASendTo` on Windows, `posix_preload.cpp::sendto`
// on POSIX) only knows about the `IUdpStrategy` interface; new strategies
// plug in through `make_strategy`.

#pragma once

#include <cstdint>
#include <memory>
#include <span>

#include "platform.h"

namespace redrover {

struct UdpSettings;

// Context passed to a strategy on every first send.
struct UdpFirstSendCtx {
    rr_socket_t sock;
    const sockaddr* to;
    rr_socklen_t to_len;
    std::span<const std::uint8_t> original_packet; // bytes the app wanted to send
};

class IUdpStrategy {
public:
    virtual ~IUdpStrategy() = default;

    // Called once per UDP socket on its first send. Implementations should
    // call redrover::real_io::send_udp() for any prefix packets / split
    // fragments they want to emit before the original packet goes out.
    // Return false to suppress the original send (e.g. ForceTcpFallback).
    virtual bool on_first_send(const UdpFirstSendCtx& ctx) = 0;
};

std::unique_ptr<IUdpStrategy> make_strategy(const UdpSettings& settings);

} // namespace redrover
