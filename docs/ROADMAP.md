# Roadmap — mapping `ISSUE-65-IDEAS.md` to this codebase

Each row links one idea from `../discord-drover/ISSUE-65-IDEAS.md` to the
specific files / functions you need to touch. The columns "Config",
"Rust", "C++" mark which pieces already have scaffolding (✓), are stubs
(⏳), or don't exist yet (✗).

| §   | Idea                                       | Config | Rust GUI                              | C++ DLL                                          |
| --- | ------------------------------------------ | ------ | ------------------------------------- | ------------------------------------------------ |
| 2.1 | Configurable prefix packets + delay        | ✓      | ✓ `view.rs` (delay input)             | ✓ `udp_strategy.cpp::ConfiguredStrategy`         |
| 2.2 | Named strategies                           | ✓      | ✓ `view.rs` (`pick_list`)             | ✓ `udp_strategy.cpp::make_strategy`              |
| 2.3 | Split first packet                         | ✓      | ⏳ add split-count input               | ✓ `ConfiguredStrategy` (uses `split_first`)      |
| 2.4 | Curated `drover-packet.bin` per provider   | ✓      | ⏳ surface in GUI                      | ✓ `udp_strategy.cpp::CuratedStrategy`            |
| 2.5 | Mutate first N packets, not just first one | ✗      | ✗                                     | ✗ change `SocketManager::is_first_send` semantics |
| 2.6 | Force TCP fallback for voice               | ✓      | ✓ `view.rs` checkbox                  | ✓ `udp_strategy.cpp::ForceTcpFallbackStrategy`   |
| 3.1 | UDP-over-TCP (custom server)               | ✗      | ✗ new section in `view.rs`            | ✗ new module                                     |
| 3.2 | SOCKS5 UDP ASSOCIATE                       | ✓      | ✓ `view.rs` checkbox                  | ✓ sync recvfrom; ⏳ async WSARecvFrom IOCP        |
| 3.3 | Local sidecar (drover-helper.exe)          | ✗      | ✗                                     | ✗ would live in a new crate                      |
| 4.1 | WinDivert / TUN opt-in                     | ✗      | ✗                                     | ✗ out of scope until 3.x lands                   |
| 4.2 | Ship curated payloads                      | ✓      | ✓ via `dist/strategies/*.bin`         | ✓ via `discord_dirs::copy_dll_into_all_app_dirs` |
| 4.3 | Diagnostic logging                         | ✓      | ✓ `[logging]` in `drover.ini`         | ✓ `logging.cpp`                                  |

## Suggested implementation order (per the original analysis)

1. **Day-one wins** (§4.2 + §4.3 + README cleanup): ship empty payload
   slots, the logging knob, and a clear "what we do and don't do"
   README. Already in this scaffold.

2. **First real coding session** (§2.1 + §2.2 + §2.3): finish the GUI
   surface for `split_first` and per-strategy preview, write at least
   one non-empty `uae-v*.bin`. Most of the C++ side is already there;
   the GUI is two new `text_input`s.

3. **Next session** (§3.2): implement `Socks5UdpRelay::ensure_association`
   and `send_via_relay` per RFC 1928. This is the architecturally
   correct fix. Tests can use a local SOCKS5 server (e.g. `microsocks`
   with UDP support, `gost`, or a tiny stub) — no need for a real
   Discord environment.

4. **Polish** (§2.6 verification): smoke-test that Discord actually
   falls back to TCP voice when `force_tcp_fallback = true`. If it
   doesn't fall back fast enough, look at hooking the WebRTC layer
   instead of WinSock — but only if needed.

## Files most likely to change per task

For each issue-65 idea, the **central** file to touch (all in
`redrover_core`, so the change applies to all three OSes automatically):

- §2.x (local UDP mangling) → `dll/src/udp_strategy.cpp`.
- §3.x (real proxying) → `dll/src/socks5_udp.cpp` (or a new module).
- §4.3 (logging / diagnostics) → `dll/src/logging.cpp` + the OS shim
  (`hooks.cpp` on Windows, `posix_preload.cpp` on Linux/macOS) for
  trace points at the hook boundary.

GUI changes are almost always:

- `crates/redrover-config/src/{udp,proxy,lib}.rs` (add the field) →
- `crates/redrover-gui/src/app.rs` (add the message + handler) →
- `crates/redrover-gui/src/view.rs` (add the widget).

## Out of scope (deliberately)

- Re-encoding voice in-DLL.
- Bypassing certificate pinning on Discord's HTTPS — we proxy the
  outer TCP, we don't MITM TLS.
- Anything that requires a Windows kernel driver. The point of this
  project is "no drivers, no global VPN, runs from a user account".
