// Entry point. Order matters here:
//   1. Drop a marker file IMMEDIATELY. If you don't see drover-attached.txt
//      next to Discord.exe, the DLL never ran — that's a "DLL hijack
//      didn't take" problem, not a redrover bug.
//   2. Load drover.ini. We need it before Logger::init so the [logging]
//      section is honored.
//   3. Init Logger (optional file + maybe spawn console-setup thread).
//   4. Resolve the real version.dll trampolines.
//   5. Install hooks.
//
// AllocConsole is deliberately NOT called from DllMain itself — see
// platform.cpp::console_setup_thread for the loader-lock reasoning.

#include <windows.h>

#include <chrono>
#include <cstdio>
#include <ctime>
#include <filesystem>
#include <string>

#include "config.h"
#include "hooks.h"
#include "logging.h"
#include "platform.h"

namespace {

std::filesystem::path module_directory(HMODULE module) {
    wchar_t buf[MAX_PATH];
    GetModuleFileNameW(module, buf, MAX_PATH);
    return std::filesystem::path(buf).parent_path();
}

// "Hey, I loaded!" — written to the Discord folder at the very first
// instruction of DllMain. If this file appears, the DLL hijack worked.
// If it doesn't, Discord is loading the system version.dll instead.
void write_attached_marker(const std::filesystem::path& dir) {
    FILE* f = nullptr;
    auto marker = (dir / "drover-attached.txt").string();
    if (fopen_s(&f, marker.c_str(), "ab") != 0 || !f) return;

    std::time_t t = std::time(nullptr);
    std::tm tm = redrover::platform::localtime_safe(t);
    char ts[32];
    std::strftime(ts, sizeof(ts), "%Y-%m-%d %H:%M:%S", &tm);

    std::fprintf(f, "[%s] redrover attached  pid=%lu\n",
                 ts, static_cast<unsigned long>(GetCurrentProcessId()));
    std::fclose(f);
}

void log_state_summary() {
    const auto& opt = redrover::g_options;
    const auto& p = opt.proxy;
    if (p.is_specified) {
        LOG_INFO("proxy: {}://{}{}:{}",
                 p.kind == redrover::ProxyKind::Socks5 ? "socks5" : "http",
                 p.has_auth() ? (p.login + ":***@") : std::string{},
                 p.host, p.port);
    } else {
        LOG_INFO("proxy: direct (UDP mangling only)");
    }
    LOG_INFO("udp strategy: {}{}",
             std::string{redrover::udp_strategy_name(opt.udp.strategy)},
             opt.udp.force_tcp_fallback ? " (overridden by force_tcp_fallback)" : "");
    if (opt.socks5_udp_associate) {
        LOG_INFO("socks5 udp_associate: enabled (sync recvfrom unwraps; async WSARecvFrom not yet supported)");
    }
}

// Runs in its own thread spawned after DllMain returns. Its job is to
// (a) trigger the actual AllocConsole, which is unsafe inside DllMain,
// and (b) re-emit the state summary so it shows up in the brand-new
// console window — the dllmain-time LOG_INFO calls happened before the
// console existed.
DWORD WINAPI deferred_console_init(LPVOID) {
    redrover::platform::enable_console();
    // Give the console thread a moment to flush its banner.
    Sleep(50);
    LOG_INFO("--- redrover console attached ---");
    log_state_summary();
    if (redrover::g_options.log_file_enabled) {
        LOG_INFO("Tip: the configured log file includes events that fired before the console opened.");
    }
    return 0;
}

void on_attach(HMODULE module) {
    auto current_dir = module_directory(module);

    // Step 1 — marker. Do this before ANYTHING that could fail.
    write_attached_marker(current_dir);

    redrover::g_current_dir = current_dir;

    // Step 2 — config. Must happen before Logger::init so [logging] applies.
    bool config_failed = false;
    std::string config_error;
    try {
        redrover::g_options = redrover::Config::load(current_dir / "drover.ini");
    } catch (const std::exception& e) {
        config_failed = true;
        config_error = e.what();
    }

    // Step 3 — logger init. File output is optional. Console (if requested)
    // is marked as "wanted" but the actual AllocConsole happens later, off
    // the DllMain thread.
    redrover::Logger::init(current_dir);
    LOG_INFO("redrover dll attached at {}", current_dir.string());
    if (config_failed) {
        LOG_ERROR("config load failed: {}", config_error);
    }
    log_state_summary();

    // Step 4 — install hooks. The 17 version.dll exports are handled by
    // PE-level forwarders declared in src/exports.cpp, so we don't need
    // to resolve %SystemRoot%\System32\version.dll manually here.
    redrover::hooks::build_command_line_cache();
    redrover::hooks::install();
    LOG_INFO("hooks installed");

    // Step 5 — kick off console setup in a worker thread (no-op if the
    // user hasn't enabled it). Doing this last means hooks are active
    // by the time any Discord socket call hits.
    if (redrover::g_options.log_console) {
        HANDLE h = CreateThread(nullptr, 0, deferred_console_init, nullptr, 0, nullptr);
        if (h) CloseHandle(h);
    }
}

void on_detach() {
    LOG_INFO("redrover dll detaching");
    redrover::hooks::uninstall();
}

} // namespace

BOOL APIENTRY DllMain(HMODULE module, DWORD reason, LPVOID /*reserved*/) {
    switch (reason) {
    case DLL_PROCESS_ATTACH:
        DisableThreadLibraryCalls(module);
        on_attach(module);
        break;
    case DLL_PROCESS_DETACH:
        on_detach();
        break;
    }
    return TRUE;
}
