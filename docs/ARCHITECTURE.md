# Architecture

This document explains how Redrover is wired together: the module
boundaries, the data flow, the threading model, the hook lifecycles on
each OS, and the design decisions behind the non-obvious choices. If
you're new to the codebase, read this once before opening a PR.

The user-facing README is intentionally short. This file is where the
"why" lives.

---

## Table of contents

1. [System overview](#1-system-overview)
2. [Component layers](#2-component-layers)
3. [Build targets and dependencies](#3-build-targets-and-dependencies)
4. [Configuration: the one contract](#4-configuration-the-one-contract)
5. [Lifecycle: Windows (DLL hijack)](#5-lifecycle-windows-dll-hijack)
6. [Lifecycle: POSIX (preload)](#6-lifecycle-posix-preload)
7. [Hook surface](#7-hook-surface)
8. [SOCKS5 UDP relay](#8-socks5-udp-relay)
9. [UDP strategies](#9-udp-strategies)
10. [Threading model](#10-threading-model)
11. [Logging](#11-logging)
12. [Design decisions](#12-design-decisions)
13. [Known limitations](#13-known-limitations)

---

## 1. System overview

```
   ┌──────────────────────────────────┐
   │           redrover.exe           │   Rust + iced (Windows-only Discord folder discovery)
   │      (installer & launcher)      │
   └──────────────┬───────────────────┘
                  │ writes
                  ▼
   ┌──────────────────────────────────┐
   │           drover.ini             │   single source of truth
   └──────────────┬───────────────────┘
                  │ read at every Discord launch
                  ▼
   ┌──────────────────────────────────┐                ┌────────────────────┐
   │      native shim (C++20)         │  intercepts →  │    Discord.exe     │
   │                                  │                │   (Chromium +      │
   │  Windows: version.dll            │                │    Electron)       │
   │     (DLL hijack + MinHook)       │                └────────────────────┘
   │  Linux:   libredrover_preload.so │
   │  macOS:   libredrover_preload.   │
   │           dylib                  │
   └──────────────────────────────────┘
```

The user never interacts with the C++ shim directly. They open the GUI,
type proxy details, click Install, and start Discord. The shim is loaded
by Discord's process automatically — on Windows because Discord links
`version.dll`, on POSIX because the user's launcher exports
`LD_PRELOAD` / `DYLD_INSERT_LIBRARIES`.

## 2. Component layers

The native shim is split into a **portable core** (compiles everywhere)
plus a **per-OS shim** (Windows DLL or POSIX preload library):

```
                      ┌─────────────────────────┐
                      │      redrover_core      │   static lib
                      │   (cross-platform)      │   built on all platforms
                      ├─────────────────────────┤
                      │ platform.cpp            │   typedefs, console, debug_write
                      │ config.cpp              │   drover.ini parser
                      │ proxy_value.cpp         │   proxy URL parser
                      │ socket_manager.cpp      │   mutex + Vec bookkeeping
                      │ udp_strategy.cpp     ⭐ │   pluggable UDP strategies
                      │ socks5_udp.cpp       ⭐ │   SOCKS5 UDP ASSOCIATE (RFC 1928)
                      │ logging.cpp             │   std::format → file + console
                      │ real_io.cpp             │   bridge to unhooked sendto
                      └────────────┬────────────┘
                                   │ static-linked into
              ┌────────────────────┼────────────────────┐
              ▼                                         ▼
   ┌──────────────────────┐               ┌──────────────────────────┐
   │   redrover_dll       │               │   redrover_preload       │
   │   (Windows shared)   │               │   (POSIX shared)         │
   ├──────────────────────┤               ├──────────────────────────┤
   │ dllmain.cpp          │               │ posix_preload.cpp        │
   │ hooks.cpp (MinHook)  │               │   dlsym(RTLD_NEXT, …)    │
   │ exports.cpp          │               │   intercepts socket(),   │
   │   (PE forwarders to  │               │   sendto(), recvfrom()   │
   │    api-ms-win-…)     │               └──────────────────────────┘
   │ discord_dirs.cpp     │
   │   (DLL hijack/spread)│
   └──────────────────────┘
```

The shim's only jobs are:

1. Install the OS-level interception (MinHook on Windows,
   `dlsym(RTLD_NEXT, …)` on POSIX).
2. Resolve "real" function pointers into `redrover::real_io::*` so the
   strategies can bypass our own hooks.
3. Discover the right config directory.
4. Translate OS-level events into calls on `redrover_core` interfaces
   (`IUdpStrategy`, `SocketManager`, `Socks5UdpRelay`).

Everything else — config parsing, strategy logic, SOCKS5 framing — lives
in `redrover_core` and is unit-testable on any platform.

## 3. Build targets and dependencies

### Rust workspace

| Crate              | Role                                                       |
| ------------------ | ---------------------------------------------------------- |
| `redrover-config`  | Pure types + INI parsing. No GUI, no OS calls. `serde` is optional. |
| `redrover-gui`     | iced 0.13 installer; depends on `redrover-config` and `windows-rs` (Windows only, gated by `cfg(windows)`). |

### C++ native shim

| CMake target        | Output                                       | When                |
| ------------------- | -------------------------------------------- | ------------------- |
| `redrover_core`     | static library                               | all platforms       |
| `redrover_dll`      | `version.dll` (x86 or x64)                   | `WIN32`             |
| `redrover_preload`  | `libredrover_preload.{so,dylib}`             | `UNIX`              |

The DLL is built with **static CRT** (`MSVC_RUNTIME_LIBRARY = MultiThreaded`)
so it has zero external dependencies; Windows can't silently reject it
for missing `msvcp140.dll` on the user's machine and fall through to
`System32\version.dll`. We learned this the painful way — see commit
log if you're curious.

### External dependencies

| Where        | What                                  | Why                                        |
| ------------ | ------------------------------------- | ------------------------------------------ |
| DLL          | MinHook                               | inline x86/x64 hooking primitives          |
| DLL          | `ws2_32` / `kernelbase` (system)      | WinSock + version APIs                     |
| Preload      | `libc`, `libdl`                       | `dlsym`, real socket calls                 |
| Rust GUI     | `iced`, `windows-rs`, `rust-ini`, `regex`, `anyhow`, `tracing` | obvious |

Anything else needs justification — see the [Don't](AGENTS.md#dont) section in
AGENTS.md.

## 4. Configuration: the one contract

`drover.ini` is the **only** thing that crosses the Rust / C++ boundary.
There is no IPC, no shared memory, no JSON-RPC. The format is
intentionally compatible with the original Pascal `drover.ini`, with a
few additive sections.

Both halves have their own parser:

- Rust: `crates/redrover-config/src/lib.rs` using the `rust-ini` crate.
- C++: `dll/src/config.cpp`, hand-rolled, **zero third-party deps**. We
  do not pull in `inih`, `tinyini`, or anything else into the injected
  binary.

If you add a field, edit **both** parsers in the same PR. The
[checklist in AGENTS.md](../AGENTS.md#config-knob-checklist) spells it
out.

```ini
[drover]
proxy = …            ; HTTP / SOCKS5 URL, optionally with auth, or empty for Direct

[udp]
strategy            = classic | uae_v1 | uae_v2 | split | custom | off
prefix_delay_ms     = 50
prefix_packets      = 00, 01
split_first         = 0
force_tcp_fallback  = false

[socks5]
udp_associate       = false

[logging]
level   = info
file    = drover.log
console = false
```

### Why duplicated parsers?

Because the DLL must be small, dependency-free, and load fast. The Rust
side has `serde` + `rust-ini`, the C++ side has 130 lines of
hand-rolled INI parsing. The format is plain enough that drift is rare,
and the duplication is the cheap insurance.

## 5. Lifecycle: Windows (DLL hijack)

```
              Discord.exe starts
                    │
                    ▼
     Windows loader walks import table
                    │
            looks up version.dll
                    │
       resolves to ./app-X/version.dll
        (our DLL, because same folder)
                    │
                    ▼
              DllMain(DLL_PROCESS_ATTACH)
              ┌───────────────────────────┐
              │ 1. Write drover-attached. │
              │    txt (marker)           │
              │ 2. Load drover.ini        │
              │ 3. Logger::init           │
              │ 4. hooks::install()       │
              │    (MinHook detours +     │
              │     resolve real funcs)   │
              │ 5. Spawn console-init     │
              │    thread (if enabled)    │
              └───────────────────────────┘
                    │
                    ▼
          Loader returns control
                    │
                    ▼
       Chromium initializes,
       version.dll exports get
       called → PE forwarders ship
       them off to api-ms-win-core
                    │
                    ▼
       Chromium opens its first
       socket → MySocket fires →
       SocketManager::add
                    │
                    ▼
       Chromium sendto / send →
       per-feature hook logic runs
                    │
                    ▼
              … Discord runs normally …
                    │
                    ▼
       Discord auto-updates →
       MyCreateProcessW fires →
       discord_dirs::copy_dll_into_
       all_app_dirs propagates us
       into the new app-Y folder
                    │
                    ▼
       Process exits →
       DllMain(DLL_PROCESS_DETACH) →
       MH_Uninitialize
```

### `version.dll` exports

We export the 17 standard version.dll functions (`GetFileVersionInfoA`,
`VerQueryValueW`, etc.) as **PE-level forwarders** to
`api-ms-win-core-version-l1-1-0`. The Windows API Set Schema resolves
that virtual DLL to the canonical implementation (typically
`kernelbase.dll`). No code in our DLL runs for those calls — the loader
handles everything.

The forwarders are declared via
`#pragma comment(linker, "/EXPORT:Name=api-ms-...,@N")` in
[`exports.cpp`](../dll/src/exports.cpp). We tried a `.def` file first
but MSVC's linker (VS 2022 19.50+) treats the `Name=Module.Func` form
as a local-alias request in some configurations, emitting LNK2001. The
pragma form is unambiguous.

This is significantly cleaner than the original Pascal approach of 17
`__asm jmp [pointer]` trampolines, and it works on x64 (where MSVC
doesn't support inline asm at all).

## 6. Lifecycle: POSIX (preload)

```
        $ LD_PRELOAD=libredrover_preload.so discord
                       │
                       ▼
            dynamic linker maps our .so
            and runs its constructors
                       │
                       ▼
            First call to socket(2):
            our extern "C" socket() is
            found first by symbol search
                       │
                       ▼
            ensure_init() (once):
              dlsym(RTLD_NEXT, "socket")
              dlsym(RTLD_NEXT, "sendto")
              dlsym(RTLD_NEXT, "recvfrom")
              Config::load(~/.config/…)
              Logger::init(…)
              make_strategy(…)
                       │
                       ▼
            real_socket(...) → register
            in SocketManager → return fd
                       │
                       ▼
            ... Discord runs normally ...
            sendto / recvfrom hooks fire,
            apply SOCKS5 wrap/unwrap or
            UDP strategy as configured
```

No "DLL hijack" trick is needed because POSIX dynamic linkage is
explicit. The user's launch command opts in by exporting the env var.

There's no folder-propagation logic on POSIX — Discord on Linux doesn't
use Squirrel-style versioned subfolders.

## 7. Hook surface

Eleven functions, all in `hooks.cpp` (Windows) and the relevant
extern-C overrides in `posix_preload.cpp`:

| Hook                       | Why                                                          |
| -------------------------- | ------------------------------------------------------------ |
| `socket`, `WSASocketW`     | Register the new fd in `SocketManager` (TCP/UDP, timestamp). |
| `WSASend`                  | Inject `Proxy-Authorization: Basic …` on the first send when HTTP+auth is configured. |
| `WSASendTo`                | Per-packet SOCKS5 UDP wrap (if associated) AND first-send UDP strategy (incl. setting up the SOCKS5 association). |
| `send`                     | Convert outgoing `HTTP CONNECT host:port` to SOCKS5 CONNECT on the first TCP send. |
| `recv`                     | Rewrite the SOCKS5 success reply (`05 00 00 …`) back to `HTTP/1.1 200 Connection Established\r\n\r\n` so Chromium thinks the CONNECT worked. |
| `recvfrom`                 | Strip the SOCKS5 UDP wrapper on inbound packets and rewrite source address. |
| `WSARecvFrom`              | Same, sync path only — async/IOCP is currently a known gap. |
| `GetCommandLineW`          | Append `--proxy-server=…` to Discord's command line so Chromium's own proxy stack honors it for HTTP. |
| `GetEnvironmentVariableW`  | Substitute `http_proxy` / `https_proxy` for Chromium's env-based proxy fallback. |
| `CreateProcessW`           | When Squirrel runs `Discord.exe` or `reg.exe` mid-update, copy our DLL into the new `app-*` folder before the child launches. |

The `sendto` function is **not hooked** — we just `GetProcAddress` a
pointer to it so `redrover::real_io::send_udp(...)` can call it directly,
bypassing `MyWSASendTo` and avoiding recursion.

## 8. SOCKS5 UDP relay

```
   ┌──────────────┐                                ┌──────────────┐
   │   Discord    │  sendto(udp_s, buf, dest)     │ SOCKS5 server│
   │              │ ───────────────────────────►  │              │
   └──────────────┘   1) on first sendto on a    │              │
                         new UDP socket:          │              │
                                                  │              │
                      a) hook opens TCP control  ┌┴───┐          │
                         channel to proxy        │TCP │  HS+AUTH │
                         (NO_AUTH or USER_PASS)  └┬───┘          │
                                                  │              │
                      b) sends UDP ASSOCIATE     │              │
                         request, gets back     │              │
                         relay BND.ADDR/PORT    │              │
                                                  │              │
                   2) hook wraps every outgoing  │              │
                      datagram in:               │              │
                      RSV(2) | FRAG | ATYP |    │              │
                      DST.ADDR | DST.PORT | DATA│              │
                      and sends to BND.PORT.    │              │
                                                  │              │
   ┌──────────────┐                              │              │
   │   Discord    │  recvfrom returns:           │              │
   │              │ ◄──────────────────────────  │              │
   │              │   unwrapped payload + real   └──────────────┘
   │              │   source address (relay        ▲
   │              │   stripped). Discord can't      │ relay forwards
   │              │   tell.                         │ to real peer
   └──────────────┘                                ▼
                                          ┌──────────────────┐
                                          │ Discord voice   │
                                          │     server      │
                                          └──────────────────┘
```

Implementation: [`socks5_udp.cpp`](../dll/src/socks5_udp.cpp). Spec:
[RFC 1928 §7](https://datatracker.ietf.org/doc/html/rfc1928#section-7)
for the protocol,
[RFC 1929](https://datatracker.ietf.org/doc/html/rfc1929) for username/
password auth.

The control TCP channel is kept alive for the lifetime of the UDP
socket; closing it tears the association down on the server side.

### Per-socket vs single-association

We use one association per UDP socket. If Discord opens N UDP sockets,
we open N TCP control channels. This is wasteful but simple. A future
optimization could multiplex one association across all sockets — see
the `by_socket_` map for the affected data structure.

## 9. UDP strategies

When the SOCKS5 UDP relay is not configured (or fails to establish),
we fall back to a **strategy** that mangles the first outgoing UDP
packet so DPI on the user's network can't fingerprint Discord voice.

```
    sendto(udp_s, real_packet_74_bytes, …)
                  │
                  ▼
      SocketManager::is_first_send(s)
                  │
              [first time]
                  │
                  ▼
       g_udp_strategy->on_first_send(ctx)
                  │
        ┌─────────┼─────────┐
        ▼         ▼         ▼
     Classic   Configured  UaeV1/UaeV2
        │         │         │
        │      send each   send curated
        │      prefix_     payload from
        │      packets;   uae-v1.bin etc;
        │      sleep;     sleep; then let
        │      then let   real send through
        │      real send
        │      through
        ▼
   send drover-packet.bin
   if present, then 0x00,
   then 0x01, sleep 50ms,
   then let the real
   74-byte packet through
```

Strategies live in [`udp_strategy.cpp`](../dll/src/udp_strategy.cpp).
Adding a new one is a single file change — declare a class implementing
`IUdpStrategy`, register it in `make_strategy()`, and surface its name
in `crates/redrover-config/src/udp.rs`. See the
[Roadmap](ROADMAP.md) for the issue-65 mapping.

## 10. Threading model

- **DllMain runs under loader lock** on Windows. Don't do anything heavy
  in `dllmain.cpp::on_attach` — no LoadLibrary that triggers other
  loads, no AllocConsole, no synchronous network. Anything heavy is
  deferred to a `CreateThread`'d worker.
- **The console-init thread** is one example. `AllocConsole` is unsafe
  in DllMain (it loads conhost), so we trigger it from a worker after
  DllMain returns.
- **`SocketManager`** is `std::mutex`-protected. Lookups are O(N) but
  N is small (a few hundred sockets max, GC'd to entries older than 30s).
- **`Socks5UdpRelay`** has its own mutex around the `by_socket_` map.
  Strategies and hooks acquire it briefly; we hold no lock across
  `sendto` / `recv` calls.
- **`Logger`** uses one mutex for serializing writes (file + console).

## 11. Logging

`dll/src/logging.cpp` is intentionally minimal — no `spdlog`, no `fmt`.
We use `std::format` directly and write to:

1. The log file (`drover.log` next to Discord.exe, configurable).
2. The debug console (when opt-in via `[logging] console = true`).
3. `OutputDebugStringA` on Windows / `stderr` on POSIX (always, for
   tools like DbgView or terminal usage).

Console output uses ANSI escape codes for per-level coloring (ERROR red,
WARN yellow, INFO cyan, DEBUG gray). Windows 10 1607+ supports this via
`ENABLE_VIRTUAL_TERMINAL_PROCESSING`, which we enable on the console
handle.

Conventions:

- INFO: one line per state change / new connection / config event.
  Never per-packet.
- DEBUG: more verbose — payload sizes, per-strategy decisions.
- WARN: degraded operation that the user might want to know about.
- ERROR: things that prevent normal function.

## 12. Design decisions

### Why C++ for the native shim, Rust for the GUI?

The native shim has to be tiny, fast, and load before Discord's main()
runs. C++ with static CRT and minimal includes produces a 100-200 KB
DLL that has zero external dependencies. Rust DLLs on Windows are
viable but the toolchain story for 32-bit shared libraries is rough,
and we'd lose easy linker-level PE export forwarders.

The Rust side is the opposite problem: GUI code that does filesystem,
registry, config validation. Rust + iced gives us a fast, native-feel
installer without GTK/Electron, and the strong type system catches
"forgot to handle the auth check" bugs at compile time.

### Why DLL hijack instead of CreateRemoteThread / SetWindowsHookEx?

DLL hijack is the least invasive option:

- No `OpenProcess(PROCESS_VM_WRITE)` — antivirus doesn't flag us.
- Works without admin rights.
- Discord runs our DllMain **before any of its own code**, so we don't
  miss early socket calls.
- Plays nicely with anti-cheat in adjacent games (we don't touch their
  processes).

The downside is that Discord could in theory move to
`SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)` to defeat us.
If that day comes, see [`docs/ROADMAP.md`](ROADMAP.md) for fallback
strategies.

### Why static CRT?

If our DLL imports `msvcp140.dll` / `vcruntime140.dll` and the user
doesn't have the matching VC++ Redistributable, the Windows loader
silently rejects our DLL and falls back to `System32\version.dll`.
Discord runs without our hooks and proxy doesn't work. Static CRT
costs ~150 KB but guarantees the DLL is self-contained.

### Why two INI parsers?

Trade-off between code duplication and dependency surface. See
[§4](#4-configuration-the-one-contract).

### Why apiset forwarders instead of forwarding to the real version.dll?

If we forward to "version.dll" by name, the OS loader resolves that
to **us** (we're version.dll!) and infinite-recurses. Apiset names
like `api-ms-win-core-version-l1-1-0` are uniquely virtual and resolve
to the canonical implementation regardless of our presence.

### Why MinHook?

It's the smallest, most-vendored x86/x64 inline-hook library that
actually works. No COM, no manifest, no runtime dependencies beyond
`ws2_32`. We pull it via CMake FetchContent so we don't vendor a
fork.

## 13. Known limitations

| Limitation                                            | Workaround / fix path                                      |
| ---------------------------------------------------- | ---------------------------------------------------------- |
| Async `WSARecvFrom` (Chromium IOCP) not intercepted  | Hook `GetQueuedCompletionStatus` or interpose completion routines. Non-trivial; tracked in ROADMAP.md. |
| GUI installer's Discord discovery is Windows-only    | On POSIX, launch with `LD_PRELOAD` manually. A future cross-platform installer mode could do app-bundle inspection. |
| SOCKS5 auth on TCP CONNECT path uses NO_AUTH only    | Matches Pascal behavior. To add: extend `convert_http_to_socks5` with a USER_PASS path mirroring `socks5_authenticate`. |
| No IPv6 in SOCKS5 UDP                                | RFC supports ATYP=0x04. Add branches in `send_via_relay` / `unwrap_reply`. |
| One SOCKS5 association per UDP socket                | Optimization, not correctness. Multiplex via a shared TCP control channel + per-target tracking. |
| No code-signing for distributed binaries             | Out of scope until we have a real release process.        |
