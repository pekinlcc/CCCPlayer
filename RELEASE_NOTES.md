# CCCPlayer Release Notes

## v1.7.2 · 2026-04-20

UI follow-up to v1.7.1. Now that the stall watcher actually works on
macOS and rate-limit auto-pauses fire cleanly, PAUSED sessions surface
far more often — and exposed a long-standing UI bug: the `T+ h:mm:ss`
elapsed clock and `last activity Nm ago` counter kept incrementing
even when the session was PAUSED. A paused session looked live.

### Fixed — UI clocks freeze while session is PAUSED

`RunningView.tsx` had a `setInterval` ticking `elapsed` and `nowTick`
once per second, unconditionally, since v1.1. Gate it on
`sessionState !== 'PAUSED'` so:

- **Elapsed** freezes at the moment of pause; on resume, it continues
  counting from where it left off. Becomes "active time" rather than
  "wall-clock since mount" — matches what users actually want to know.
- **Last activity Nm ago** also freezes because `nowTick` (the memo's
  dependency that drives the relative-time recomputation) stops
  advancing. The memo still re-runs if a genuine event arrives during
  pause (which moves `lastActivityAt`), so fresh activity still
  surfaces naturally.

No other behavior changes. One-line guard on the existing effect plus
a dep-list update.

---

## v1.7.1 · 2026-04-20

Critical bug fix: the stall watcher has been silently disabled on
macOS since v1.0 because of a wrong `CLOCK_MONOTONIC` constant.

### Fixed — stall watcher was never actually running on macOS

`crates/harness/src/stall.rs` hard-coded `CLOCK_MONOTONIC = 1` (the
Linux value). On macOS the real value is 6 — the xnu kernel rejects
clock-id 1 with `EINVAL`, and the existing error handler silently
zeroed the `Timespec`:

```rust
if rc != 0 {
    if !ts.is_null() {
        *ts = Timespec::default();  // ← silent zero
    }
}
```

The effect: `now_ns()` returned 0 every call, `elapsed()` was always
`0.saturating_sub(0) = 0`, and `is_stalled()` never became `true`. So
for every macOS release up to v1.7.0, if a CLI child process wrote
output and then hung (or exited without tokio noticing the pipe
close), the orchestrator would sit waiting **forever** — with no
stall-driven SIGINT / SIGTERM / SIGKILL escalation and no transition
to `PAUSED`/`ERRORED`.

This was observed in the wild on a Hermes Linux session at round 17
REFINING: 169 minutes with zero new events, the session.json stuck
in `RUNNING/REFINING`, and the claude child process gone (exited but
the runner's `tokio::select!` over `child.wait()` never returned, so
`StreamEvent::Finished` was never sent and the UI never learned).

v1.7.1:

- Platform-aware constant — `CLOCK_MONOTONIC = 6` on macOS, `= 1` on
  other unixes.
- Defensive fallback — if `clock_gettime` rejects our clock id on a
  future platform we haven't enumerated, fall back to
  `std::time::Instant`. Wrong semantics (Instant doesn't pause during
  macOS sleep — boottime-ish) is a much cheaper failure mode than a
  permanently-zero clock that silently disables the stall watcher.
- Regression test — `stall::tests::now_ns_is_nonzero_and_monotonic`
  asserts `now_ns() > 0` and monotonic so this exact class of bug
  can't regress hidden.

### Root-cause ownership

The two pre-existing stall tests (`bump_resets_elapsed`,
`crosses_threshold`) were failing on macOS dev machines before this
release; v1.4–v1.7.0 releases shipped despite those failures because
they were misdiagnosed as "flaky / wall-clock sensitive". They were
actually the canary for the real bug. Sorry.

### Tests

- `cargo test -p cccplayer-harness --lib stall` — 3/3 pass (was 1/3
  on macOS in v1.7.0).
- `cargo test --workspace --lib` — 57/58 pass. The one remaining
  failure is `workdir::tmp_blacklisted`, which is an environmental
  flaky (dev machine has a `.key` file in `/tmp`) unrelated to v1.7.1.

### Not in this release

