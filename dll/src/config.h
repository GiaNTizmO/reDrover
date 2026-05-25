// Mirror of crates/redrover-config/src/lib.rs. If you add a knob there,
// add it here too — the GUI writes, the DLL reads.

#pragma once

#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "proxy_value.h"

namespace redrover {

enum class UdpStrategy {
    Off,
    Classic, // Pascal-compatible default: 0x00, 0x01, sleep, original.
    UaeV1,
    UaeV2,
    Split,
    Custom,
};

UdpStrategy parse_udp_strategy(std::string_view s);
std::string_view udp_strategy_name(UdpStrategy s);

struct UdpSettings {
    UdpStrategy strategy = UdpStrategy::Classic;
    uint32_t prefix_delay_ms = 50;
    std::vector<std::vector<uint8_t>> prefix_packets = {{0x00}, {0x01}};
    uint8_t split_first = 0;
    bool force_tcp_fallback = false;
};

struct DroverOptions {
    std::string proxy_url;
    ProxyValue proxy; // parsed view of proxy_url
    UdpSettings udp;
    bool socks5_udp_associate = false;
    std::optional<std::string> log_level;
    bool log_file_enabled = true; // Keep legacy configs writing drover.log by default.
    std::optional<std::string> log_file;
    bool log_console = false; // Windows: AllocConsole + ANSI-colored mirror
};

namespace Config {
    DroverOptions load(const std::filesystem::path& ini_path);
}

// Globals populated at DLL load time.
extern std::filesystem::path g_current_dir;
extern DroverOptions g_options;

} // namespace redrover
