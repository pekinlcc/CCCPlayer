//! End-to-end test: drive a full PLANNING → IMPLEMENTING → REVIEWING →
//! (GOAL-CHECK × 2) → DONE loop using the fake CLIs.
//!
//! This is the PRD §19 #3 "fake CLI test suite" in its minimum form.

use std::path::PathBuf;
use std::time::Duration;

use cccplayer_core::session::Session;
use cccplayer_core::state::SessionState;
use cccplayer_harness::{Orchestrator, OrchestratorConfig};
use tokio::sync::mpsc;

#[tokio::test]
async fn full_loop_reaches_done() {
    let workdir = tempfile::tempdir().expect("tempdir");
    // Write GOAL.md first — required by every prompt.
    std::fs::write(
        workdir.path().join("GOAL.md"),
        "Produce a hello.txt file.\n",
    )
    .unwrap();

    // Locate the fake binaries — they live in target/debug relative to the
    // workspace root.
    let manifest = env!("CARGO_MANIFEST_DIR");
    let workspace_root = PathBuf::from(manifest).join("..").join("..");
    let fake_claude = workspace_root
        .join("target")
        .join("debug")
        .join("cccplayer-fake-claude");
    let fake_codex = workspace_root
        .join("target")
        .join("debug")
        .join("cccplayer-fake-codex");
    assert!(
        fake_claude.exists(),
        "fake-claude binary not built at {}; run `cargo build` first",
        fake_claude.display()
    );
    assert!(fake_codex.exists(), "fake-codex binary not built");

    let session = Session::open(workdir.path()).expect("open session");
    let cfg = OrchestratorConfig {
        claude_path: fake_claude,
        codex_path: fake_codex,
        claude_auto_approve_flag: None, // fakes don't need it
        codex_auto_approve_flag: None,
        stall_threshold: Duration::from_secs(30),
        fake_mode: true,
    };
    let mut orch = Orchestrator::new(session, cfg).expect("new");
    orch.ensure_initialized("Produce a hello.txt file.").unwrap();

    let (tx, mut rx) = mpsc::unbounded_channel();
    // Capture events in a background task so we can observe the loop.
    let captured = tokio::spawn(async move {
        let mut acc = Vec::new();
        while let Some(ev) = rx.recv().await {
            acc.push(ev);
        }
        acc
    });

    orch.run(tx).await.expect("run");
    drop(orch); // drops the unbounded sender held inside? no — but the tx was moved in
               // anyway. The background task will exit when all senders drop.

    // Wait a brief moment to collect events.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let events = captured.await.expect("captured");
    assert!(!events.is_empty(), "should have emitted events");

    // Final assertions:
    assert!(
        workdir.path().join("PRD.md").exists(),
        "PRD.md should have been written by PLANNING"
    );
    assert!(
        workdir.path().join("hello.txt").exists(),
        "hello.txt should have been written by IMPLEMENTING"
    );
    assert!(
        workdir.path().join("codex_review_v1.md").exists(),
        "codex_review_v1.md should have been written by REVIEWING"
    );

    // Re-open the session and confirm state is DONE.
    let session = Session::open(workdir.path()).expect("reopen");
    let meta = cccplayer_core::persistence::load_session_meta(&session)
        .unwrap()
        .unwrap();
    assert!(
        matches!(meta.state, SessionState::Done),
        "expected DONE, got {:?}",
        meta.state
    );
}

/// Second integration test: force the first review to request changes, so
/// the loop exercises REFINING → second REVIEW → GOAL-CHECK → DONE. Uses
/// CCCPLAYER_FAKE_FORCE_CHANGES env var recognized by the fake CLIs.
#[tokio::test]
async fn refining_loop_reaches_done() {
    let workdir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        workdir.path().join("GOAL.md"),
        "Produce a hello.txt file (via refining).\n",
    )
    .unwrap();
    // File-based flag: robust against parallel-test env pollution.
    std::fs::write(workdir.path().join(".fake-force-changes"), "").unwrap();

    let manifest = env!("CARGO_MANIFEST_DIR");
    let workspace_root = PathBuf::from(manifest).join("..").join("..");
    let fake_claude = workspace_root
        .join("target")
        .join("debug")
        .join("cccplayer-fake-claude");
    let fake_codex = workspace_root
        .join("target")
        .join("debug")
        .join("cccplayer-fake-codex");

    let session = Session::open(workdir.path()).expect("open");
    let cfg = OrchestratorConfig {
        claude_path: fake_claude,
        codex_path: fake_codex,
        claude_auto_approve_flag: None,
        codex_auto_approve_flag: None,
        stall_threshold: Duration::from_secs(30),
        fake_mode: true,
    };
    let mut orch = Orchestrator::new(session, cfg).expect("new");
    orch.ensure_initialized("Produce a hello.txt (via refining).")
        .unwrap();

    let (tx, _rx) = mpsc::unbounded_channel::<cccplayer_core::events::Event>();
    orch.run(tx).await.expect("run");
    drop(orch); // release the flock before reopening

    // After refining, both hello.txt (refined) and v1/v2 reviews should
    // exist; session reaches DONE.
    assert!(workdir.path().join("hello.txt").exists());
    assert!(workdir.path().join("codex_review_v1.md").exists());
    assert!(workdir.path().join("codex_review_v2.md").exists());

    let session = Session::open(workdir.path()).expect("reopen");
    let meta = cccplayer_core::persistence::load_session_meta(&session)
        .unwrap()
        .unwrap();
    assert!(
        matches!(meta.state, SessionState::Done),
        "expected DONE after refining, got {:?}",
        meta.state
    );
}

