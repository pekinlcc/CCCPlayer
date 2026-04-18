CCCPlayer — how to install
===========================================

This build is NOT signed with an Apple Developer ID (free distribution;
paid $99/yr option has not been set up). On a fresh Mac, macOS Gatekeeper
will refuse to open the app because "the developer cannot be verified".

The installer below sidesteps Gatekeeper by clearing the download-quarantine
flag that macOS attaches to files from the internet.


OPTION A — easiest (double-click install.command)
-------------------------------------------------

  1. Extract this zip if you haven't already (Finder does it on double-click).
  2. Double-click  install.command
     ↳ macOS may say "install.command cannot be opened because the developer
       cannot be verified". If so: right-click install.command → Open →
       confirm "Open" in the dialog. This only happens the first time.
  3. A Terminal window opens and runs a few steps: remove quarantine flag,
     copy CCCPlayer.app to /Applications, launch it.
  4. Done. You can close the Terminal and delete this folder.


OPTION B — from Terminal (if install.command is blocked)
--------------------------------------------------------

  1. Drag CCCPlayer.app to /Applications in Finder.
  2. Open Terminal and run:

        xattr -cr /Applications/CCCPlayer.app
        open /Applications/CCCPlayer.app


OPTION C — right-click → Open, no Terminal
------------------------------------------

  1. Move CCCPlayer.app to /Applications.
  2. In Finder, right-click (or Control-click) CCCPlayer.app.
  3. Choose "Open".
  4. Confirm "Open" in the Gatekeeper dialog.
  5. Subsequent launches are normal double-click.


Prerequisites
-------------

Before you run CCCPlayer, make sure these two CLIs are installed and logged
in on your Mac:

  - Claude Code CLI    (https://docs.anthropic.com/en/docs/claude-code)
  - Codex CLI          (npm install -g @openai/codex, then `codex login`)

CCCPlayer uses YOUR Claude + Codex accounts / tokens. The app itself makes
no network calls.


Source + latest release
-----------------------

  https://github.com/pekinlcc/CCCPlayer
