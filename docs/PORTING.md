# Cross-platform porting notes

The C++ side of redrover is split into three CMake targets so platform
specifics stay isolated:

```
                ┌─────────────────────┐
                │   redrover_core     │  static lib — all platforms
                │  (pure logic, no    │
                │   OS interception)  │
                └─────────┬───────────┘
                          │ depends on
        ┌─────────────────┼──────────────────┐
        │                                    │
        ▼                                    ▼
 ┌─────────────────┐               ┌──────────────────────┐
 │  redrover_dll   │               │  redrover_preload    │
 │  Windows only   │               │  Linux / macOS only  │
 │  (version.dll)  │               │  (libredrover_preload│
 │  MinHook +      │               │   .so / .dylib)      │
 │  DLL hijack     │               │  LD_PRELOAD / DYLD   │
 └─────────────────┘               └──────────────────────┘
```

## What's in `redrover_core`

Everything that doesn't need to talk to the OS interception mechanism:

| File              | Purpose                                                  |
| ----------------- | -------------------------------------------------------- |
| `platform.h/cpp`  | Type aliases (`rr_socket_t`), `localtime_safe`, `debug_write` |
| `config.{h,cpp}`  | `drover.ini` parser                                      |
| `proxy_value.{h,cpp}` | Proxy URL parser                                     |
| `socket_manager.{h,cpp}` | Thread-safe per-socket bookkeeping                |
| `udp_strategy.{h,cpp}` | Pluggable UDP first-send strategies                 |
| `socks5_udp.{h,cpp}`  | SOCKS5 UDP ASSOCIATE skeleton                        |
| `logging.{h,cpp}` | Tiny `std::format` logger                                |
| `real_io.{h,cpp}` | Bridge to the unhooked `sendto` (per-OS shim sets the ptr) |

Anything new that doesn't touch hook tables / DLL hijack / dyld interpose
should land here.

## What's in `redrover_dll` (Windows)

| File                 | Purpose                                                |
| -------------------- | ------------------------------------------------------ |
| `dllmain.cpp`        | `DllMain` — runs on Discord process startup            |
| `hooks.cpp/.h`       | MinHook detours for WinSock + process funcs            |
| `exports.cpp`        | `#pragma comment(linker, "/EXPORT:…")` forwarders that redirect every version.dll function to `api-ms-win-core-version-l1-1-0`. No code runs for these — the OS loader handles them. |
| `discord_dirs.{cpp,h}` | Self-propagate into sibling `app-*` folders          |

Builds x64 by default (`-A x64`) since Discord 1.0.9000+ is 64-bit. Pass
`-A Win32` for legacy 32-bit Discord installs. Both bitnesses work
because the version.dll exports are pure PE forwarders, not asm
trampolines.

## What's in `redrover_preload` (POSIX)

| File                  | Purpose                                              |
| --------------------- | ---------------------------------------------------- |
| `posix_preload.cpp`   | LD_PRELOAD / DYLD_INSERT_LIBRARIES hooks for `socket(2)` and `sendto(2)` |

That's it — one file. Everything else is in `redrover_core`.

## Linux

```bash
# Build
./scripts/build.sh

# Use
LD_PRELOAD=$PWD/build-output/libredrover_preload.so discord
```

Config lives in `$REDROVER_DIR` (if set), otherwise `$XDG_CONFIG_HOME/redrover`,
otherwise `~/.config/redrover/drover.ini`.

The Linux Chromium binary (which Discord ships) honors the `http_proxy`,
`https_proxy`, and `all_proxy` environment variables natively, so you
generally don't need redrover to mangle TCP — just `export` the vars and
launch Discord. redrover's value-add on Linux is exclusively the UDP
voice-channel mangling and (eventually) SOCKS5 UDP ASSOCIATE.

If you want force-everything-through-a-proxy with no TCP escape, look at
`proxychains-ng` instead; it composes cleanly with `LD_PRELOAD`.

## macOS

```bash
# Build
./scripts/build.sh

# Use (note: System Integrity Protection considerations apply)
DYLD_INSERT_LIBRARIES=$PWD/build-output/libredrover_preload.dylib \
    DYLD_FORCE_FLAT_NAMESPACE=1 \
    /Applications/Discord.app/Contents/MacOS/Discord
```

Config lives in `~/Library/Application Support/redrover/drover.ini`
(or `$REDROVER_DIR`).

### SIP caveats

- macOS strips `DYLD_*` variables when launching SIP-protected binaries.
  Apple-signed / system-signed apps reject injection. Discord is signed
  by a third-party developer ID, so it normally accepts dyld interpose —
  but Apple is gradually tightening this.
- For released versions of Discord, prefer launching its real Mach-O
  binary directly (`/Applications/Discord.app/Contents/MacOS/Discord`)
  rather than going through the `open(1)` shim — `open` may re-launch
  via launchd and lose the env var.

If injection stops working in a future macOS version, the architectural
alternative is to ship a kernel/network extension or a TUN-based helper.
That's explicitly out of scope (see `docs/ROADMAP.md` §4.1).

## Adding new functionality

Place new code in `redrover_core` unless it must call into MinHook /
dlsym. The platform-specific entry points (`hooks.cpp`, `posix_preload.cpp`)
should stay thin — they translate from "OS event" to "core call" and back.

Things that belong in `redrover_core`:
- Any new UDP strategy.
- SOCKS5 implementation details.
- New config knobs.
- Logging targets.

Things that belong in the per-OS shim:
- Hook installation (MinHook calls on Windows, dlsym on POSIX).
- Resolving real function pointers and wiring them into `real_io`.
- OS-specific config-directory discovery.
- DLL hijack / dyld interpose plumbing.

## Compiler requirements

- C++20 is required (we use `<format>`, `<span>`, `<filesystem>`).
- Tested compilers:
  - MSVC 19.30+ (Visual Studio 2022)
  - clang 17+ (Apple clang from Xcode 15+)
  - gcc 13+
- gcc 11/12 (Ubuntu 22.04 default) is missing `std::format`. Upgrade gcc
  via `apt install gcc-13 g++-13` or use clang.