/// GOAL.md being deleted mid-session must pause, not crash.
#[tokio::test]
async fn missing_goal_causes_pause() {
    let workdir = tempfile::tempdir().expect("tempdir");
    std::fs::write(workdir.path().join("GOAL.md"), "test goal\n").unwrap();

    let manifest = env!("CARGO_MANIFEST_DIR");
    let workspace_root = PathBuf::from(manifest).join("..").join("..");
    let fake_claude = workspace_root
        .join("target")
        .join("debug")
        .join("cccplayer-fake-claude");
    let fake_codex = workspace_root
        .join("target")
        .join("debug")
        .join("cccplayer-fake-codex");

    let session = Session::open(workdir.path()).expect("open");
    let cfg = OrchestratorConfig {
        claude_path: fake_claude,
        codex_path: fake_codex,
        claude_auto_approve_flag: None,
        codex_auto_approve_flag: None,
        stall_threshold: Duration::from_secs(30),
        fake_mode: true,
    };
    let mut orch = Orchestrator::new(session, cfg).expect("new");
    orch.ensure_initialized("test goal").unwrap();

    // Delete GOAL.md before running so the first step() will detect it.
    std::fs::remove_file(workdir.path().join("GOAL.md")).unwrap();

    let (tx, _rx) = mpsc::unbounded_channel::<cccplayer_core::events::Event>();
    orch.run(tx).await.expect("run should not error, just pause");
    drop(orch);

    let session = Session::open(workdir.path()).expect("reopen");
    let meta = cccplayer_core::persistence::load_session_meta(&session)
        .unwrap()
        .unwrap();
    assert!(
        matches!(meta.state, SessionState::Paused),
        "expected PAUSED after GOAL.md deletion, got {:?}",
        meta.state
    );
}

/// External stop() should transition the session to ABANDONED while leaving
/// artifacts intact.
#[tokio::test]
async fn external_stop_transitions_to_abandoned() {
    let workdir = tempfile::tempdir().expect("tempdir");
    std::fs::write(workdir.path().join("GOAL.md"), "test goal\n").unwrap();

    let manifest = env!("CARGO_MANIFEST_DIR");
    let workspace_root = PathBuf::from(manifest).join("..").join("..");
    let fake_claude = workspace_root
        .join("target")
        .join("debug")
        .join("cccplayer-fake-claude");
    let fake_codex = workspace_root
        .join("target")
        .join("debug")
        .join("cccplayer-fake-codex");

    let session = Session::open(workdir.path()).expect("open");
    let cfg = OrchestratorConfig {
        claude_path: fake_claude,
        codex_path: fake_codex,
        claude_auto_approve_flag: None,
        codex_auto_approve_flag: None,
        stall_threshold: Duration::from_secs(30),
        fake_mode: true,
    };
    let mut orch = Orchestrator::new(session, cfg).expect("new");
    let cancel = orch.cancel_handle();
    orch.ensure_initialized("test").unwrap();

    // Fire stop() *before* the loop starts so the first step() picks it up.
    cancel.stop();

    let (tx, _rx) = mpsc::unbounded_channel::<cccplayer_core::events::Event>();
    orch.run(tx).await.expect("run");
    drop(orch);

    let session = Session::open(workdir.path()).expect("reopen");
    let meta = cccplayer_core::persistence::load_session_meta(&session)
        .unwrap()
        .unwrap();
    assert!(
        matches!(meta.state, SessionState::Abandoned),
        "expected ABANDONED after stop, got {:?}",
        meta.state
    );
}

/// Usage events from the stream-json output accumulate into usage.json.
#[tokio::test]
async fn usage_is_tracked_per_session() {
    let workdir = tempfile::tempdir().expect("tempdir");
    std::fs::write(workdir.path().join("GOAL.md"), "test\n").unwrap();

    let manifest = env!("CARGO_MANIFEST_DIR");
    let workspace_root = PathBuf::from(manifest).join("..").join("..");
    let fake_claude = workspace_root
        .join("target")
        .join("debug")
        .join("cccplayer-fake-claude");
    let fake_codex = workspace_root
        .join("target")
        .join("debug")
        .join("cccplayer-fake-codex");

    let session = Session::open(workdir.path()).expect("open");
    let cfg = OrchestratorConfig {
        claude_path: fake_claude,
        codex_path: fake_codex,
        claude_auto_approve_flag: None,
        codex_auto_approve_flag: None,
        stall_threshold: Duration::from_secs(30),
        fake_mode: true,
    };
    let mut orch = Orchestrator::new(session, cfg).expect("new");
    orch.ensure_initialized("test").unwrap();

    let (tx, _rx) = mpsc::unbounded_channel::<cccplayer_core::events::Event>();
    orch.run(tx).await.expect("run");
    drop(orch);

    let session = Session::open(workdir.path()).expect("reopen");
    let usage = cccplayer_core::persistence::load_usage(&session).unwrap();
    assert!(
        usage.claude_total() > 0,
        "claude usage should be > 0 after a session"
    );
    assert!(
        usage.codex_total() > 0,
        "codex usage should be > 0 after a session"
    );
}
