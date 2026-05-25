# Contributing

Thanks for considering a contribution. Redrover is small and welcomes
focused PRs.

## Quick orientation

Before touching anything, skim:

- [`README.md`](README.md) — what the project is.
- [`AGENTS.md`](AGENTS.md) — the repo map and house rules.
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — the technical doc.

## Setup

```bash
# Rust workspace
cargo build --release
cargo test --workspace

# Native shim (Windows / 64-bit)
scripts\build.ps1

# Native shim (Linux / macOS)
./scripts/build.sh
```

Required toolchains are listed in [`docs/BUILDING.md`](docs/BUILDING.md).

## Code style

**Rust**

- `rustfmt` defaults. CI runs `cargo fmt --check`.
- `clippy` warnings are errors in CI — fix them or `#[allow(...)]` with
  a comment.
- No `unwrap()` in `crates/redrover-gui/src/install.rs` — installer
  errors must reach the GUI as text.

**C++**

- C++20. `std::format`, `<span>`, `<filesystem>` are fair game.
- Cross-platform code uses `redrover::rr_socket_t` and friends from
  `dll/src/platform.h`. Don't reach for `SOCKET` / `int` directly.
- New socket I/O calls `redrover::real_io::send_udp(...)` to avoid
  recursing through our own hooks.
- Logging: `LOG_INFO(...)` for state changes (never per-packet);
  `LOG_DEBUG(...)` for verbose detail.

## The config-knob checklist

Adding a field to `drover.ini` is the single most common PR. Three
edits, one PR:

1. **Rust types** — `crates/redrover-config/src/lib.rs`. Add the field
   to `DroverOptions`, parse it in `from_ini`, write it in `save`.
2. **C++ types** — `dll/src/config.h` (struct field) and
   `dll/src/config.cpp` (parse).
3. **Documentation** — `dist/drover.ini` with a comment block.

If the field should be user-visible, also surface it in
`crates/redrover-gui/src/{app,view}.rs` (Message variant + form
control).

## Adding a UDP strategy (issue-65)

A new entry in [`docs/ROADMAP.md`](docs/ROADMAP.md):

1. Add the variant to `UdpStrategy` in `crates/redrover-config/src/udp.rs`
   and `dll/src/config.h::UdpStrategy`. Update `parse_udp_strategy` and
   `udp_strategy_name` in both sides.
2. Create a class implementing `IUdpStrategy` in
   `dll/src/udp_strategy.cpp`. Call `log_first_send(...)` at the top of
   `on_first_send` to keep diagnostics consistent.
3. Plug into `make_strategy()`.
4. (Optional) Ship a curated payload in `dist/strategies/your-strategy.bin`.

That's it — the hook layer doesn't need changes.

## Adding a Windows hook

1. Add the `Type_t` typedef in `dll/src/hooks.cpp`.
2. Add the `Real*` global pointer.
3. Write `MyFunc` with the same signature.
4. Add a `create_hook("...", "FuncName", reinterpret_cast<void*>(&MyFunc), real_func)`
   line in `hooks::install()`.

If the function exists on POSIX too, mirror it in `posix_preload.cpp`
using `dlsym(RTLD_NEXT, "FuncName")`.

## Testing

- `cargo test --workspace` should pass.
- For C++ logic, unit tests via [Catch2](https://github.com/catchorg/Catch2)
  are encouraged but not yet wired up. If you add tests, please also
  add them to CI.
- For the DLL, manual smoke testing is the norm:
  - Build, install via GUI, launch Discord, send a message (TCP works),
    join a voice channel (UDP works).
  - Check `drover.log` for the expected `[udp] first send …` line.

## PR process

1. Fork and create a feature branch.
2. Make the change. Update docs in the same PR.
3. Run `cargo fmt`, `cargo clippy`, `cargo test`.
4. For native changes, build and smoke-test locally.
5. Open the PR with a short description and a checkbox list of what you
   verified.

For non-trivial changes, a draft PR or issue first is appreciated so we
can align on the approach.

## Bug reports

Please include:

- OS + version (Windows 10 22H2, Ubuntu 24.04, etc.).
- Discord build number (Help → About).
- Contents of `drover.ini` (redact credentials).
- Contents of `drover.log` (with `[logging] level = debug` if possible).
- For Windows: a Process Monitor capture filtered to
  `Path contains version.dll AND ProcessName = Discord.exe` if the DLL
  appears not to load.
