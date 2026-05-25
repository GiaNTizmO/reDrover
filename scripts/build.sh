#!/usr/bin/env bash
# reDrover build script - Linux / macOS.
#
# Builds:
#   1. redrover_core         (static lib, internal)
#   2. redrover_preload      (libredrover_preload.so on Linux / .dylib on macOS)
#   3. reDrover GUI (cargo)  -> target/release/reDrover
# Stages everything into build-output/.
#
# Usage:
#   ./scripts/build.sh                  # release
#   ./scripts/build.sh --debug
#   ./scripts/build.sh --clean
#   ./scripts/build.sh --skip-gui       # only the preload .so/.dylib
#   ./scripts/build.sh --skip-native    # only the Rust GUI

set -euo pipefail

CONFIG="Release"
SKIP_GUI=0
SKIP_NATIVE=0
DO_CLEAN=0
VERBOSE=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --debug)        CONFIG="Debug" ;;
        --release)      CONFIG="Release" ;;
        --clean)        DO_CLEAN=1 ;;
        --skip-gui)     SKIP_GUI=1 ;;
        --skip-native)  SKIP_NATIVE=1 ;;
        --verbose)      VERBOSE=1 ;;
        -h|--help)
            grep -E '^#( |$)' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
    esac
    shift
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NATIVE_DIR="$REPO_ROOT/dll"
NATIVE_BUILD_DIR="$NATIVE_DIR/build"
CARGO_TARGET_DIR="$REPO_ROOT/target"
DIST_DIR="$REPO_ROOT/dist"
OUTPUT_DIR="$REPO_ROOT/build-output"

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
fail() { printf '\033[1;31mERROR: %s\033[0m\n' "$*" >&2; exit 1; }

case "$(uname -s)" in
    Linux*)   OS="linux"   ;;
    Darwin*)  OS="macos"   ;;
    *)        fail "Unsupported OS: $(uname -s). Use scripts/build.ps1 on Windows." ;;
esac

step "Detected OS: $OS  (build configuration: $CONFIG)"

# ---- Toolchain checks ------------------------------------------------------

ensure() {
    command -v "$1" >/dev/null 2>&1 || fail "Required tool not found: $1"
}

[[ $SKIP_GUI -eq 1 ]]    || ensure cargo
[[ $SKIP_NATIVE -eq 1 ]] || { ensure cmake; ensure cc; ensure c++; }

# ---- Clean -----------------------------------------------------------------

if [[ $DO_CLEAN -eq 1 ]]; then
    step "Cleaning"
    rm -rf "$NATIVE_BUILD_DIR" "$CARGO_TARGET_DIR" "$OUTPUT_DIR"
fi

# ---- Build native ----------------------------------------------------------

if [[ $SKIP_NATIVE -eq 0 ]]; then
    step "Configuring native library ($CONFIG)"
    cmake -S "$NATIVE_DIR" -B "$NATIVE_BUILD_DIR" \
        -DCMAKE_BUILD_TYPE="$CONFIG"

    step "Building native library"
    if [[ $VERBOSE -eq 1 ]]; then
        cmake --build "$NATIVE_BUILD_DIR" --verbose
    else
        cmake --build "$NATIVE_BUILD_DIR" -- -j"$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)"
    fi
fi

# ---- Build GUI -------------------------------------------------------------

if [[ $SKIP_GUI -eq 0 ]]; then
    step "Building Rust GUI ($CONFIG)"
    pushd "$REPO_ROOT" >/dev/null
    if [[ "$CONFIG" == "Release" ]]; then
        cargo build -p redrover-gui --release
    else
        cargo build -p redrover-gui
    fi
    popd >/dev/null
fi

# ---- Stage -----------------------------------------------------------------

step "Staging artifacts into $OUTPUT_DIR"
rm -rf "$OUTPUT_DIR/strategies"
mkdir -p "$OUTPUT_DIR/strategies"

if [[ "$OS" == "linux" ]]; then
    LIB_NAME="libredrover_preload.so"
else
    LIB_NAME="libredrover_preload.dylib"
fi
LIB_SRC="$NATIVE_BUILD_DIR/$LIB_NAME"

if [[ $SKIP_NATIVE -eq 0 ]]; then
    [[ -f "$LIB_SRC" ]] || fail "Native library not found at $LIB_SRC"
    cp -f "$LIB_SRC" "$OUTPUT_DIR/"
fi

if [[ $SKIP_GUI -eq 0 ]]; then
    if [[ "$CONFIG" == "Release" ]]; then
        GUI_SRC="$CARGO_TARGET_DIR/release/reDrover"
    else
        GUI_SRC="$CARGO_TARGET_DIR/debug/reDrover"
    fi
    [[ -f "$GUI_SRC" ]] || fail "GUI binary not found at $GUI_SRC"
    rm -f "$OUTPUT_DIR/redrover"
    cp -f "$GUI_SRC" "$OUTPUT_DIR/reDrover"
    chmod +x "$OUTPUT_DIR/reDrover"
fi

cp -f "$DIST_DIR/drover.ini" "$OUTPUT_DIR/drover.ini"
# Optional UDP prefix payload used by the `classic` strategy.
if [[ -f "$DIST_DIR/drover-packet.bin" ]]; then
    cp -f "$DIST_DIR/drover-packet.bin" "$OUTPUT_DIR/drover-packet.bin"
fi
if [[ -d "$DIST_DIR/strategies" ]]; then
    find "$DIST_DIR/strategies" -maxdepth 1 -type f \
        \( ! -name '*.bin' -o -size +0c \) \
        -exec cp -f '{}' "$OUTPUT_DIR/strategies/" \;
fi

step "Done"
echo ""
echo "Artifacts in $OUTPUT_DIR:"
( cd "$OUTPUT_DIR" && find . -maxdepth 2 -type f -printf '  %p (%s bytes)\n' 2>/dev/null \
                  || ls -R "$OUTPUT_DIR" )

echo ""
echo "Run examples:"
if [[ "$OS" == "linux" ]]; then
    echo "    LD_PRELOAD=$OUTPUT_DIR/$LIB_NAME discord"
else
    echo "    DYLD_INSERT_LIBRARIES=$OUTPUT_DIR/$LIB_NAME DYLD_FORCE_FLAT_NAMESPACE=1 \\"
    echo "        /Applications/Discord.app/Contents/MacOS/Discord"
fi
