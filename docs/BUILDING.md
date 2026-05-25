# Building

## One-liner (recommended)

**Windows** — from PowerShell at the repo root:

```powershell
scripts\build.ps1
```

**Linux / macOS** — from any shell:

```bash
./scripts/build.sh
```

Both scripts build the Rust GUI installer **and** the native library
(`version.dll` on Windows, `libredrover_preload.so` on Linux,
`libredrover_preload.dylib` on macOS), then stage everything into
`build-output/`. See [`PORTING.md`](PORTING.md) for the cross-platform
architecture and runtime usage on each OS.

### Windows script flags (`build.ps1` / `build.cmd`)

| Flag                | What it does                                              |
| ------------------- | --------------------------------------------------------- |
| `-Config Debug`     | Build the Debug configuration of both halves              |
| `-Arch Win32`       | Target a legacy 32-bit Discord instead of the x64 default |
| `-Clean`            | Wipe `target/`, `dll/build/`, and `build-output/` first   |
| `-Skip Dll`         | Only build the Rust GUI                                   |
| `-Skip Gui`         | Only build the DLL                                        |
| `-VerboseLog`       | Pass `--verbose` to cargo and cmake                       |

The script lets CMake's Visual Studio generator locate the installed
toolchain directly. It does not invoke `vswhere` or `VsDevCmd`, and it
can be run from an ordinary PowerShell session. `scripts\build.cmd` is
a `cmd.exe`-friendly wrapper.

### POSIX script flags (`build.sh`)

| Flag             | What it does                                              |
| ---------------- | --------------------------------------------------------- |
| `--debug`        | Build Debug instead of Release                            |
| `--clean`        | Wipe `target/`, `dll/build/`, `build-output/` first       |
| `--skip-gui`     | Only build the preload library                            |
| `--skip-native`  | Only build the Rust GUI                                   |
| `--verbose`      | Pass `--verbose` to cmake                                 |

## Prerequisites

- **Rust** (stable). Install via [rustup](https://rustup.rs/).
- **Visual Studio 2022** with the "Desktop development with C++" workload.
  We need the x86/x64 MSVC tools, CMake, and the Windows SDK.
- **CMake ≥ 3.20** (Visual Studio ships its own copy; that's fine).

## Build the GUI installer (`reDrover.exe`)

```powershell
cargo build --release -p redrover-gui
# -> target/release/reDrover.exe
```

For day-to-day development you can also `cargo run -p redrover-gui` and
it will work fine on Linux / macOS: the registry-touching code is gated
behind `cfg(windows)` and returns an empty list elsewhere, so the GUI
launches and renders but "Install" is a no-op.

## Build the DLL (`version.dll`)

```powershell
cd dll
cmake -S . -B build -A x64            # current Discord releases
cmake --build build --config Release
# → build/Release/version.dll
```

The first configure pulls MinHook via `FetchContent`; subsequent builds
are incremental. Use `-A Win32` only for a legacy 32-bit Discord install;
`version.dll` must have the same bitness as `Discord.exe`.

## Run

1. Drop `version.dll` (from the CMake build) next to `reDrover.exe`
   (from cargo).
2. Optionally drop `dist/drover-packet.bin` and any `dist/strategies/*.bin`
   payloads you want available.
3. Launch `reDrover.exe`. Fill in the proxy fields (or pick Direct),
   tweak the UDP strategy if needed, click **Install**.
4. The installer copies `version.dll`, `drover.ini`, and the optional
   payloads into every Discord `app-*` folder it can find via the
   registry. Start Discord normally.

## Test

```powershell
cargo test --workspace
```

The DLL has no Rust-side tests; for the C++ side, the SOCKS5 framing
in `dll/src/socks5_udp.cpp` would be the highest-value target. A future
iteration should bring in [Catch2](https://github.com/catchorg/Catch2)
for unit testing wrap/unwrap round-trips.

## GitHub Actions Releases

The `CI` workflow can be launched manually from the Actions tab to run
the test/build matrix without publishing anything.

The `Release` workflow can be launched manually with a tag matching the
workspace version, for example `v0.1.0`, or is started automatically by
pushing that tag. It publishes archives rather than installers:

| Archive                             | Contents                                |
| ----------------------------------- | --------------------------------------- |
| `reDrover-<version>-windows-x64.zip` | `reDrover.exe`, `version.dll`, config and payload files |
| `reDrover-<version>-linux-x64.tar.gz` | Linux preload library, config and payload files |
| `reDrover-<version>-linux-arm64.tar.gz` | Linux ARM64 preload library, config and payload files |
| `reDrover-<version>-macos-arm64.tar.gz` | macOS ARM64 preload library, config and payload files |

Each package is first uploaded as a workflow artifact, then attached to
the GitHub Release. There is no Windows ARM64 release archive yet:
MinHook-backed `version.dll` is currently supported only on Windows x64.
Empty curated strategy placeholders kept in the source tree are not
included in published archives.

## Troubleshooting

- **`fatal error LNK1112: module machine type ... conflicts with target
  machine type ...`** — clean `dll/build/` and configure again with the
  architecture matching the Discord executable (`-A x64` normally, or
  `-A Win32` for legacy 32-bit installs).
- **MinHook fetch fails** — check your network or pre-populate
  `dll/build/_deps/minhook-src/` from a clean clone.
- **Discord still uses the system DLL** — `version.dll` must live in
  the **exact same folder** as `Discord.exe` (or `DiscordCanary.exe` /
  `DiscordPTB.exe`). The installer handles this; if you're testing
  manually, double-check the path.
