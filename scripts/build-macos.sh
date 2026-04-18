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

APP_PATH="$BUNDLE_DIR/macos/CCCPlayer.app"
DMG_PATH=$(find "$BUNDLE_DIR/dmg" -maxdepth 1 -name 'CCCPlayer*.dmg' 2>/dev/null | head -1 || true)

# --- Ad-hoc code signature ---------------------------------------------------
#
# We don't have a paid Apple Developer ID, so we can't produce a notarized
# build that passes Gatekeeper on other Macs cleanly. What we CAN do is
# ad-hoc sign the bundle with `codesign -s -`. This has two effects:
#   - The binary has a valid signature recorded in its header, which some
#     tools and future macOS versions are strict about (otherwise `open`
#     may log "broken signature" warnings).
#   - Consistent identity across rebuilds so the hardened-runtime plumbing
#     inside macOS doesn't mis-classify the app each launch.
#
# Gatekeeper's "cannot be opened because the developer cannot be verified"
# warning is NOT defeated by ad-hoc signing — the installer script shipped
# below clears the quarantine xattr instead.
if [[ -d "$APP_PATH" ]]; then
    echo "Ad-hoc codesigning $APP_PATH ..."
    codesign --force --deep --sign - "$APP_PATH"
    codesign --verify --verbose=2 "$APP_PATH" >/dev/null 2>&1 \
        && echo "  → signature valid (ad-hoc)" \
        || echo "  → warning: ad-hoc signature did not verify"
fi

# --- Bundle install zip ------------------------------------------------------
#
# Produce dist/CCCPlayer-<version>-<arch>-install.zip containing:
#   - CCCPlayer.app (ad-hoc signed)
#   - install.command (double-click script: xattr clear + cp to /Applications
#     + launch)
#   - README.txt (brief install instructions for humans)
#
# End-user flow: download zip → double-click → macOS extracts → double-click
# install.command (right-click → Open the first time to clear Gatekeeper
# warning on the script itself).
VERSION=$(grep -E '^version = "' "$ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')
DIST_DIR="$ROOT/dist"
mkdir -p "$DIST_DIR"
STAGING="$ROOT/target/installer-staging"
rm -rf "$STAGING"
mkdir -p "$STAGING"

if [[ -d "$APP_PATH" ]]; then
    cp -R "$APP_PATH" "$STAGING/"
    cp "$ROOT/scripts/install.command" "$STAGING/install.command"
    chmod +x "$STAGING/install.command"
    cp "$ROOT/scripts/installer-README.txt" "$STAGING/README.txt" 2>/dev/null \
        || echo "(no README.txt template found; zip will ship without it)"

    case "$ARCH" in
        universal) ZIP_NAME="CCCPlayer-${VERSION}-universal-install.zip" ;;
        arm64)     ZIP_NAME="CCCPlayer-${VERSION}-arm64-install.zip" ;;
        x86_64)    ZIP_NAME="CCCPlayer-${VERSION}-x86_64-install.zip" ;;
    esac
    (cd "$STAGING" && zip -qry "$DIST_DIR/$ZIP_NAME" .)
    echo "Packaged installer: $DIST_DIR/$ZIP_NAME"

    # Also drop the plain .app tarball and the .dmg into dist/ if present
    # (so README download links stay stable).
    TAR_NAME="CCCPlayer-${VERSION}-${ARCH//universal/universal}-install.app.tar.gz"
    case "$ARCH" in
        universal) TAR_NAME="CCCPlayer-${VERSION}-universal.app.tar.gz" ;;
        arm64)     TAR_NAME="CCCPlayer-${VERSION}-arm64.app.tar.gz" ;;
        x86_64)    TAR_NAME="CCCPlayer-${VERSION}-x86_64.app.tar.gz" ;;
    esac
    (cd "$BUNDLE_DIR/macos" && tar -czf "$DIST_DIR/$TAR_NAME" CCCPlayer.app)
    if [[ -n "$DMG_PATH" ]]; then
        cp "$DMG_PATH" "$DIST_DIR/CCCPlayer-${VERSION}-${ARCH}.dmg" 2>/dev/null || true
    fi
fi

echo
echo "------------------------------------------------------------"
echo "Build complete. Artifacts:"
find "$BUNDLE_DIR" -maxdepth 3 -name 'CCCPlayer*.app' -o -name 'CCCPlayer*.dmg' 2>/dev/null | sed 's/^/  /'
find "$DIST_DIR" -maxdepth 1 -name "CCCPlayer-${VERSION}-*" 2>/dev/null | sed 's/^/  /'
echo
echo "Distribution options (free, no Apple Developer account):"
echo "  - Easiest:       ship the -install.zip — users double-click install.command"
echo "  - Power users:   ship the .dmg, tell them to run 'xattr -cr /Applications/CCCPlayer.app'"
echo "  - Paid signing:  swap '--sign -' above for 'Developer ID Application: <name>' + xcrun notarytool"
echo "------------------------------------------------------------"
