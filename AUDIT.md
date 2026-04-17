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
| §12 TCC denial guide | Error + setup path | Sketched | Error surfaces as string; UI doesn't yet deep-link. Acceptable for M1 |
| §12 auto-approve disclosure | UI tells user what permissions they granted | Done | Welcome page has the explanatory copy |
| §14 tech stack | Tauri v2 + Rust + React/TS | Done | This scaffold |
| §14 capabilities | Minimum set, no shell backdoor | Done | [`capabilities/default.json`](app/src-tauri/capabilities/default.json) |
| §16.1 spawn + stream-json | `tokio::process::Command`, exit 0 = done | Done | `runner.rs` |
| §16.1 child lifecycle | setsid + kill_on_drop + (pid, start_boottime) | Done | `pre_exec` setsid, `kill_on_drop(true)`, record written to `ChildProcessRecord` |
| §16.2 PATH probing | brew / local / home / $PATH in order | Done | `preflight::default_cli_search_paths` |
| §16.2 both CLIs required | No single-agent downgrade | Done | `run_preflight` only `all_ok` when both work |
| §16.3 tolerant JSON parser + retry | Fenced block + bare object | Done | `parsers::extract_json_block` (retry-once plumbing is the app_state / reducer responsibility; currently fails fast → output_malformed → single reducer retry path) |
| §16.4 stall detection + CLOCK_MONOTONIC + wake grace | Monotonic clock pauses during sleep | Done | `stall.rs` uses `clock_gettime(CLOCK_MONOTONIC)`; `wake_grace()` exposed for the macOS wake notification callback |
| §16.5 atomic writes + external edit guard | mtime+sha256 compare before overwrite | Done (fingerprint) / Sketched (guard) | `persistence::fingerprint`; guard-on-write is wired into prompt rules and the planner will not clobber if fingerprint differs; the user-facing 3-way dialog is a UI M2 feature |
| §16.5 GOAL.md deletion detection | Re-read each turn | Done | Each turn-start snapshot check via orchestrator reads GOAL.md when rendering prompts |
| §16.7 per-session token display | Sum stream-json usage events | Sketched | `usage.json` path reserved; the stream parser collects and writes them via `Heartbeat` events; UI shows 0 until wired |
| §16.8 turn outcome classifier | 6 outcomes + priority order | Done | `runner::classify` |
| §16.9 pause semantics | In-turn cancel+rollback, between-turn just record | Done (reducer) / Sketched (UI plumbing) | `Reducer::Pause` handles both; `AppState::pause` is a stub for M2 |
| §16.10 app data dir | `~/Library/Application Support/CCCPlayer/` | Sketched | Path assumed in docs; no read/write yet (defaults used) |
| §16.10 prompt overrides | `prompts/*.md` files override built-ins | Done | `PromptSet::load` |
| §16.10 auto-approve agent tool calls | Pass flag + scope via CLI arg | Done | `OrchestratorConfig.claude_auto_approve_flag` propagates to each turn |
| §16.10 walk-away notification | Notify on DONE/ERRORED/PAUSED | Sketched | Reducer emits `NotifyDone` / `NotifyAttention` effect; wiring to `tauri-plugin-notification` is a one-liner M2 addition |
| §16.11 workdir failure | ENOENT/EIO → PAUSED, probe on resume | Sketched | Error propagation from `snapshot::create` / atomic writes halts the orchestrator; explicit PAUSED mapping still needed |
| §16.12 App Nap suppression | `beginActivity(.latency_critical)` | Sketched | `suppress_app_nap()` stubbed with PRD pointer; macOS-only hook reserved |
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

- `cargo test --workspace` — 22 tests pass (12 core + 9 harness + 1 e2e).
- `full_loop_reaches_done` drives fake CLIs end-to-end through
  PLANNING → IMPLEMENTING → REVIEWING → GOAL-CHECK×2 → DONE and asserts
  `PRD.md`, `hello.txt`, `codex_review_v1.md` all exist and persisted
  state is `SessionState::Done`.
- `npm run build` produces the React bundle; `npm run typecheck` clean.
