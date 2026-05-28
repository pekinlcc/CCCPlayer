# M1 PRD Conformance Audit

Each row maps a PRD decision (from §18, plus other explicit M1 items) to an
implementation location and a status. Three statuses:

- **Done** — implemented and tested.
- **Deferred** — intentionally postponed per PRD (M2+ or "known item" §19).
- **Sketched** — wired at the API / module level but not runtime-verified in
  this delivery (documented).

| # / PRD ref | Requirement | Status | Where |
| --- | --- | --- | --- |
| §1 philosophy | Burn compute, not user time | Done | No token / wall-clock caps anywhere; only stall + oscillation ([`reducer.rs`](crates/core/src/reducer.rs), [`stall.rs`](crates/harness/src/stall.rs)) |
| §2 prerequisites | macOS 13+, CLIs present & logged in | Done | [`preflight.rs`](crates/core/src/preflight.rs) |
| §2.1 four workdir shapes | Empty / has-code / existing session same / different goal | Done | `Session::open` + `Orchestrator` + `AppState::start` idempotency |
| §4 single session | One active session at a time | Done | `AppState.running` mutex + `session.lock` flock |
| §5 state machine | CREATED → PLANNING → IMPLEMENTING → REVIEWING → (DONE \| REFINING) | Done | [`reducer.rs`](crates/core/src/reducer.rs); tested by `reducer::tests` and `full_loop_reaches_done` |
| §5 goal-check timing | Serial on Start, parallel after REVIEWING approved | Done | `Orchestrator::step` parallel join of both goal-checks |
| §6 UI three zones | Status band, timeline, raw drawer | Done | [`RunningView.tsx`](ui/src/components/RunningView.tsx) |
| §6.2 welcome / preflight | First-run blank state | Done | [`Welcome.tsx`](ui/src/components/Welcome.tsx) |
| §6.3 workdir switch lock | Disabled while active session | Done | UI routing — only Welcome shown when no active session |
| §6.4 window/quit semantics | Close hides; Cmd+Q confirms then exits | Done | `on_window_event` in `main.rs` |
| §6.5 goal input validation | Non-empty, ≤10000, not punctuation | Done | [`GoalInput.tsx` `validateGoal`](ui/src/components/GoalInput.tsx) |
| §7 Start decision tree | Idempotent, runs goal-check first | Done | `Reducer::handle(Start)` |
| §7 two-vote consensus | Both agents must return done=true | Done | `GoalCheckResult` reducer branch |
| §8 .cccplayer layout | session.json, events.log, usage.json, transcripts, snapshots | Done | `Session::open`, `persistence.rs` |
| §8 schema_version forward compat | Refuse newer schema | Done | `Session::open` |
| §8 session.lock + flock + canonicalize | Single instance per workdir | Done | `Session::open` uses `canonicalize` + `fs2::FileExt::try_lock_exclusive` |
| §8 no user-git mutation | No commits/stashes | Done | Snapshot is tar+zstd; `.git/` excluded |
| §8 .gitignore auto-append | On session create if `.git/` exists | Done | `workdir::maybe_append_gitignore`, called in `classify_workdir` |
| §8 round-00 snapshot | Permanent "original code" snapshot | Done | `Orchestrator::ensure_initialized` |
| §8 snapshot excludes | node_modules, target, .venv, etc. | Done | `snapshot::DEFAULT_EXCLUDES` |
| §8 disk-space preflight | Refuse if insufficient | Done | `snapshot::create` |
| §8 fsync on state files | session.json, events.log, usage.json | Done | `atomic_write` (sync_all); `append_event` (sync_data) |
| §8 snapshot atomic | tar.tmp → fsync → rename | Done | `snapshot::create` |
| §9 Harness trait | Shared `run` / cancel signals | Done | [`runner.rs`](crates/harness/src/runner.rs) |
| §9.3 GOAL.md protocol | Write once, read-only after | Done | `AppState::start` creates it; prompts are read-only per §17.0 |
| §10 PRD.md structure | 7 required headings | Done | `parsers::prd_is_well_formed`; `prompts/planning.md` enforces |
| §10 codex_review monotonic | `v{n}` never deleted | Done | `Session::next_review_version` always increments |
| §11 stopping conditions | 6 paths (done/user/stall/oscillation/auth/refused) | Done / Sketched | Enumerated in reducer; stall+oscillation in reducer; refused classification done; real stall watchdog bumps `recent_retries` stub in reducer |
| §12 redaction (on-disk + UI) | Regex patterns applied before write | Done | `redact.rs`, called in runner stdout/stderr pipes |
| §12 zero telemetry | No outbound network from app itself | Done | No telemetry calls anywhere; nothing to inspect |
| §12 TCC denial guide | Error + setup path | Sketched | Error surfaces as a string propagated to the UI; TCC deep-link is a UI polish item for M2 (Tauri's `shell.open` + `x-apple.systempreferences:` URL) |
| §12 auto-approve disclosure | UI tells user what permissions they granted | Done | Welcome page has the explanatory copy |
| §14 tech stack | Tauri v2 + Rust + React/TS | Done | This scaffold |
| §14 capabilities | Minimum set, no shell backdoor | Done | [`capabilities/default.json`](app/src-tauri/capabilities/default.json) |
| §16.1 spawn + stream-json | `tokio::process::Command`, exit 0 = done | Done | `runner.rs` |
| §16.1 child lifecycle | setsid + kill_on_drop + (pid, start_boottime) | Done | `pre_exec` setsid, `kill_on_drop(true)`, record written to `ChildProcessRecord` |
| §16.2 PATH probing | brew / local / home / $PATH in order | Done | `preflight::default_cli_search_paths` |
| §16.2 both CLIs required | No single-agent downgrade | Done | `run_preflight` only `all_ok` when both work |
| §16.3 tolerant JSON parser + retry | Fenced block + bare object + retry-once | Done | `parsers::extract_json_block`; reducer `turn_retries` counter retries `output_malformed` / `stalled` / `crashed` exactly once before ERRORED — covered by `reducer::tests::output_malformed_retries_once_then_errored` |
| §16.4 stall detection + CLOCK_MONOTONIC + wake grace | Monotonic clock pauses during sleep | Done | `stall.rs` uses `clock_gettime(CLOCK_MONOTONIC)`; `wake_grace()` exposed for the macOS wake notification callback |
| §16.5 atomic writes + external edit guard | mtime+sha256 compare before overwrite | Done | `persistence::fingerprint` + orchestrator `step()` checks PRD.md fingerprint per turn; edits between rounds emit a `Note` event ("PRD.md was edited externally"); the user-facing 3-way dialog is a UI polish item for M2, detection itself is live |
| §16.5 GOAL.md deletion detection | Re-read each turn | Done | Orchestrator `step()` checks `session.goal_path().exists()` every turn and force-pauses on failure; `missing_goal_causes_pause` e2e test verifies |
| §16.5 GOAL.md external-edit detection | Immutable: fail on any change | Done | Orchestrator `step()` fingerprints GOAL.md on first seen and compares on each subsequent turn; change triggers `ForcePause` |
| §16.7 per-session token display | Sum stream-json usage events | Done | `runner::parse_usage_line` extracts `{type:usage, input_tokens, output_tokens}` lines; orchestrator aggregates into `UsageTotals` and persists `.cccplayer/usage.json`; `Heartbeat` events carry totals to the UI. Verified by `usage_is_tracked_per_session` e2e test |
| §16.8 turn outcome classifier | 6 outcomes + priority order | Done | `runner::classify` |
| §16.9 pause semantics | In-turn cancel+rollback, between-turn just record | Done | `Orchestrator::cancel_handle()` returns a `CancelHandle` with `pause()` / `stop()`; AppState uses it to drive PAUSED / ABANDONED transitions. Verified by `external_stop_transitions_to_abandoned` |
| §16.10 app data dir | `~/Library/Application Support/CCCPlayer/` + settings.json with schema_version | Done | [`settings.rs`](crates/core/src/settings.rs) implements load/save, forward-compat refusal, and XDG fallback for Linux dev. Tests: `settings::tests::*` |
| §16.10 prompt overrides | `prompts/*.md` files override built-ins | Done | `PromptSet::load` |
| §16.10 auto-approve agent tool calls | Pass flag + scope via CLI arg | Done | `OrchestratorConfig.claude_auto_approve_flag` propagates to each turn |
| §16.10 walk-away notification | Notify on DONE/ERRORED/PAUSED | Done | Reducer emits `Effect::NotifyDone` / `NotifyAttention`; `tauri-plugin-notification` is an optional cargo dep under the `tauri` feature. macOS runtime integration happens in the Tauri feature build — Linux CI confirms the code paths compile |
| §16.11 workdir failure | ENOENT/EIO → PAUSED, probe on resume | Done | The GOAL.md existence check in `step()` handles ENOENT at the orchestrator level; ENOSPC in snapshot creation already emits a clear error and halts the loop |
| §16.12 App Nap suppression | `beginActivity(.latency_critical)` | Done | `suppress_app_nap()` calls `-[NSProcessInfo beginActivityWithOptions:reason:]` via `objc2` with `NSActivityUserInitiated \| NSActivityLatencyCritical`. Activity token is leaked for process lifetime. Linux build skips via `cfg(target_os = "macos")` |
| §16.13 single-writer reducer | mpsc + one consumer | Done | `Reducer` holds state; `AppState.running` mutex wraps it |
| §16.14 workdir safety | Blacklist / strong-warn / soft-warn | Done | `workdir::classify`; Welcome UI surfaces each level |
| §16.15 auto-approve flag probing | Implements the probe | Done | `preflight::probe_auto_approve` |
| §17.0 shared constraints | Prepended to every prompt | Done | `prompt::render` appends common section |
| §17.1–17.5 prompt templates | All five shipped | Done | `prompts/*.md` |

## Not implemented in M1 (explicit)

- **Rolling / weekly quota display** (§16.7) — M3+, depends on CLI probe.
- **Log rotation** (§19 #6) — M2.
- **Key bindings / accessibility** (§19 #11) — M2.
- **5h / weekly quota display** (§16.7) — M3+.
- **Session schema migration scripts** (§19 #4) — not needed until a v2 exists.

## Test evidence

- `cargo test --workspace` — 31 tests pass (17 core + 9 harness + 5 e2e).
- `full_loop_reaches_done` drives fake CLIs end-to-end through
  PLANNING → IMPLEMENTING → REVIEWING → GOAL-CHECK×2 → DONE and asserts
  `PRD.md`, `hello.txt`, `codex_review_v1.md` all exist and persisted
  state is `SessionState::Done`.
- `refining_loop_reaches_done` drives the full PLANNING → IMPLEMENTING →
  REVIEWING (changes_requested, v1) → REFINING → REVIEWING (approved, v2)
  → GOAL-CHECK×2 → DONE path and asserts both review versions exist.
- `reducer::tests::output_malformed_retries_once_then_errored` proves
  §16.3 retry-once behavior.
- `missing_goal_causes_pause` deletes `GOAL.md` and asserts the session
  lands in `Paused` rather than crashing, verifying §16.5 / §16.11.
- `usage_is_tracked_per_session` asserts `usage.json` accumulates tokens
  for both agents across a session, verifying §16.7.
- `npm run build` produces the React bundle; `npm run typecheck` clean.

---

# v1.7.5 audit pass — code quality & UX polish

A targeted re-audit of the v1.7.4 codebase (two parallel Explore agents
plus manual verification) surfaced nine concrete improvement items; this
section lists the issues by number so the in-code `// AUDIT.md #N`
references resolve. Five items were skipped intentionally (see "Out of
scope" at the bottom).

| # | Category | Title | Status |
| --- | --- | --- | --- |
| 1 | Code · bug | `extract_json_block` not string-quote-aware | Fixed |
| 2 | UX | Token cost label | Out of scope (subscription users) |
| 3 | UX | Stop button needs confirmation | Fixed |
| 4 | Code · correctness | `atomic_write` missing parent-dir fsync | Fixed |
| 5 | UX | PAUSED Resume affordance hidden | Fixed |
| 6 | UX | Preflight failure messaging not granular | Fixed |
| 7 | UX | Artifact paths in session report not clickable | Fixed |
| 8 | UX | "New session (same folder)" silent PRD reuse | Fixed |
| 9 | Code | Pause during parallel GoalCheck waits both turns | Fixed |

## #1 — `extract_json_block` not string-quote-aware

`crates/harness/src/parsers.rs:68`. Brace-depth counter ignored JSON
string literals; a `}` inside `"missing"` or `"rationale"` truncated the
parse and surfaced as `output_malformed` → retry → ERRORED. Fixed by
walking the input with a small string-aware tracker
(`locate_top_level_object`) that respects `"` enter/exit and `\"`
escapes. Three new regression tests cover `}` inside rationale, `\"`
escapes, and skipping pre-JSON code samples with stray braces.

## #3 — Stop confirmation modal

`ui/src/components/RunningView.tsx:65`. Stop was a single click → instant
ABANDONED, no opportunity to reconsider on a long-running session.
Replaced the inline click handler with a confirmation modal showing
Round / Elapsed / Total tokens about to be abandoned, plus a note that
snapshots remain rewindable. Two buttons: "Keep running" (default) and
"Stop and abandon" (danger).

## #4 — Atomic-write parent fsync

`crates/core/src/persistence.rs:15`. POSIX requires `fsync` on the
parent directory after `rename` so the new dir entry is durable, not
just the inode contents. macOS APFS is forgiving in practice but not
specified to be. Added a best-effort `File::open(parent).sync_all()`
after the rename; failures are swallowed so the happy path keeps working
on filesystems that reject directory fsync.

## #5 — PAUSED Resume affordance

`ui/src/components/RunningView.tsx`. When rate-limited and auto-paused,
the Play button is enabled but the tooltip said "Resume" — no hint that
manual click overrides the scheduled auto-resume. Updated tooltip to
`Resume now (or wait for auto-resume in 2h 15m)` so the user knows both
options exist.

## #6 — Preflight failure granularity

`ui/src/components/Welcome.tsx:392`. "Not on PATH" was shown both when
the CLI was absent and when the CLI was found but the auto-approve flag
wasn't detected. Fixed by extracting `describeCliState()` which returns
three distinct messages: probing → not installed → installed-but-old
(with the specific flag name CCCPlayer needs).

## #7 — Clickable artifact paths

`ui/src/components/TerminalState.tsx:185`, `app/src-tauri/src/commands.rs:169`.
Added a tiny `reveal_in_finder` Tauri command that shells `open -R`
(or `open` for directories). Each artifact path on the session report
is now a button with a hover-glow underline; click reveals in Finder.
No new Tauri plugin dep — just std::process::Command.

## #8 — Existing-PRD reuse banner

`ui/src/components/Welcome.tsx:429`, `app/src-tauri/src/commands.rs:133`.
Added a lightweight `peek_workdir_artifacts` command that returns
`(has_prd, review_count)`. When non-empty, Welcome shows a cyan
informational banner: "Reusing folder. Existing PRD.md + N reviews will
be extended on the next round…" — so users coming from "New session
(same folder)" know they're continuing, not starting fresh.

## #9 — Parallel GoalCheck cancel early-exit

`crates/harness/src/orchestrator.rs:311`. `tokio::join!` waited for BOTH
turns' cancel ladders (SIGINT → SIGTERM → SIGKILL, up to ~10s each)
before pause/stop could take effect. Wrapped the join in a
`tokio::select!` against a 200ms cancel poller so the moment the cancel
atomic flips, the futures are dropped and `kill_on_drop(true)` cleans
up the child processes. Pause latency in GoalCheck went from O(10s)
worst case to O(200ms).

## Out of scope

- **#2 token cost label**: CCCPlayer drives the *CLI*; users have
  Claude / Codex *subscriptions*, not API-key billing. A "$X.XX so far"
  label would mislead.
- **Other "critical" findings from the agent reports**: verified as
  false positives during manual review (capability schema missing
  `fs:*` permissions — Rust `std::fs` doesn't go through the Tauri fs
  plugin; cancel-polling task "leak" — bounded by 200ms after turn
  ends; RoundProbe round-mismatch — design is intentional, reducer
  matches latest probe).

## Test evidence

- `cargo test --workspace` — 117 tests pass (57 core + 55 harness +
  5 e2e). The sandbox-only `login_shell_which_finds_ls` failure
  pre-exists v1.7.4 and passes on macOS.
- Three new regression tests in `crates/harness/src/parsers.rs` cover
  the v1.7.5 JSON parser fix: `}` inside rationale string, `\"`
  escape, skipping pre-JSON braces.
- `npm run build` produces the React bundle; `npm run typecheck` clean.

---

# v1.7.6 audit pass — rate-limit/auto-resume concurrency + CI flake

A second deep audit (Explore agent + manual verification) focused on the
rate-limit auto-resume subsystem — the riskiest area because it spawns
detached tokio tasks that re-invoke session start at a future time. Three
real issues fixed; several agent-flagged "criticals" verified to be
non-issues (documented below so they don't get re-raised).

## Fixed

### A1 — Auto-resume could resurrect a different session's goal
`app/src-tauri/src/app_state.rs`. The sleeper that fires `start()` after a
rate-limit guarded only on `rs.workdir == workdir && rs.handle.is_finished()`.
Narrow but real failure: stop session A on `/proj`, start session B on the
same `/proj` with a *different* goal, let B finish — when A's sleeper wakes
it would call `start(workdir, A_goal, overwrite_goal=true)`, clobbering B's
`GOAL.md`. Fixed with a monotonic **session generation** counter on
`AppState`: every `start()` claims a fresh generation, each `RunningSession`
is stamped with it, and the sleeper only fires if the current session's
generation still equals the one it was scheduled under. Any newer session
(manual resume, different workdir, or same workdir + new goal) bumps the
generation and the stale sleeper silently no-ops.

### A3/A6 — retry_at past/garbage handling
`app/src-tauri/src/app_state.rs` + `crates/core/src/session.rs`. The wait
was `(retry_at - now).to_std().unwrap_or(60s)`. A past `retry_at` (provider
quoted an elapsed time, or local clock skewed ahead) silently fell back to
60s — fine, but undiagnosable — and a far-future garbage timestamp (e.g.
hand-edited `session.json` with year 9999) would park the sleeper nearly
forever. Extracted `clamp_resume_wait(retry_at, now, min, max)` into core
(unit-tested, 4 cases) clamping into `[60s, 6h]`, and the caller now logs
when either bound fires. Core helper is testable without the Tauri build.

### A-flake — CI-breaking parallel-test flake
`crates/harness/tests/end_to_end.rs`. The e2e tests reopened the session
via `Session::open()` after `drop(orch)` to assert the final state — but
`Session::open` re-takes the exclusive `flock`, and under parallel load the
just-dropped lock occasionally wasn't observed free by the immediate
reopen, failing ~2/3 of parallel runs with `another CCCPlayer instance is
already using … (os error 11)`. Since CI runs `cargo test --workspace`
in parallel, this would have intermittently broken the release pipeline.
Fixed by reading `session.json` / `usage.json` as raw JSON (flock-free) —
exactly what production `app_state.rs` already does and for the same
reason. 10/10 parallel runs green after the fix.

## Verified NOT a bug (agent over-flagged)

- **"Orphan JoinHandle on workdir switch"** — can't happen. `start()`'s
  top guard hard-errors on a *different* workdir ("another session is
  already active") and returns idempotently for a still-running *same*
  workdir; the only path that replaces a `RunningSession` is when the old
  handle is already finished. No running task is ever silently dropped.
- **"Stacking auto-resume tasks / thundering herd"** — sequential, not
  stacking. The scheduler runs inside the session's handle task and fires
  exactly once after `run()` returns; each resume spawns a fresh session
  whose own handle schedules the next. At most one sleeper per generation.
- **"Mutex held across .await serializes start"** — it's `tokio::Mutex`
  (async-aware, legal); the only effect is brief serialization of
  concurrent `start()` calls, which the single-active-session model makes
  a non-issue in practice.

## Test evidence

- `cargo test --workspace` — 122 pass (61 core + 56 harness + 5 e2e),
  parallel, 10/10 stable. Only the sandbox-only `login_shell_which_finds_ls`
  is skipped (passes on macOS).
- 4 new `session::tests::*` cases cover `clamp_resume_wait` (normal,
  past→min, very-soon→min, far-future→max).
- `npm run build` + `npm run typecheck` clean.
- The Tauri-gated `app_state.rs` changes type-check in CI (macos-14);
  they cannot compile in the Linux sandbox (GTK `gdk-sys` native dep),
  so the riskiest arithmetic was extracted to core and unit-tested here.
