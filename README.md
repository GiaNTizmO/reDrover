<h1 align="center">reDrover</h1>

<p align="center">
  <em>Per-process proxy + voice-bypass for Discord. No drivers, no global VPN.</em>
</p>

<p align="center">
  <a href="https://github.com/hdrover/discord-drover/actions"><img alt="CI" src="https://img.shields.io/badge/ci-pending-lightgrey"></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue"></a>
  <img alt="Platform" src="https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-green">
  <img alt="Rust" src="https://img.shields.io/badge/rust-stable-orange">
  <img alt="C++" src="https://img.shields.io/badge/C%2B%2B-20-blue">
</p>

---

reDrover forces the **Discord** desktop client to send its TCP traffic
through a user-specified **HTTP** or **SOCKS5** proxy and applies optional
mangling to outgoing **UDP** packets to bypass local voice-chat blocks.
It runs entirely at the user-process level — no kernel driver, no
system-wide VPN, no admin rights.

The Windows component is a `version.dll` next to `Discord.exe` (DLL hijack +
WinSock detours via [MinHook]). The Linux / macOS component is a small
shared library used via `LD_PRELOAD` / `DYLD_INSERT_LIBRARIES`.

This is a from-scratch reimplementation of [hdrover/discord-drover] in
**Rust + iced** (installer GUI) and **C++20** (native shim). It is
designed around the brainstorm in [`docs/ISSUE-65-IDEAS.md`][issue65-doc]
so that the ideas in upstream issue [hdrover/discord-drover#65] can be
implemented incrementally.

[MinHook]: https://github.com/TsudaKageyu/minhook
[hdrover/discord-drover]: https://github.com/hdrover/discord-drover
[hdrover/discord-drover#65]: https://github.com/hdrover/discord-drover/issues/65
[issue65-doc]: docs/ISSUE-65-IDEAS.md

## Table of contents

- [Features](#features)
- [Quick start](#quick-start)
- [Configuration](#configuration)
- [How it works](#how-it-works)
- [Building from source](#building-from-source)
- [Documentation](#documentation)
- [Status](#status)
- [Contributing](#contributing)
- [License](#license)

## Features

- **HTTP and SOCKS5 proxies** for Discord's TCP (chat, REST, gateway, updater).
- **HTTP Basic auth** (RFC 7617).
- **SOCKS5 username/password auth for UDP ASSOCIATE** (RFC 1929).
- **SOCKS5 UDP ASSOCIATE** (RFC 1928 §7) — actual UDP-over-SOCKS5 relay, not just TCP.
- **Pluggable voice-UDP strategies** - the GUI exposes implemented `classic`, `split`, and `custom` variants plus `off` and `force_tcp_fallback`.
- **Drop-in compatibility** with `drover.ini` from the original Pascal project.
- **Self-propagation**: when Discord auto-updates to a new `app-X.Y.Z` folder, the DLL re-installs itself.
- **Cross-platform native shim**: same C++ core, Windows DLL or POSIX preload library.
- **Iced GUI installer** — pick a proxy, click Install, done.
- **Live debug console** on Windows via opt-in `AllocConsole`.
- **Structured log file** in the Discord folder for post-mortem troubleshooting.

## Quick start

### Windows

1. Download or build `reDrover.exe` and `version.dll` (see [Building from source](#building-from-source)).
2. Drop both into the same folder (along with optional `drover.ini`, `drover-packet.bin`, and `strategies/*.bin`).
3. Close Discord.
4. Run `reDrover.exe`, fill in the proxy details, click **Install**.
5. Start Discord normally.

### Linux

```bash
./scripts/build.sh
LD_PRELOAD=$PWD/build-output/libredrover_preload.so discord
```

Config lives in `$XDG_CONFIG_HOME/redrover/drover.ini` (default
`~/.config/redrover/drover.ini`).

### macOS

```bash
./scripts/build.sh
DYLD_INSERT_LIBRARIES=$PWD/build-output/libredrover_preload.dylib \
    DYLD_FORCE_FLAT_NAMESPACE=1 \
    /Applications/Discord.app/Contents/MacOS/Discord
```

Config lives in `~/Library/Application Support/redrover/drover.ini`.

> ⚠️ macOS SIP may strip `DYLD_*` env vars for signed apps. See
> [`docs/PORTING.md`](docs/PORTING.md) for caveats.

## Configuration

`drover.ini` lives next to `Discord.exe` on Windows or in the platform
config dir on POSIX. Both the GUI installer and the native shim parse it.

```ini
[drover]
proxy = http://user:pass@127.0.0.1:1080      ; or socks5://…  or empty for Direct

[udp]
strategy        = classic                     ; off | classic | split | custom
prefix_delay_ms = 50
prefix_packets  = 00, 01                      ; hex bytes, each entry is one packet
split_first     = 0                           ; >1 splits the first 74-byte packet into N chunks
force_tcp_fallback = false                    ; intentionally break UDP so Discord falls back to TCP voice

[socks5]
udp_associate = false                         ; route voice UDP through SOCKS5 UDP ASSOCIATE

[logging]
level   = info                                ; off | error | warn | info | debug | trace
file_enabled = true                           ; write the configured log file
file    = drover.log
console = false                               ; Windows: AllocConsole + ANSI-color live tail
```

See [`docs/ROADMAP.md`](docs/ROADMAP.md) for how each knob maps to upstream
issue #65 ideas, and [`dist/drover.ini`](dist/drover.ini) for the canonical
reference with comments.

## How it works

```
                ┌──────────────────────┐
                │  reDrover.exe (Rust) │
                │  iced GUI installer  │
                └──────────┬───────────┘
                           │  writes
                           ▼
                ┌──────────────────────┐
                │     drover.ini       │  ← single source of truth
                └──────────┬───────────┘
                           │  read on every Discord launch
                           ▼
            ┌──────────────────────────────┐                    ┌──────────────┐
            │   native shim (C++20)        │  hooks WinSock →   │  Discord.exe │
            │                              │                    │              │
            │   Windows: version.dll       │                    │  Chromium /  │
            │      (DLL hijack + MinHook)  │                    │  Electron    │
            │   POSIX:   libredrover_*     │                    └──────────────┘
            │      (LD_PRELOAD / DYLD_     │
            │       INSERT_LIBRARIES)      │
            └──────────────────────────────┘
```

The Windows version is loaded because Discord links `version.dll`
implicitly via the file-version-info API family. Our DLL is mounted
next to `Discord.exe`; Windows' loader prefers same-folder DLLs and
calls our `DllMain` before any of Discord's code runs. We then forward
the 17 standard version.dll exports back to `api-ms-win-core-version-l1-1-0`
via PE export forwarders (no asm trampolines, works on x86 and x64).

For details see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Building from source

**One-liner** (Windows):

```powershell
scripts\build.ps1
```

**One-liner** (Linux / macOS):

```bash
./scripts/build.sh
```

Both scripts produce a ready-to-ship `build-output/` directory.

Tagged GitHub releases contain archives only: Windows x64, Linux x64,
Linux ARM64, and macOS ARM64 packages. The Actions `Release` workflow
can also be run manually with a tag matching the version in `Cargo.toml`.

### Requirements

| Platform | Toolchain                                                                |
| -------- | ------------------------------------------------------------------------ |
| Windows  | Visual Studio 2022 with C++ x86/x64 build tools, CMake 3.20+, Rust stable |
| Linux    | clang 17+ or gcc 13+, CMake 3.20+, Rust stable                           |
| macOS    | Xcode 15+ (Apple clang), CMake 3.20+, Rust stable                        |

gcc 11/12 doesn't have `std::format`. Upgrade to gcc-13 or switch to clang.

See [`docs/BUILDING.md`](docs/BUILDING.md) for flags, troubleshooting, and CI.

## Documentation

| Document                                         | What                                                      |
| ------------------------------------------------ | --------------------------------------------------------- |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)   | Module map, threading, data flow, design decisions        |
| [`docs/PORTING.md`](docs/PORTING.md)             | Cross-platform layout, how to run on Linux/macOS          |
| [`docs/BUILDING.md`](docs/BUILDING.md)           | Build scripts, flags, compiler matrix                     |
| [`docs/ROADMAP.md`](docs/ROADMAP.md)             | Mapping from upstream issue #65 ideas to source files     |
| [`AGENTS.md`](AGENTS.md)                         | Briefing for AI agents and new contributors               |
| [`CONTRIBUTING.md`](CONTRIBUTING.md)             | How to add a feature, code style, PR process              |

## Status

The project is **functional but young**. Known working:

- HTTP proxy with and without Basic auth (TCP).
- SOCKS5 TCP proxy via HTTP-CONNECT to SOCKS5 translation (NO_AUTH method).
- SOCKS5 UDP ASSOCIATE with and without user/pass auth.
- All `classic` / `custom` / `split` / `uae_*` UDP strategies for voice bypass.
- SOCKS5 UDP ASSOCIATE — sync receive path (most TCP responses, some UDP setups).
- Cross-platform compile on Windows / Linux / macOS.

Known gaps (with workarounds):

- Async `WSARecvFrom` (Chromium's IOCP path) is not yet intercepted, so SOCKS5 UDP voice may not work end-to-end on Windows. Tracked in [`docs/ROADMAP.md`](docs/ROADMAP.md).
- The Iced GUI's Discord-folder discovery is Windows-only; on POSIX you launch via `LD_PRELOAD` manually.
- No code-signing for distributed binaries.

## Contributing

PRs welcome. The repo is set up so that **most new features are a single
file change** in `dll/src/` (cross-platform core) plus a single field in
`crates/redrover-config/src/`. See [`CONTRIBUTING.md`](CONTRIBUTING.md)
for the checklist and code style.

## License

[MIT](LICENSE). Acknowledgements to:

- [hdrover/discord-drover] for the original concept and the wire-level decisions we kept compatible with.
- [TsudaKageyu/minhook](https://github.com/TsudaKageyu/minhook) for the hooking primitives.
- [iced-rs/iced](https://github.com/iced-rs/iced) for the GUI framework.