- Reducer-level watchdog ("no events for N × stall_threshold seconds
  → force-pause with reason 'supervisor timeout'") as belt to the
  stall-watcher suspenders. The current fix restores the suspender;
  adding the belt is deferred to a later release.
- Root-cause analysis of why `child.wait()` itself appears to have
  stalled in the Hermes Linux case — without the stall watcher to
  kick the child, there's nothing to observe. With v1.7.1 the stall
  watcher will now fire at 10 minutes; if the underlying `wait()`
  bug is still there, we'll see it surface cleanly as a
  `TurnOutcome::Stalled` instead of a silent hang.

---

## v1.7.0 · 2026-04-20

Fixes a different-shape failure the v1.6 Hermes Linux session made
visible: both agents can **agree** to shelve a core deliverable, then
declare `done=true`, and the loop ends as DONE even though the
user's goal was never actually produced. This is not stagnation — the
loop is "productive" in the v1.6 sense; it's consensus rationalization.

### Changed — Hard deliverables gate (Part A v2)

`PRD.md` gains a new required section: `## Hard deliverables`. PLANNING
writes it from `GOAL.md` during the first planning turn, with one
bullet per concrete, externally-observable required output (files,
commands, end-to-end behaviors). Items once written are **append-only**
across rounds.

Three layers of enforcement sit on top of it:

1. **REFINING — no shelving a hard deliverable.** Before marking any
   item `status: shelved`, Claude must grep `## Hard deliverables`.
   If the shelve candidate substantially names a hard deliverable,
   shelve is forbidden — the gap must be resolved as `accepted`
   (actual delivery) or `partial` (the partial produced + named
   remaining gap). Prompt-level contract.

2. **REVIEWING — missing section is blocking.** If `## Hard
   deliverables` is absent OR empty, Codex's review flags it as a
   blocking finding titled "PRD missing Hard deliverables section".
   If a previously-shelved item turns out to substantially refer to
   a hard deliverable, Codex re-raises it as blocking. Prompt-level
   contract.

3. **GOAL-CHECK — orchestrator-level hard gate (the enforcement teeth).**
   After either agent's goal-check returns, the orchestrator reads
   PRD's `## Hard deliverables` independently and cross-checks each
   entry against the agent's `missing[]` and `shelved[]`. Any match
   overrides `done=true` to `done=false`, prepends a gate message to
   the rationale, and emits a `Note` event naming the matched items.
   **This fires regardless of prompt compliance** — it's the
   belt-and-suspenders that catches a dishonest or confused goal-check.

### The matching algorithm (why not Jaccard here?)

v1.6's stagnation detector uses token-set Jaccard at threshold 0.35.
That algorithm misses the hard-deliverable case completely: the PRD's
hard deliverable uses product language ("Linux distribution ISO file
produced by the build pipeline") while the shelved/missing item uses
gap language ("Full end-to-end ISO artifact is still missing"). Shared
tokens: one (`iso`), Jaccard: 1/16 ≈ 0.06.

v1.7 adds `jaccard::match_any_deliverable`, a **distinguishing-token
overlap** matcher:

- Tokenize both strings via the same tokenizer (lowercase, strip ASCII
  punctuation, filter tokens shorter than 3 chars).
- Drop a curated STOP_WORDS list (articles, copulas, pronouns,
  adverbs, session-meta vocabulary like "missing"/"shelved"/"user",
  and generic tech nouns like "file"/"build"/"code").
- If any remaining distinguishing token appears in both strings, it's
  a match.

The Hermes Linux case now matches via `iso`, as intended. The false
neighbors it was built to reject — "Docker missing" vs "Codex missing"
(shared only "missing", filtered), "flat vs nested config keys" vs
anything ISO-shaped (no overlap) — stay rejected.

Biased toward false positives: a spurious match costs one extra
"rejected done" round, which is cheap. A false negative silently ships
an undelivered goal, which is the exact bug this release exists to fix.

### Added

- `prompts/planning.md` — new `## Hard deliverables` section spec +
  "Hard deliverables rules" body block with sentinel line
  `- no concrete artifact required per GOAL.md` for abstract goals.
- `prompts/refining.md` — MANDATORY grep-before-shelve clause.
- `prompts/reviewing.md` — two new blocking-finding triggers
  (missing section, shelved-hard re-raise).
- `prompts/goal-check.md` — done=true gate rule + hard-deliverables
  walkthrough requirement before answering.
- `jaccard::match_any_deliverable` — distinguishing-token-overlap
  matcher (7 tests covering the real ISO case, multi-deliverable
  indexing, empty-edge-cases, stop-word noise rejection,
  domain-disjoint rejection).
- `parsers::parse_hard_deliverables` + `has_hard_deliverables_section`
  — PRD section extractors tolerating numbered headings, mixed bullet
  prefixes, trailing EOF, case variance, and the sentinel line (8
  tests).
- `orchestrator::hard_deliverable_gate` — per-agent goal-check
  post-processor that runs the gate and emits the Note event.

### Engineering notes

- **The gate fires in the orchestrator, not the reducer.** The reducer
  still sees the gated `done=false` result via `GoalCheckResult` and
  proceeds normally (round advance → REFINING). Keeping the gate in
  the orchestrator means the reducer's contract is "do what the
  command says"; interpretation of what DONE means lives one layer up.
- **Both agents are gated independently.** If Claude says done=true
  and Codex says done=false, and Claude's claim hits the gate,
  Claude's done gets flipped to false. Both agents then correctly
  report done=false and the session continues. The DONE transition
  requires both to say done=true *after* the gate.
- **Deliverables list is empty → gate is a no-op.** If PLANNING
  hasn't written the section (first-round race) or it contains only
  the sentinel (abstract goal), the orchestrator skips the match
  scan entirely. Reviewing will flag the absence separately.

### Tests

- `cargo test -p cccplayer-core jaccard` — 22/22 pass (7 new).
- `cargo test -p cccplayer-harness parsers` — 27/27 pass (8 new).

### Not included in this release

- A UI panel that visualizes the Hard deliverables list on the
  session report. For now the gate's Note event surfaces in the
  highlights timeline; dedicated visualization is deferred.
- Automatic `attempted_alternatives` / `counter_argument` enforcement
  at file-parse time (v1.6 still trusts the prompts).

---

## v1.6.1 · 2026-04-20

Small UX follow-up on top of v1.6.0's GOAL.md conflict modal.

### Changed — existing GOAL.md pre-loads into the textarea

Before: picking a folder that already had `GOAL.md` gave the user no
visible signal that one existed — they'd type a new goal blindly and
only see the conflict modal after hitting Play.

v1.6.1: as soon as the folder is selected (or typed in), CCCPlayer
reads the existing `GOAL.md` and pre-populates the goal textarea with
its content. A small hint under the textarea tells you what's going
on:

- **cyan** "loaded existing GOAL.md — keep to reuse, edit to
  overwrite" when the textarea still matches the file verbatim.
- **amber** "diverged from existing GOAL.md — Play will confirm
  overwrite" once you start typing.

Keeping the pre-loaded content verbatim and hitting Play starts the
session immediately (the backend sees matching content and skips the
rewrite). Editing triggers the same three-way modal from v1.6.0.

### Preserves in-progress edits across folder switches

If you've already started typing a goal for folder A and then switch
to folder B (which also has a GOAL.md), your in-progress goal is left
alone. The pre-load only replaces the textarea when it's empty or
still verbatim what was auto-loaded previously — typed content is
never clobbered without going through the conflict modal.

### Added

- Backend `read_goal_md` Tauri command — returns `Option<String>`
  for the workdir's `GOAL.md` (None when absent; errors are treated
  as absence so the UX affordance is non-fatal).
- `ui/src/api.ts` — `readGoalMd(path): Promise<string | null>`
  wrapper.
- Welcome.tsx — 300ms-debounced `useEffect` on workdir change,
  `autoLoadedGoal` state + stale-closure-safe refs, and the two
  hint states in `.goal-meta`.

### Engineering notes

- Single-owner state: `autoLoadedGoal` tracks the exact string we
  pre-populated so the "is the current goal still auto-loaded"
  comparison is a cheap identity check rather than a fuzzy match.
- No backend schema changes. `start_session`'s existing conflict
  path handles the "user edited the pre-loaded content" case without
  any new machinery.

---

## v1.6.0 · 2026-04-20

Two substantive behaviour changes — one that fixes a quiet data-loss
bug, one that rebuilds the stagnation detector from the ground up
after a real v1.5 session (Hermes Linux, 13 rounds) surfaced the
detector's blind spots.

### Fixed — GOAL.md conflict dialog (Part B)

Before v1.6, starting a session on a workdir that already contained a
`GOAL.md` would **silently ignore the goal the user typed in the UI**
and proceed using the stale on-disk file. The session would then run
with the wrong immutable benchmark — quietly divergent from the
user's mental model.

v1.6 fixes this:

- **Backend** — `start_session` now returns a structured
  `StartError::GoalConflict { existing }` when the workdir's
  `GOAL.md` differs from the provided goal and `overwrite_goal=false`
  (the default). `app_state::AppState::start` gains an
  `overwrite_goal: bool` param; when `true` it atomic-rewrites the
  file. Auto-resume (rate-limit recovery) passes `true` because by
  then GOAL.md is whatever the running session has been executing
  against.
- **Frontend** — a three-way confirmation modal now surfaces the
  conflict: **Use existing** (adopts the disk content as the session
  goal), **Overwrite with new** (rewrites atomically), **Cancel**
  (dismisses without starting). Both Goal contents are shown
  side-by-side in the modal so the user can see what they're
  deciding between.

### Changed — stagnation detector rebuild (Part A)

The v1.5 detector killed the loop whenever the worst-case
`missing[]` count across 3 rounds wasn't strictly decreasing. That's
a single-signal heuristic, and it misfires on sessions where:

- the count is stable but the *items* are cycling through genuinely
  different problems each round;
- the items are repeating but Claude / Codex are actively trying new
  angles (the loop is productive but slow); or
- one agent is done and the other isn't — a 3-round flat stretch is
  the norm, not a deadlock.

v1.6 replaces that with a **triple-gate A ∧ B ∧ ¬C detector over a
5-round window**. All three conditions must hold in the same window
before the detector will fire:

- **A** (count) — worst-case `missing[]` count did not strictly
  decrease across the window.
- **B** (items) — each agent's `missing[]` list is substantially the
  same across every adjacent pair in the window, measured by a
  Jaccard-pair algorithm (threshold 0.35, 80% pair coverage
  in both directions). Calibrated against real v1.5 session data so
  reworded identical items match while single-word-overlap false
  neighbors (e.g. "Docker missing" vs "Codex missing" at J=0.33) do
  not.
- **¬C** (no new angles) — *neither* agent wrote a concrete
  `attempted_alternatives` / `counter_argument` in *any* round of the
  window. Sentinel phrases (`no new angle attempted this round`,
  `conceded; Claude's reason is sound`) do **not** count as concrete
  attempts, so honestly stuck items still register as stuck.

**Safety bias**: the detector returns `None` whenever data is
incomplete (fewer than 5 rounds, or any round missing its attempts
stamp). We never kill sessions on partial information.

The reducer gains a new `StateCommand::RoundAttemptsParsed` that the
orchestrator sends after every successful REFINING turn — at that
point the `codex_review_v{N}.md` file holds both Codex's findings
(with any `counter_argument:` lines) and Claude's just-appended
response section (with any `attempted_alternatives:` lines), so all
three gate signals are known.

When the detector does fire, the reason string sent to the session
report now names the recurring items and suggests shelving them in
PRD's `## Shelved disagreements`, rather than just "we gave up".

### Added

- `crates/core/src/jaccard.rs` — small similarity primitive with
  language-agnostic tokenizer (CJK-safe, no stop words, no
  stemming). 15 unit tests.
- `parsers::parse_round_attempts` — extracts per-round attempt
  signals from a fully-populated `codex_review_v{N}.md`. 7 unit
  tests covering sentinel handling, per-section parsing, and empty
  values.
- `reducer::RoundProbe` replaces the scalar `missing_history:
  Vec<u32>` — records per-agent missing lists, per-agent done flags,
  per-agent tried-new-angle flags, plus an `attempts_recorded`
  stamp so the detector knows when a round is fully scored. 7 new
  stagnation-scenarios tests (kill/don't-kill matrix).
- `reducer::StateCommand::RoundAttemptsParsed` — the new signal path.
- UI: three-way `GoalConflictModal` in Welcome; `ui/src/styles.css`
  gains a `.modal`/`.btn` set (cyberpunk-matched).

### Engineering notes

- The detector runs on `RoundAttemptsParsed`, not on
  `GoalCheckResult`, because ¬C can only be evaluated after Refining
  has written Claude's response section. GoalCheckResult still pushes
  a fresh `RoundProbe` (with `attempts_recorded=false`); the
  subsequent RefiningParsed stamps the bools on and invokes
  `stagnation_reason()`.
- Auto-resume's recursive `start()` call now passes
  `overwrite_goal=true`. Argued safe: after a rate-limit pause the
  session's GOAL.md is whatever the agent's been running against; any
  external edit during the pause is separately caught by §16.11's
  fingerprint guard on the next step.
- No session-file schema changes. An in-flight v1.5.x session
  reopened under v1.6 sees an empty `missing_history` (shape changed
  from `Vec<u32>` to `Vec<RoundProbe>` but the field lives only in
  reducer memory, not in persisted `session.json`).

### Tests

- `cargo test -p cccplayer-core` — 50/51 pass. The single failure is
  the pre-existing `workdir::tmp_blacklisted` test which flakes on
  machines with `*.key` files in `/tmp`; unrelated to v1.6.
- `cargo test -p cccplayer-harness` — parser tests 19/19, fake-cli
  and end-to-end intact. Two `stall::tests` failures pre-date v1.6
  (wall-clock sensitive on loaded machines).

### Not included in this release

- Per-round stagnation context panel on the terminal-state screen
  (duration + stuck items + attempts for the last 5 rounds). The
  existing "reason:" line on the session report picks up the v1.6
  rich reason string so the information is surfaced; a dedicated
  panel with per-round visualisation is deferred.

---

## v1.5.0 · 2026-04-19

Fixes the "CLAUDE CODE CLI: Not on PATH" false negative reported by
users whose claude / codex are installed via nvm, fnm, asdf, volta, or
any other node-version manager that Finder-launched `.app` bundles
can't naturally see.

### Added

- **Login-shell PATH probe** as a fallback in `preflight::find_cli`.
  When the hardcoded-directory search and the process PATH both miss,
  CCCPlayer now runs `$SHELL -l -i -c 'command -v <cli>'` with a 5-
  second timeout to ask the user's actual Terminal shell where the
  binary lives. This catches installs hiding under versioned paths
  like `~/.nvm/versions/node/v20.11.0/bin/claude` that we can't
  hard-code. Login + interactive mode sources `.zprofile`, `.zshrc`,
  `.bash_profile`, and `.bashrc` — the same rc files a Terminal
  window sources — so whatever PATH the user sees in Terminal,
  CCCPlayer sees too.

### Changed

- **`augment_path` scans version-manager dirs dynamically** on
  startup. Added:
  - `~/.nvm/versions/node/*/bin`
  - `~/.local/share/fnm/node-versions/*/installation/bin`
  - `~/.asdf/shims`
  - `~/n/bin`

  These dirs make `node` reachable for `#!/usr/bin/env node` shebang
  resolution at CLI spawn time, even for users whose node is managed
  by a version tool. Empty entries are skipped silently.

### Defense in depth

- `login_shell_which` validates its input name against
  `[a-zA-Z0-9_.-]` before passing it to the shell — no chance of
  shell injection even though callers only pass "claude" / "codex".
- Alias declarations, shell builtins, functions, and relative paths
  returned by `command -v` are rejected — we need a real spawnable
  absolute path to run `<cli> --version`.

### Tests

- 7 new unit tests cover `parse_command_v_output` edge cases (alias
  forms, builtins, functions, relative paths, whitespace, multi-line).
- 1 integration test runs `login_shell_which("ls")` against the host
  shell to verify the plumbing actually works.

### Not included in this release

- Manual "override CLI path" UI — still want this as a safety net for
  users whose login shell can't find things. Planned for a future
  minor if needed.
- CLI doctor panel showing diagnostic details — also deferred.

### Artifacts

- `dist/CCCPlayer-1.5.0-arm64-install.zip`  (~4.5 MB)  ← recommended
- `dist/CCCPlayer-1.5.0-arm64.dmg`           (~5.7 MB)
- `dist/CCCPlayer-1.5.0-arm64.app.tar.gz`    (~4.5 MB)

No schema or runtime-behaviour changes beyond CLI discovery. Existing
sessions resume cleanly; no event-log changes.

---

## v1.4.1 · 2026-04-19

Root-cause follow-up to the v1.4.0 test session where the stagnation
detector fired at round 6 because Claude and Codex reached a stable
disagreement on goal completion — Claude self-shelved items in its
goal_check JSON, Codex kept listing them as missing because they were
never written to PRD. Fixes three separate bugs contributing to the
false stagnation, plus adds a rich execution report on the terminal
screen.

### Fixed

- **Stagnation detector false positive**. `missing_history` used to get
  one push per `GoalCheckResult`, so two-agent disagreement like
  Claude=0 / Codex=3 produced an `[0,3,0,3,0,3]` sequence where the
  last three `[3,0,3]` aren't strictly decreasing, tripping the
  "stagnation" guard after the very first goal check. Now the reducer
  waits until BOTH agents report per round, then pushes a single entry
  equal to `max(claude_missing, codex_missing)`. A real stuck session
  still trips the guard; a unilateral "one agent is stricter" pattern
  no longer does.
- **Stagnation reason was invisible**. The reducer emitted the
  "progress stagnated" string through `Effect::NotifyAttention` only,
  which was discarded by `apply_effects`. The terminal screen then had
  no way to tell the user WHY the session errored. Both stagnation and
  rate-limit branches now additionally emit the reason as a `Note`
  event so it lands in `events.log` and the UI can surface it.
- **`shelved[]` was being invented by goal-check**. The goal-check
  prompt allowed Claude to list items in `shelved` that weren't
  actually in `PRD.md`'s `## Shelved disagreements` section. Since
  Codex's goal-check reads the real PRD, the two agents' views
  diverged forever. Prompt tightened: goal-check is read-only,
  `shelved` must mirror PRD verbatim, and if an item "should" be
  shelved it goes in `missing` with a note asking the next REFINING to
  formalize it.
- **Refining wasn't actually writing shelved items to PRD**. The v1.3
  prompt said to, but Claude in real sessions was marking items as
  `status: shelved` in its response and leaving them only in that
  response. Prompt now includes a **mandatory final grep check**: if
  you marked anything shelved, `PRD.md`'s `## Shelved disagreements`
  section must contain a matching entry before the turn ends.

### Added

- **Session report on the terminal screen** (DONE / STOPPED / ERRORED).
  Replaces the previous one-line banner with:
  - Duration, rounds, per-agent token totals.
  - Final goal check from each agent side by side: done / missing /
    shelved / rationale — so the user can see exactly where the two
    agents agreed or disagreed at the moment the session ended.
  - A `reason` line derived from the most recent stagnation / rate-
    limit / auth note (so "why did it error?" is visible without
    digging into events.log).
  - Highlights timeline: curated list of state transitions, reviews
    written, goal checks, notes, and non-OK agent_finished events. One
    line per moment, color-coded. Routine heartbeats are omitted.
  - Artifacts panel pointing at `.cccplayer/events.log`, `transcripts/`,
    and `snapshots/` inside the workspace.
  - Actions: `New session (same folder)` and `← Back to start`.

### Schema changes (additive, backward compatible)

- `SessionSummary` (UI-side type) now passes a full end-of-session
  snapshot from `RunningView` to `TerminalState`; old sessions that
  navigate to the terminal screen directly gracefully fall back to the
  legacy plain banner.
- No Rust event schema changes. Older `events.log` files replay fine
  (the new UI extraction tolerates missing fields).

---

## v1.4.0 · 2026-04-18

Philosophical overhaul of the prompt layer. Every phase now treats
`GOAL.md` as the only immutable benchmark; everything else — the PRD,
its milestones, its sub-goals, earlier design decisions — is
disposable evidence that can (and should) be revised when it stops
serving the goal.

### Prompts (substantial rewrite)

- **`common.md`** now declares two first-class principles:
  - *GOAL.md is the lens*: every action (plan / implement / review /
    refine) must first ask "does this move us closer to what the user
    asked for?" Actions that can't be tied back to GOAL.md are out of
    scope.
  - *Only GOAL.md is sacred — everything else is revisable evidence*.
    Milestones, sub-goals, earlier design choices, previously
    accepted findings — all are hypotheses. Change them when evidence
    contradicts them. Guardrail: pivots require a concrete reason; "I
    want to try something else" alone isn't enough, to avoid
    thrashing. Every revision gets a one-line Changelog entry in PRD.
- **`planning.md`** gains two new steps:
  - *Step 1 — Goal decomposition*: break `GOAL.md` into the smallest
    set of independently-verifiable sub-goals, each traceable to a
    sentence in GOAL.md. These land in a new `## Sub-goals` section
    of `PRD.md`.
  - *Step 3 — Self-adversarial check*: before saving, re-read GOAL.md
    end to end and verify every sentence has a coverage chain
    sub-goal → scope → design → milestone. Gaps = revise. Also
    required: think about implementation-path feasibility,
    dependency risk, performance / platform constraints.
  - PRD gains a `## Changelog` section. Every later turn that revises
    the document adds a one-liner so the audit trail is intact.
- **`implementing.md`** adds a mandatory first step:
  - Before coding, confirm the next milestone is still the shortest
    path to GOAL.md given new evidence. If not, pivot: mark the
    superseded milestone in PRD, add the replacement, log it in
    Changelog, then implement the NEW next step.
  - Output now includes `goal_anchor: <sub-goal>` and
    `plan_revised: <yes/no>` lines.
- **`reviewing.md`** restructures findings to goal-first:
  - New `## Goal coverage pass` section lists every sub-goal and
    judges `delivered | partial | missing` per sub-goal before any
    code-quality nitpicking.
  - Every blocking finding now requires a `goal_link` field — which
    `GOAL.md` sentence or sub-goal the finding relates to. Findings
    that can't be tied to GOAL.md are automatically non_blocking.
  - New severity `path_drift`: code is doing what PRD says but PRD
    itself has drifted from GOAL.md. Fix is a PRD revision, not a
    code change. `status: approved` requires both `blocking=[]` and
    `path_drift=[]`.
- **`refining.md`** requires a `goal_impact` line on every response:
  - For `accepted` / `partial`: which GOAL.md sub-goal the fix
    advances.
  - For `rejected` / `shelved`: why NOT doing this doesn't harm
    GOAL.md delivery.
  - New status `stale`: the finding refers to a milestone or design
    decision that has been superseded in PRD's Changelog; cite the
    entry and skip.
- **`goal-check.md`** (unchanged) already evaluates against GOAL.md
  directly; it's the model for what the other prompts now inherit.

### Notes on side effects

- Review files and Claude responses will be a bit more structured and
  a bit longer (goal_link, goal_impact, path_drift category). Token
  cost per cycle goes up a small amount in exchange for a cleaner
  audit trail back to GOAL.md.
- Agents are now explicitly allowed to change milestones mid-session.
  There's a non-zero risk of thrashing — if observed in practice the
  guardrail wording in `common.md` can be tightened further in a
  follow-up release.

### Docs

- `PRD.md`: decisions 79-83 cover the philosophy change, goal
  decomposition, goal-anchored milestones, goal-first review, and
  goal_impact field respectively.
- `README.md`: EN + ZH "What's new in v1.4" sections added.

### Artifacts

- `dist/CCCPlayer-1.4.0-arm64-install.zip`  (recommended)
- `dist/CCCPlayer-1.4.0-arm64.dmg`
- `dist/CCCPlayer-1.4.0-arm64.app.tar.gz`

No breaking runtime changes — event schema and session.json format
unchanged from v1.3.x. Existing sessions resume cleanly; the new
prompts only take effect on turns spawned by this build.

---

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
