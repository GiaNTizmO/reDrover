# AGENTS.md

A briefing for AI agents and human contributors walking into this repo
cold. Read once, then keep [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
open while you work.

---

## TL;DR

- **Two binaries, one config file.**
  - `reDrover.exe` — Rust + iced installer GUI (Windows-only Discord folder discovery).
  - `version.dll` (Windows) / `libredrover_preload.{so,dylib}` (POSIX) — C++20 native shim that intercepts WinSock / POSIX socket calls in the running Discord process.
- They communicate **only** through `drover.ini`. Both halves have their own parser; if you add a knob, edit both.
- The Windows DLL is loaded via **DLL hijack** of `version.dll` next to `Discord.exe`. The 17 version.dll exports are forwarded at the PE level to `api-ms-win-core-version-l1-1-0`, so no asm trampolines are needed and the same C++ source builds for both x86 and x64.
- POSIX uses `LD_PRELOAD` (Linux) or `DYLD_INSERT_LIBRARIES` (macOS) and resolves real libc symbols via `dlsym(RTLD_NEXT, …)`.

## Repository map

```
.
├── Cargo.toml                       # Rust workspace
├── LICENSE                          # MIT
├── README.md                        # User-facing
├── AGENTS.md                        # ← you are here
├── CONTRIBUTING.md                  # PR workflow
│
├── crates/
│   ├── redrover-config/             # Shared INI types & parser (Rust)
│   │   └── src/{lib,proxy,udp}.rs
│   └── redrover-gui/                # Iced installer (Rust)
│       └── src/{main,app,view,discord,install}.rs
│
├── dll/                             # C++ native shim
│   ├── CMakeLists.txt
│   └── src/
│       ├── platform.{h,cpp}         # ★ cross-platform abstractions
│       ├── real_io.{h,cpp}          # ★ unhooked-sendto bridge
│       ├── config.{h,cpp}           # drover.ini parser (zero deps)
│       ├── proxy_value.{h,cpp}      # proxy URL parser
│       ├── socket_manager.{h,cpp}   # per-socket bookkeeping
│       ├── udp_strategy.{h,cpp}     # ⭐ pluggable UDP first-send strategies
│       ├── socks5_udp.{h,cpp}       # ⭐ SOCKS5 UDP ASSOCIATE (RFC 1928/1929)
│       ├── logging.{h,cpp}          # tiny std::format logger
│       │
│       │  (Windows-only)
│       ├── dllmain.cpp              # DllMain entry
│       ├── hooks.{h,cpp}            # MinHook detours
│       ├── exports.cpp              # PE-level version.dll forwarders
│       ├── discord_dirs.{h,cpp}     # self-propagate to sibling app-* folders
│       │
│       │  (POSIX-only)
│       └── posix_preload.cpp        # LD_PRELOAD / DYLD_INSERT_LIBRARIES shim
│
├── dist/                            # User-facing reference files
│   ├── drover.ini                   # Annotated reference config
│   ├── drover-packet.bin            # Pre-built UDP payload (1200 bytes)
│   └── strategies/                  # Curated UDP strategy presets
│
├── docs/
│   ├── ARCHITECTURE.md              # Technical design doc
│   ├── PORTING.md                   # Cross-platform layout & runtime usage
│   ├── BUILDING.md                  # Build matrix
│   └── ROADMAP.md                   # Issue #65 → file map
│
└── scripts/
    ├── build.ps1, build.cmd         # Windows build
    └── build.sh                     # Linux / macOS build
```

## Build & test

| Action                  | Command                                       |
| ----------------------- | --------------------------------------------- |
| Full build, Windows     | `scripts\build.ps1`                           |
| Full build, POSIX       | `./scripts/build.sh`                          |
| Just the Rust workspace | `cargo build --release` (from repo root)      |
| Just the native shim    | `cmake -S dll -B dll/build && cmake --build dll/build` |
| Rust unit tests         | `cargo test --workspace`                      |
| Cross-compile sanity    | `cargo check --workspace` on a fresh checkout |

The Windows script delegates Visual Studio toolchain selection to CMake's
Visual Studio generator; it does not invoke `vswhere` or `VsDevCmd`.

## Where to find things

| If you want to…                              | Start at                                                          |
| -------------------------------------------- | ----------------------------------------------------------------- |
| Add a new GUI control                        | `crates/redrover-gui/src/view.rs`                                 |
| Add a new config field                       | `crates/redrover-config/src/lib.rs` **and** `dll/src/config.{h,cpp}` |
| Add a new UDP voice-bypass strategy          | `dll/src/udp_strategy.cpp::make_strategy`                         |
| Modify the SOCKS5 protocol logic             | `dll/src/socks5_udp.cpp`                                          |
| Add a Windows-only socket hook               | `dll/src/hooks.cpp`                                               |
| Add a POSIX-only socket hook                 | `dll/src/posix_preload.cpp`                                       |
| Trace what `version.dll` exports do          | `dll/src/exports.cpp` (PE forwarders to api-ms-win-core-version)  |
| Understand Discord folder discovery          | `crates/redrover-gui/src/discord.rs` (Rust) + `dll/src/discord_dirs.cpp` (C++) |
| See what every drover.ini knob does          | [`dist/drover.ini`](dist/drover.ini)                              |
| See how an issue-65 idea is wired            | [`docs/ROADMAP.md`](docs/ROADMAP.md)                              |

## House rules

### Cross-platform discipline

- Anything that talks to MinHook, `dlsym`, `AllocConsole`, the registry, or `app-*` folders → per-OS file (`hooks.cpp`, `posix_preload.cpp`, `discord_dirs.cpp`, etc.).
- Anything else → core library, uses `redrover::rr_socket_t` / `redrover::platform::*` instead of OS-specific types.
- New socket-touching code should call `redrover::real_io::send_udp(...)` instead of bare `sendto` to avoid recursing through our own hook.

### Config knob checklist

A new field in `drover.ini` requires **three** edits in one PR:

1. `crates/redrover-config/src/lib.rs` — add field, parse, save.
2. `dll/src/config.{h,cpp}` — add field, parse.
3. `dist/drover.ini` — document with a comment block.

If the field should be user-visible, also surface it in
`crates/redrover-gui/src/{app,view}.rs`.

### Don't

- Don't add new C++ dependencies to the DLL without a strong reason. The native shim ships into Discord's address space; every byte and every global initializer matters.
- Don't switch the CRT to dynamic (`/MD`). We use `/MT` so the DLL has zero external dependencies and the Windows loader can't silently reject it.
- Don't replace the C++ shim with a Rust shim "for consistency". 32-bit Rust DLLs in a foreign process are doable but unpleasant; we'd lose the `__pragma(comment(linker, "/EXPORT:..."))` forwarder trick.
- Don't `unwrap()` inside `crates/redrover-gui/src/install.rs`. Installer failures must surface as a friendly error in the GUI, never as a panic.
- Don't pull `spdlog` / `fmt` into the DLL. The custom logger is 80 lines and uses `std::format`; that's enough.

### Logging

Use the macros from `dll/src/logging.h`:

```cpp
LOG_INFO("[udp] strategy initialized: {}", name);
LOG_WARN("[socks5-udp] handshake failed: {}", reason);
LOG_DEBUG("[udp] sending {} prefix bytes", n);
```

Keep INFO event-driven (one line per state change / new connection — never
per-packet). DEBUG can be more verbose; the user opts in via
`[logging] level = debug`.

## Cross-references

- [`README.md`](README.md) — user-facing intro.
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — module diagram and rules.
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — issue-65 idea → file map.
- [`docs/BUILDING.md`](docs/BUILDING.md) — build / run / test.
- [`docs/PORTING.md`](docs/PORTING.md) — per-OS deployment & caveats.
- Upstream brainstorm: [`docs/ISSUE-65-IDEAS.md`](docs/ISSUE-65-IDEAS.md).
