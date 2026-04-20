//! App-level state owned by Tauri: holds the running Orchestrator (if any),
//! bridges its event stream to the frontend via `AppHandle::emit`.
//!
//! See PRD §16.13 single-writer state machine — we enforce it here by
//! owning the `Orchestrator` behind a Tokio `Mutex` and only accepting state
//! mutations via commands that cross this boundary.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use cccplayer_core::events::{Event, RawLogLine};
use cccplayer_core::session::Session;
use cccplayer_harness::orchestrator::CancelHandle;
use cccplayer_harness::{Orchestrator, OrchestratorConfig};
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use crate::commands::StartError;

/// What Tauri's `manage()` keeps alive for us.
pub struct AppState {
    pub running: Arc<Mutex<Option<RunningSession>>>,
}

/// One in-flight session.
pub struct RunningSession {
    pub workdir: PathBuf,
    pub handle: JoinHandle<Result<()>>,
    pub cancel: CancelHandle,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            running: Arc::new(Mutex::new(None)),
        }
    }

    /// Start a new session or resume the existing one for `workdir`. Returns
    /// `StartError::Other` if a different session is already running or any
    /// infra step fails; returns `StartError::GoalConflict` if the workdir
    /// has a `GOAL.md` that differs from the caller-supplied `goal` and
    /// `overwrite_goal` is `false`.
    pub async fn start(
        &self,
        app: AppHandle,
        workdir: PathBuf,
        goal: String,
        overwrite_goal: bool,
        config: OrchestratorConfig,
    ) -> Result<(), StartError> {
        let mut guard = self.running.lock().await;
        if let Some(existing) = guard.as_ref() {
            if existing.workdir != workdir {
                return Err(StartError::Other {
                    message: format!(
                        "another session is already active on {}",
                        existing.workdir.display()
                    ),
                });
            }
            if !existing.handle.is_finished() {
                return Ok(()); // idempotent — "Start" is safe to double-click
            }
        }

        // GOAL.md handling (v1.6 §16.5):
        // - File absent → write it.
        // - Present, content matches → leave alone.
        // - Present, content differs, `overwrite_goal=false` → GoalConflict
        //   so the UI can prompt.
        // - Present, content differs, `overwrite_goal=true` → atomic rewrite.
        // Trim comparisons to shrug off trailing newline differences that
        // round-trip through the UI.
        let goal_path = workdir.join("GOAL.md");
        if goal_path.exists() {
            let existing =
                std::fs::read_to_string(&goal_path).map_err(|e| StartError::Other {
                    message: format!("read existing GOAL.md: {e}"),
                })?;
            if existing.trim() != goal.trim() {
                if !overwrite_goal {
                    return Err(StartError::GoalConflict { existing });
                }
                cccplayer_core::persistence::atomic_write(&goal_path, goal.as_bytes())
                    .context("overwrite GOAL.md")
                    .map_err(StartError::from)?;
            }
        } else {
            cccplayer_core::persistence::atomic_write(&goal_path, goal.as_bytes())
                .context("write GOAL.md")
                .map_err(StartError::from)?;
        }

        let session = Session::open(&workdir)
            .context("open session")
            .map_err(StartError::from)?;
        let mut orch = Orchestrator::new(session, config).map_err(StartError::from)?;
        let cancel = orch.cancel_handle();
        orch.ensure_initialized(goal.lines().next().unwrap_or(""))
            .map_err(StartError::from)?;

        let (events_tx, mut events_rx) = mpsc::unbounded_channel::<Event>();
        let (raw_tx, mut raw_rx) = mpsc::unbounded_channel::<RawLogLine>();
        orch.set_raw_sink(raw_tx);

        let app_for_events = app.clone();
        tokio::spawn(async move {
            while let Some(ev) = events_rx.recv().await {
                let _ = app_for_events.emit("cccplayer://event", &ev);
            }
        });

        let app_for_raw = app.clone();
        tokio::spawn(async move {
            while let Some(line) = raw_rx.recv().await {
                let _ = app_for_raw.emit("cccplayer://raw", &line);
            }
        });

        // Spawn the orchestrator. When it returns (because the session
        // paused / errored / finished), check the persisted session meta
        // for a rate-limit retry_at; if one is recorded, schedule an
        // auto-resume task that fires at that time and re-invokes start.
        let workdir_for_wait = workdir.clone();
        let goal_for_wait = goal.clone();
        let app_for_wait = app.clone();
        let running_for_wait = self.running.clone();
        let handle = tokio::spawn(async move {
            let result = orch.run(events_tx).await;
            // Inspect the session meta on disk to see whether we paused
            // on a rate-limit. `Session::open` would re-acquire the flock
            // so we read raw JSON here instead.
            let meta_path = workdir_for_wait.join(".cccplayer/session.json");
            if let Ok(body) = std::fs::read_to_string(&meta_path) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                    let state = v.get("state").and_then(|x| x.as_str()).unwrap_or("");
                    let retry_at = v
                        .get("retry_at")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string());
                    if state == "PAUSED" {
                        if let Some(retry_at_str) = retry_at {
                            schedule_auto_resume(
                                app_for_wait,
                                running_for_wait,
                                workdir_for_wait,
                                goal_for_wait,
                                retry_at_str,
                            );
                        }
                    }
                }
            }
            result
        });
        *guard = Some(RunningSession {
            workdir,
            handle,
            cancel,
        });
        Ok(())
    }

    pub async fn pause(&self) -> Result<()> {
        let guard = self.running.lock().await;
        if let Some(rs) = guard.as_ref() {
            rs.cancel.pause();
        }
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        let mut guard = self.running.lock().await;
        if let Some(rs) = guard.take() {
            rs.cancel.stop();
            // Don't abort: let the orchestrator unwind cleanly so it can
            // write the final ABANDONED state to session.json.
        }
        Ok(())
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Schedule a tokio task that wakes at `retry_at_rfc3339` and re-invokes
/// `AppState::start` with the recorded (workdir, goal). If the user has
/// stopped or started a different session in the meantime we bail out.
/// Added v1.3 for rate-limit self-healing.
fn schedule_auto_resume(
    app: AppHandle,
    running: Arc<Mutex<Option<RunningSession>>>,
    workdir: PathBuf,
    goal: String,
    retry_at_rfc3339: String,
) {
    let Ok(retry_at) = chrono::DateTime::parse_from_rfc3339(&retry_at_rfc3339) else {
        tracing::warn!(
            "auto-resume: could not parse retry_at='{retry_at_rfc3339}', giving up"
        );
        return;
    };
    let retry_at_utc = retry_at.with_timezone(&chrono::Utc);
    let now = chrono::Utc::now();
    let wait = (retry_at_utc - now).to_std().unwrap_or(Duration::from_secs(60));
    tracing::info!(
        "auto-resume scheduled in {:?} (at {retry_at_rfc3339})",
        wait
    );
    tokio::spawn(async move {
        tokio::time::sleep(wait).await;
        // Confirm the guard still points at the same paused session we
        // scheduled for. If user already resumed / stopped / started
        // somewhere else, don't interfere.
        {
            let guard = running.lock().await;
            match guard.as_ref() {
                Some(rs)
                    if rs.workdir == workdir && rs.handle.is_finished() =>
                {
                    tracing::info!("auto-resume: firing Start on {:?}", workdir);
                }
                _ => {
                    tracing::info!(
                        "auto-resume: guard moved on, skipping scheduled fire"
                    );
                    return;
                }
            }
        }
        // Drop the guard then re-enter `start` which takes its own.
        let cfg = match default_config() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("auto-resume: default_config failed: {e:#}");
                return;
            }
        };
        let state = AppState { running };
        // overwrite_goal=true is safe here: rate-limit auto-resume on an
        // already-initialized session means GOAL.md is whatever the agent
        // has been running against; content should match, the write is a
        // no-op. If someone raced in and edited the file meanwhile, §16.11
        // fingerprint guard will catch it on the next step.
        if let Err(e) = state.start(app, workdir, goal, true, cfg).await {
            tracing::warn!("auto-resume: start failed: {e:?}");
        }
    });
}

/// Default orchestrator config, populated from preflight.
pub fn default_config() -> Result<OrchestratorConfig> {
    let report = cccplayer_core::preflight::run_preflight(None, None);
    let claude = report
        .claude
        .ok_or_else(|| anyhow::anyhow!("claude CLI not found"))?;
    let codex = report
        .codex
        .ok_or_else(|| anyhow::anyhow!("codex CLI not found"))?;
    Ok(OrchestratorConfig {
        claude_path: claude.path,
        codex_path: codex.path,
        claude_auto_approve_flag: claude.auto_approve_flag,
        codex_auto_approve_flag: codex.auto_approve_flag,
        stall_threshold: Duration::from_secs(600),
        fake_mode: false,
    })
}
