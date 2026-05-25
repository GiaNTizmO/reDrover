#include "discord_dirs.h"

#include <windows.h>
#include <filesystem>

#include "config.h"
#include "logging.h"

namespace redrover::discord_dirs {

namespace {
constexpr std::wstring_view kNames[] = {
    L"Discord.exe",
    L"DiscordCanary.exe",
    L"DiscordPTB.exe",
};

bool case_insensitive_equal(std::wstring_view a, std::wstring_view b) {
    if (a.size() != b.size()) return false;
    for (size_t i = 0; i < a.size(); ++i) {
        if (towlower(a[i]) != towlower(b[i])) return false;
    }
    return true;
}

bool dir_contains_discord(const std::filesystem::path& dir) {
    for (auto name : kNames) {
        if (std::filesystem::exists(dir / name)) return true;
    }
    return false;
}
} // namespace

bool is_discord_executable_w(std::wstring_view filename) {
    for (auto name : kNames) {
        if (case_insensitive_equal(filename, name)) return true;
    }
    return false;
}

void copy_dll_into_all_app_dirs() {
    auto src_dll  = g_current_dir / L"version.dll";
    auto src_opts = g_current_dir / L"drover.ini";
    if (!std::filesystem::exists(src_dll) || !std::filesystem::exists(src_opts)) return;

    auto base = g_current_dir.parent_path();
    std::error_code ec;
    for (auto& entry : std::filesystem::directory_iterator(base, ec)) {
        if (!entry.is_directory()) continue;
        auto leaf = entry.path().filename().wstring();
        if (leaf.rfind(L"app-", 0) != 0) continue;
        auto& dir = entry.path();
        if (!dir_contains_discord(dir)) continue;

        auto dst_dll  = dir / L"version.dll";
        auto dst_opts = dir / L"drover.ini";
        if (std::filesystem::exists(dst_dll) || std::filesystem::exists(dst_opts)) continue;

        std::filesystem::copy_file(src_dll, dst_dll,
                                   std::filesystem::copy_options::overwrite_existing, ec);
        std::filesystem::copy_file(src_opts, dst_opts,
                                   std::filesystem::copy_options::overwrite_existing, ec);

        for (auto extra : {L"drover-packet.bin", L"uae-v1.bin", L"uae-v2.bin"}) {
            auto src_extra = g_current_dir / extra;
            if (std::filesystem::exists(src_extra)) {
                std::filesystem::copy_file(src_extra, dir / extra,
                                           std::filesystem::copy_options::overwrite_existing, ec);
            }
        }

        LOG_INFO("propagated drover into {}", dir.string());
    }
}

} // namespace redrover::discord_dirs
