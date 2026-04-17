#!/usr/bin/env bash
# Build the CCCPlayer macOS .app bundle. Must be run on macOS (Apple SDK is
# Apple-only; no cross-compile path from Linux).
#
# Usage:
#   ./scripts/build-macos.sh                 # release build, universal
#   ./scripts/build-macos.sh --arch arm64    # release build, Apple Silicon only
#   ./scripts/build-macos.sh --arch x86_64   # release build, Intel only
#   ./scripts/build-macos.sh --dev           # debug dev-run (cargo tauri dev)

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

if [[ "$(uname)" != "Darwin" ]]; then
    echo "error: this script must be run on macOS (needs Apple SDK)." >&2
    exit 1
fi

ARCH="universal"
MODE="release"
for arg in "$@"; do
    case "$arg" in
        --arch) shift; ARCH="$1"; shift ;;
        --arch=*) ARCH="${arg#*=}" ;;
        arm64|x86_64|universal) ARCH="$arg" ;;
        --dev) MODE="dev" ;;
        -h|--help)
            sed -n '3,11p' "$0"
            exit 0
            ;;
    esac
done

# --- Preflight ---------------------------------------------------------------

need() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "error: '$1' not found on PATH. Install it and re-run." >&2
        exit 1
    fi
}
need cargo
need node
need npm
need xcrun  # confirms Command Line Tools / Xcode

# Install rustup targets for a universal build.
if [[ "$ARCH" == "universal" ]]; then
    rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
elif [[ "$ARCH" == "arm64" ]]; then
    rustup target add aarch64-apple-darwin >/dev/null
elif [[ "$ARCH" == "x86_64" ]]; then
    rustup target add x86_64-apple-darwin >/dev/null
else
    echo "error: unknown --arch '$ARCH'. Use arm64, x86_64, or universal." >&2
    exit 1
fi

# Tauri CLI. Keep it pinned to 2.x.
if ! command -v cargo-tauri >/dev/null 2>&1 && ! cargo tauri --version >/dev/null 2>&1; then
    echo "Installing Tauri CLI (cargo-tauri) ..."
    cargo install tauri-cli --version "^2"
fi

# --- Frontend ----------------------------------------------------------------

echo "Installing UI dependencies ..."
pushd ui >/dev/null
if [[ -f package-lock.json ]]; then
    npm ci
else
    npm install
fi
popd >/dev/null

# --- Build -------------------------------------------------------------------

cd "$ROOT/app/src-tauri"

if [[ "$MODE" == "dev" ]]; then
    echo "Launching cargo tauri dev (Cmd+Q to exit) ..."
    exec cargo tauri dev --features tauri
fi

case "$ARCH" in
    universal)
        echo "Building universal release bundle ..."
        cargo tauri build --features tauri --target universal-apple-darwin
        ;;
    arm64)
        echo "Building arm64 release bundle ..."
        cargo tauri build --features tauri --target aarch64-apple-darwin
        ;;
    x86_64)
        echo "Building x86_64 release bundle ..."
        cargo tauri build --features tauri --target x86_64-apple-darwin
        ;;
esac

# --- Locate artifacts --------------------------------------------------------

BUNDLE_DIR="$ROOT/target"
case "$ARCH" in
    universal) BUNDLE_DIR="$BUNDLE_DIR/universal-apple-darwin/release/bundle" ;;
    arm64)     BUNDLE_DIR="$BUNDLE_DIR/aarch64-apple-darwin/release/bundle" ;;
    x86_64)    BUNDLE_DIR="$BUNDLE_DIR/x86_64-apple-darwin/release/bundle" ;;
esac

echo
echo "------------------------------------------------------------"
echo "Build complete. Artifacts:"
find "$BUNDLE_DIR" -maxdepth 3 -name 'CCCPlayer*.app' -o -name 'CCCPlayer*.dmg' 2>/dev/null | sed 's/^/  /'
echo
echo "Next steps:"
echo "  - Launch:         open '$BUNDLE_DIR/macos/CCCPlayer.app'"
echo "  - Code sign:      codesign --deep --force --sign \"Developer ID Application: YOUR NAME\" <path>.app"
echo "  - Notarize:       xcrun notarytool submit <path>.dmg --keychain-profile \"notary\" --wait"
echo "  - First run:      right-click → Open, or 'spctl --add' to bypass Gatekeeper warnings."
echo "------------------------------------------------------------"
