# `version.dll` (Redrover DLL)

The injected component. See `../docs/ARCHITECTURE.md` for the full
picture. This README only covers DLL-local concerns.

## Build

```powershell
cmake -S . -B build -A x64
cmake --build build --config Release
```

Output: `build/Release/version.dll`.
Use `-A Win32` only when targeting a legacy 32-bit Discord executable; the DLL bitness must match its host process.

## Layout

```
src/
├── platform.{h,cpp}   Cross-platform abstractions (rr_socket_t, console)
├── real_io.{h,cpp}    Bridge to unhooked sendto (Win/POSIX)
├── dllmain.cpp        DllMain entry point (Win)
├── hooks.{h,cpp}      MinHook wrappers; all detours live here (Win)
├── exports.cpp        PE-level forwarders for the 17 version.dll exports (Win)
├── socket_manager.*   Per-socket bookkeeping
├── config.{h,cpp}     drover.ini parser (zero deps)
├── proxy_value.*      Proxy URL parser
├── discord_dirs.*     Discover sibling app-* folders for self-spread (Win)
├── udp_strategy.*  ⭐ Pluggable UDP first-send strategies (issue #65 §2)
├── socks5_udp.*    ⭐ SOCKS5 UDP ASSOCIATE relay (issue #65 §3.2)
├── logging.{h,cpp}    Tiny std::format logger
└── posix_preload.cpp  LD_PRELOAD / DYLD_INSERT_LIBRARIES shim (Linux/macOS)
```

## Adding a new UDP strategy

1. Declare it in `config.h` (the `UdpStrategy` enum) and recognize the
   name in `config.cpp::parse_udp_strategy`.
2. Add a class implementing `IUdpStrategy` in `udp_strategy.cpp`.
3. Plug it into `make_strategy()`.
4. Surface it in `crates/redrover-config/src/udp.rs` and
   `crates/redrover-gui/src/view.rs` so the GUI can write it.

That's it — `hooks.cpp::MyWSASendTo` doesn't need changes.

## SOCKS5 UDP ASSOCIATE — status

Implemented end-to-end for the **synchronous** WinSock path (RFC 1928 §7,
plus RFC 1929 username/password subnegotiation):

- `ensure_association(udp_sock)` — TCP control channel + handshake + parses
  the relay endpoint from the server's reply.
- `send_via_relay(...)` — wraps every outgoing UDP datagram with the
  SOCKS5 UDP header and sends it to the relay (per-packet, not just first).
- `unwrap_reply(...)` — strips the header from inbound packets and
  rewrites the source address so Discord sees the original peer.
- `recvfrom` and `WSARecvFrom` (sync) are hooked.

**Known limitation:** `WSARecvFrom` with `LPWSAOVERLAPPED` /
`LPWSAOVERLAPPED_COMPLETION_ROUTINE` (the async/IOCP path) is **not**
intercepted yet. If Chromium uses async receive for voice (likely),
unwrap won't fire and Discord will see SOCKS5-wrapped garbage. A single
WARN line is emitted the first time this is observed. Implementing this
properly requires interposing IOCP completion routines / wrapping
`GetQueuedCompletionStatus` — a meaningful chunk of work.
