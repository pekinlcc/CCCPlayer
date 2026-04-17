# CCCPlayer

Claude Code × Codex dual-agent loop, wrapped in a macOS client.

See [`PRD.md`](PRD.md) for the full design and [`AUDIT.md`](AUDIT.md) for the
current M1 conformance status.

## Layout

```
crates/
├── core/         # cccplayer-core    — state machine, persistence, snapshot
├── harness/      # cccplayer-harness — CLI runner, stall, orchestrator
└── fake-cli/     # cccplayer-fake-claude, cccplayer-fake-codex (tests)
app/
└── src-tauri/    # desktop binary & Tauri commands
ui/               # React + Vite frontend
prompts/          # default agent prompt templates (embedded at build)
```

## Build

```sh
# Full workspace — Rust only, no Tauri native deps required.
cargo check --workspace
cargo test  --workspace

# Frontend
cd ui && npm install && npm run build
```

For the full desktop binary on macOS:

```sh
cd ui && npm install
cd .. && cargo tauri dev   # or: cargo tauri build --features tauri
```

## Architecture highlights

- **Single-writer state machine** — every transition flows through
  `core::reducer::Reducer`; the app holds it behind one Tokio mutex, all
  events funnel via `StateCommand`. See [`reducer.rs`](crates/core/src/reducer.rs).
- **Harness owns the CLI** — `harness::HarnessRunner` spawns `claude` /
  `codex` in a fresh process group (`setsid` + `kill_on_drop(true)`),
  streams stdout/stderr with in-line redaction, and exposes a stall
  watcher backed by `CLOCK_MONOTONIC`.
- **`.cccplayer/` per workspace** — private state (session.json, events.log,
  transcripts, `round-00…round-N` snapshots). `round-00` is the user's
  original code and is never deleted.
- **Fake CLIs drive integration tests** — `cccplayer-fake-claude` and
  `cccplayer-fake-codex` are standalone binaries that implement the same
  stream-json / file-output contract; `cargo test` exercises the whole
  PLANNING → DONE loop without burning tokens.
- **Workspace safety** — paths are classified before any agent writes land:
  system dirs (`/`, `/System`, …) are refused; home-class dirs require
  confirmation; soft warnings surface for sensitive files.

## Status

M1 scope is landed and verified under the fake-CLI integration test.
macOS-shell-specific integrations (App Nap suppression via `beginActivity`,
`tauri-plugin-notification`, the TCC deep-link) are stubbed with PRD
pointers and slated for M2 — see [`AUDIT.md`](AUDIT.md) for the row-by-row
status.
