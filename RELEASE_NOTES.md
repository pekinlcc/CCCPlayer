# CCCPlayer Release Notes

## v1.3.2 · 2026-04-18

Distribution hardening (free tier — no paid Apple Developer account).

### Added

- **Ad-hoc code signing** on every build (`codesign --deep --force
  --sign -`). Does NOT make Gatekeeper trust the app (that requires a
  paid Developer ID + notarization), but it gives the binary a valid
  internal signature so `open` stops logging "broken signature"
  warnings and macOS doesn't re-classify the bundle on every launch.
- **One-click installer bundle** `dist/CCCPlayer-<ver>-arm64-install.zip`
  containing:
    - `CCCPlayer.app` (ad-hoc signed)
    - `install.command` — double-click script that clears the
      `com.apple.quarantine` xattr, copies the app to `/Applications`,
      and launches it. User still has to right-click → Open the first
      time to clear Gatekeeper's warning on the script itself, but
      that's one confirmation vs. typing `xattr` by hand.
    - `README.txt` with three alternate install paths (`install.command`
      / right-click Open / Terminal xattr).
- README now documents the three install options clearly in both EN
  and ZH sections.

### Not done (intentional)

- **No notarization.** Requires an Apple Developer Program membership
  ($99/year). If you set one up: swap `--sign -` in `build-macos.sh`
  for `"Developer ID Application: <Your Name>"` and add
  `xcrun notarytool submit ... --wait && xcrun stapler staple` after
  the dmg is produced.

---

## v1.3.1 · 2026-04-18

Small but visible fixes.

### Fixed

- **Claude token count was ~25× under-reported.** The stream-json
  `usage` object carries four buckets (`input_tokens`,
  `cache_creation_input_tokens`, `cache_read_input_tokens`,
  `output_tokens`); the parser only summed input+output, which for
  Claude Code 2.x is just the "new-input delta" and ignores the ~10k–
  200k cache-read tokens per call. That made Claude look like a
  lightweight (~120k total) vs Codex (~3M) when in reality Claude is
  the heavyweight per PRD §5 (runs 3 of the 4 main phases). The
  comparison was apples-to-oranges: Codex's `tokens used: N` already
  aggregates all buckets. Parser now sums all four for Claude so
  numbers are comparable.
- **Long goals were silently truncated in the Track display.** The
  first-line-only `.slice(0, 60)` meant a real two-sentence goal just
  ended with `…`. Now the full goal wraps in the display area (height
  capped + scrollable) and the element has a `title` tooltip showing
  everything on hover.

---

## v1.3.0 · 2026-04-18

Fixes a class of silent bugs where Codex could run for 100+ rounds
without producing real work (quota exhaustion detection + mandatory new
review-file requirement). Overhauls agent prompts so Claude and Codex
can genuinely disagree, debate, and shelve items rather than blindly
accepting every finding. PRD becomes a living document.

### Prompts (major rewrite)

- **Disagreement protocol**: Claude is no longer required to accept
  every Codex finding. Four possible responses per blocking item now:
  `accepted | partial | rejected | shelved`. Rejecting a finding
  requires a concrete technical reason ("performance X", "API
  constraint Y"), not "I disagree". After the same item has been
  rejected-then-re-raised 2 consecutive rounds with no new argument
  emerging, either agent may mark it SHELVED and both move on.
- **Shelved disagreements** become a first-class concept. Each is
  recorded in `PRD.md` under a new `## Shelved disagreements` section
  with both sides' positions verbatim and a note on why shelving is
  safe for the goal. Neither agent may re-block on a shelved item.
- **Consensus-first, then proximity**: the stated philosophy is to
  close everything both agents agree on, shelve the items they
  philosophically can't agree on, and declare the goal met when nothing
  non-shelved remains. DONE is allowed with non-empty `shelved[]` as
  long as each entry is documented in PRD.
- **PRD is a living document**: implementing and refining prompts now
  explicitly allow — and encourage — updating `PRD.md` mid-stream when
  implementation reveals a design gap. `GOAL.md` remains immutable.
  Solved milestones get checked off, stale "Current state" gets
  rewritten. Goal first; everything else is mutable.
- **Reviewing** now reads the previous review's `## Claude Code 回应`
  section consciously. For `status=rejected` items, Codex must either
  concede or produce a new counter-argument; it cannot simply re-paste
  the same bullet. After 2 rounds of no new argument, Codex MUST shelve.
- **Goal check JSON** gains a `shelved: string[]` field. Done can be
  `true` with shelved non-empty.

### Runtime fixes

- **Codex quota exhaustion detected** (was the root cause of the
  "166 rounds, 28 reviews" zombie loop in real user testing). Runner's
  classifier now recognises `"you've hit your usage limit"`, `"rate
  limit"`, `"quota exceeded"`, `"Retry-After:"`, and a few more as a
  new `TurnOutcome::RateLimited`. This sits above `Crashed` in the
  precedence order so CLIs that exit 0 while printing an ERROR line
  (Codex's behaviour on quota) don't slip through as `Ok`.
- **RateLimited → PAUSED, not ERRORED**. The reducer transitions to
  PAUSED with a descriptive note and parks `retry_at` on `SessionMeta`
  so recovery is automatic.
- **Auto-resume scheduler**. When the orchestrator task finishes and
  the session is PAUSED with a `retry_at`, AppState spawns a tokio
  task that wakes at that time and re-invokes `start` with the same
  `(goal, workdir)`. Respects user intervention: if you stop or start
  a different session in the meantime, the scheduled resume no-ops.
- **Parse `try again at 4:08 AM` and `Retry-After: <seconds>`** from
  the CLI tail so the scheduler has a specific wake time.
- **Reviewing must write a new `codex_review_v{N+1}.md`**. The
  orchestrator now snapshots `latest_review()` before each Reviewing
  turn and, if no higher-numbered file exists after, downgrades the
  outcome to `OutputMalformed` regardless of exit code. Without this
  the orchestrator kept re-ingesting the previous (stale) review as a
  fresh verdict and rounds ticked indefinitely.

### UI

- **Shelved section** in the Remaining-to-goal panel: magenta dashed
  block below the Claude / Codex columns, listing items both agents
  agreed to disagree on. Dedup across both snapshots.
- **Auto-resume countdown pill** in the transport bar whenever the
  session is PAUSED and a `retry_at` is known: `⏳ auto-resume in 2h 13m`
  with tooltip showing the absolute time. Updates every second.

### Schema changes (backward compatible)

- `EventKind::GoalCheck` + `StateCommand::GoalCheckResult` gained
  `shelved: Vec<String>`. Historical events deserialise with empty vec.
- `SessionMeta` gained `retry_at: Option<String>` (RFC3339). Pre-1.3
  session.json files parse unchanged.
- `TurnResult` gained `retry_at: Option<DateTime<Utc>>`.

### Known limitations

- Agent behaviour under the new disagreement protocol depends on how
  well the underlying models follow the prompt. If Claude or Codex
  keeps re-raising a shelved item, manual intervention may still be
  needed — the stagnation detector will catch runaway loops at the
  reducer level.
- Quota exhaustion detection only covers known English substrings. A
  localised error message would still slip through to `Crashed`.

---

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
