# Changelog

All notable changes to Redrover are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Initial public scaffold: Rust + iced GUI installer, C++20 native shim.
- HTTP and SOCKS5 proxy support for Discord TCP, with HTTP Basic auth and
  SOCKS5 user/pass auth (RFC 1929).
- SOCKS5 UDP ASSOCIATE relay per RFC 1928 §7 (sync receive path).
- Pluggable UDP voice-bypass strategies (`classic`, `uae_v1/v2`, `split`,
  `custom`, `force_tcp_fallback`).
- Cross-platform native shim: Windows `version.dll` via DLL hijack +
  MinHook, Linux/macOS `libredrover_preload.{so,dylib}` via
  `LD_PRELOAD` / `DYLD_INSERT_LIBRARIES`.
- PE-level export forwarders to `api-ms-win-core-version-l1-1-0` —
  works on both x86 and x64 Discord.
- Static CRT linkage for the Windows DLL so it has zero external
  dependencies and the loader can't silently reject it.
- Opt-in debug console (`AllocConsole`) on Windows with ANSI color.
- Structured log file (`drover.log`) with INFO/DEBUG/WARN/ERROR levels.
- GitHub Actions CI across Linux / Windows / macOS.

### Known issues
- Async `WSARecvFrom` (Chromium IOCP path) is not intercepted on Windows;
  SOCKS5 UDP voice may not work end-to-end. Tracked in `docs/ROADMAP.md`.
