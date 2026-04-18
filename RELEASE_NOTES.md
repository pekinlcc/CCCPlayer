# CCCPlayer Release Notes

## v1.2.0 · 2026-04-18

Makes the "distance to goal" panel actually update per cycle, fixes the
pause/stop UX so clicks feel responsive, and adds a Resume button so a
paused session can be continued without losing state.

### Changed

- **GOAL_CHECK now runs after every REVIEWING**, regardless of verdict.
  Previously the state machine only entered `GoalCheck` phase when Codex
  returned `Approved` — which in practice could never happen for dozens
  of rounds (Codex kept finding one blocking item, loop stayed in
  `REVIEWING ↔ REFINING`). The Remaining-to-goal panel kept showing
  "waiting for first goal check" all the way to round 167+ in real user
  testing. Now every `REVIEWING → (verdict)` cycle launches GOAL_CHECK on
  both agents in parallel, so the panel refreshes every 2–10 minutes.
  DONE still gates on both agents independently reporting `done=true`.

  Trade-off: +2 CLI invocations per round (one Claude goal-check, one
  Codex goal-check), each short. Worth it for the continuous signal.

  `Blocked` verdict still skips straight to Refining (unchanged) — if
  the review machinery itself is malformed, asking "are we done?" at
  that point is pointless.

### Added

- **Resume button.** Once a session enters `PAUSED`, the big triangle
  Play button in the transport lights up again as Resume; clicking it
  calls `startSession(goal, workdir)` (idempotent on the AppState side)
  so the orchestrator picks up from the recorded state/phase. Before
  this, pause was effectively a one-way gate into Stop territory.
- **Optimistic transport feedback.** Clicking Pause or Stop now
  immediately shows `⏸ PAUSING…` / `■ STOPPING…` in the title-bar badge
  and disables the button, instead of waiting 5–10 seconds for the
  orchestrator to actually unwind the current turn. The label clears
  itself once the reducer confirms the real `state_changed` event.

### Fixed

- Pause and Stop appeared to "do nothing" because the orchestrator
  needs up to 5s (SIGINT grace) + next step-loop iteration to actually
  transition state, and the UI gave no in-between signal. This is now
  fixed by the optimistic feedback above.

### Known limitations

- Rate-limit / quota-exhaustion auto-pause still not implemented. When
  a CLI's 5-hour quota runs out you still see a crash loop until the
  flapping limit triggers ERRORED. Planned for v1.3.

---

## v1.1.0 · 2026-04-18

Focused on runtime correctness, making "how close are we to done?" legible
at a glance, and UX polish.

### Added

- **Remaining to goal panel** — new Progress section that shows the latest
  `goal_check` result from each agent side-by-side, plus a top-level
  agreement badge:
  - `✓ both agree · goal met` (terminal)
  - `◐ 1 agent done · 1 disagrees`
  - `✖ both say not done`
  - `○ waiting for first goal check`

  Each column shows that agent's `missing[]` list + `rationale`, round
  number, and age. No fuzzy cross-matching — the PRD contract is that both
  must independently answer `done=true` to transition to `DONE`, so we
  surface exactly that contract.
- **Per-agent token labels** — the Tokens KV now shows two labeled rows
  (`Claude 120,360` / `Codex 3,021,158`) instead of a single summed pair,
  so you can tell where the cost is going.
- **Native folder picker** — the FOLDER row grows a `Browse…` button that
  opens macOS's native directory picker via `tauri-plugin-dialog`.
- **Version string in shell footer** reflects the actual app version.
- **Extra outcome icons in title-bar status badge** — `⏸ PAUSED`, `✖ ERRORED`,
  `■ STOPPED`, `✓ DONE` alongside the existing `▶ RUNNING` / `◇ NO SESSION`.

### Fixed

- **State badge + phase display stuck at "PLANNING"** — the reducer emits
  `state_changed` payloads as Rust `Debug` (CamelCase like
  `Running/Implementing`), but the UI regex required all-uppercase and
  never matched. Result: every state transition was silently dropped,
  phase stayed at its initial value, and `onDone` for Errored / Done /
  Abandoned never fired. A session that had actually gone to `Errored`
  would still show `▶ RUNNING` in the title bar, making pause buttons
  appear broken even though there was just nothing live to pause. Regex
  is now case-insensitive and CamelCase is normalised to
  SCREAMING_SNAKE_CASE.
- **Event schema**: `GoalCheck` events now carry the full `missing[]`
  list and `rationale` (previously only `missing_count`). Older event
  logs (pre-v1.1) still parse — the new fields default to empty.

### Changed

- The Tauri app now depends on `tauri-plugin-dialog` (~40 KB in the
  bundle). Capability `dialog:allow-open` added.
- `StateCommand::GoalCheckResult` signature gained `missing: Vec<String>`
  and `rationale: String`. Callers updated; tests updated.

### Known limitations

- Goal-check data only refreshes at the end of each REVIEWING → REFINING
  → GOAL_CHECK cycle (every 5–30 min in steady state). Between cycles
  the Remaining panel shows the previous snapshot with a "round N · Nm
  ago" age tag.
- Rate-limit / quota-exhaustion auto-pause is not yet implemented (the
  agent crash-loops 140× scenario we saw in user testing still ends at
  `ERRORED` rather than `PAUSED-with-auto-resume`). Planned for v1.2.

---

## v1.0.0 · 2026-04-17

Initial public release. Retroactively labeled — this is the baseline
captured right after M1 scope closed out.

### Core capabilities

- **Two-agent loop**: Claude Code and Codex drive a shared
  `PRD.md` + `codex_review_v{N}.md` cycle end-to-end. State machine
  (`PLANNING → IMPLEMENTING → REVIEWING → REFINING → GOAL_CHECK`) lives
  in a single-writer reducer in `crates/core`.
- **macOS desktop app** built on Tauri 2 with a Winamp-style chrome
  shell + cyberpunk neon theme (cyan / magenta / lime on near-black).
- **Triangle / double-bar / square transport controls** mapping to Play,
  Pause, Stop — idempotent per PRD §7.
- **Structured event timeline + live raw stdout/stderr drawer** — two
  Tauri event channels (`cccplayer://event` structured,
  `cccplayer://raw` per-line), ring-buffered to 2000 lines in the UI.
- **Safe-by-default workspace classifier** — home / system paths are
  refused or require explicit confirmation; `.cccplayer/` is
  auto-added to `.gitignore`.
- **Round snapshots** — every round start takes a `tar.zst` snapshot
  into `.cccplayer/snapshots/`; `round-00` is the user's original
  code and is never deleted.
- **App Nap suppression** (macOS) via `NSProcessInfo.beginActivity`
  so long runs don't get throttled while the window is hidden.
- **Finder-launched PATH augmentation** — the app prepends Homebrew /
  nvm / volta / bun / `~/.cargo/bin` / `~/.local/bin` to the process
  PATH at startup so `#!/usr/bin/env node` scripts (Codex) can find
  `node` without a Terminal-inherited environment.
- **Zero telemetry** — the app itself makes no network calls. All
  tokens are billed to the user's own Claude + Codex accounts.

### Shipped adaptations

- Claude Code 2.x: `-p` with `--output-format stream-json` requires
  `--verbose`; usage is parsed from nested
  `{"type":"assistant","message":{...,"usage":{...}}}`.
- Codex 0.1x: invoked as `codex exec <prompt>`; tokens scraped from
  the trailing plain-text `tokens used\n<N>` pair.

### Known issues in 1.0

Fixed in 1.1. See above.
