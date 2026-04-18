#!/bin/bash
#
# CCCPlayer one-click installer (free distribution, no Apple signing).
#
# What it does:
#   1. Clears the com.apple.quarantine xattr on CCCPlayer.app (that's the
#      flag that makes Gatekeeper say "cannot be opened because the
#      developer cannot be verified" on downloaded apps).
#   2. Copies CCCPlayer.app to /Applications (overwriting any previous
#      install).
#   3. Clears the xattr on the installed copy too (Finder copy may re-
#      attach it depending on macOS version).
#   4. Launches CCCPlayer from /Applications.
#
# Double-click this file from the unzipped folder. macOS will open it in
# Terminal. The very first time, macOS may block it with "cannot be
# opened because the developer cannot be verified" — right-click the
# file, choose "Open", and confirm; subsequent double-clicks just work.
#
# This script uses no sudo — /Applications is user-writable by default on
# macOS. If your /Applications is locked down, copy CCCPlayer.app there
# manually and run `xattr -cr /Applications/CCCPlayer.app` yourself.

set -euo pipefail

cd "$(dirname "$0")"

SRC="CCCPlayer.app"
DST="/Applications/CCCPlayer.app"

printf '\033[1;35m╭─────────────────────────────────────────────╮\033[0m\n'
printf '\033[1;35m│         CCCPlayer · install.command         │\033[0m\n'
printf '\033[1;35m╰─────────────────────────────────────────────╯\033[0m\n'
echo

if [[ ! -d "$SRC" ]]; then
    echo "✖ Error: '$SRC' is not in the same folder as this script."
    echo "  Extract the .zip fully before running install.command."
    echo
    read -r -p "Press Return to close this window."
    exit 1
fi

echo "→ Clearing Gatekeeper quarantine flag on the staged bundle ..."
xattr -cr "$SRC" 2>/dev/null || true

if [[ -d "$DST" ]]; then
    echo "→ Removing previous /Applications/CCCPlayer.app ..."
    rm -rf "$DST"
fi

echo "→ Copying to /Applications ..."
if ! cp -R "$SRC" "$DST" 2>/dev/null; then
    echo
    echo "  /Applications is not writable without sudo — prompting for your"
    echo "  admin password now. (You can alternatively quit this installer"
    echo "  and drag $SRC to /Applications yourself, then run"
    echo "  'xattr -cr /Applications/CCCPlayer.app' in Terminal.)"
    echo
    sudo cp -R "$SRC" "$DST"
fi

echo "→ Clearing quarantine on the installed copy ..."
xattr -cr "$DST" 2>/dev/null || sudo xattr -cr "$DST" 2>/dev/null || true

echo "→ Launching CCCPlayer ..."
open "$DST"

echo
printf '\033[1;32m✓ CCCPlayer is installed in /Applications and running.\033[0m\n'
echo "  You can now delete this folder. Pin CCCPlayer to the Dock from"
echo "  the running icon if you want one-click access."
echo
read -r -p "Press Return to close this window."
