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
use cccplayer_core::events::Event;
use cccplayer_core::session::Session;
use cccplayer_harness::{Orchestrator, OrchestratorConfig};
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

/// What Tauri's `manage()` keeps alive for us.
pub struct AppState {
    pub running: Arc<Mutex<Option<RunningSession>>>,
}

/// One in-flight session.
pub struct RunningSession {
    pub workdir: PathBuf,
    pub handle: JoinHandle<Result<()>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            running: Arc::new(Mutex::new(None)),
        }
    }

    /// Start a new session or resume the existing one for `workdir`. Returns
    /// an error if a different session is already running.
    pub async fn start(
        &self,
        app: AppHandle,
        workdir: PathBuf,
        goal: String,
        config: OrchestratorConfig,
    ) -> Result<()> {
        let mut guard = self.running.lock().await;
        if let Some(existing) = guard.as_ref() {
            if existing.workdir != workdir {
                anyhow::bail!(
                    "another session is already active on {}",
                    existing.workdir.display()
                );
            }
            if !existing.handle.is_finished() {
                return Ok(()); // idempotent — "Start" is safe to double-click
            }
        }

        // Write GOAL.md if missing.
        let goal_path = workdir.join("GOAL.md");
        if !goal_path.exists() {
            cccplayer_core::persistence::atomic_write(&goal_path, goal.as_bytes())
                .context("write GOAL.md")?;
        }

        let session = Session::open(&workdir).context("open session")?;
        let mut orch = Orchestrator::new(session, config)?;
        orch.ensure_initialized(goal.lines().next().unwrap_or(""))?;

        let (events_tx, mut events_rx) = mpsc::unbounded_channel::<Event>();
        let app_for_forward = app.clone();
        tokio::spawn(async move {
            while let Some(ev) = events_rx.recv().await {
                let _ = app_for_forward.emit("cccplayer://event", &ev);
            }
        });

        let handle = tokio::spawn(async move { orch.run(events_tx).await });
        *guard = Some(RunningSession { workdir, handle });
        Ok(())
    }

    pub async fn pause(&self) -> Result<()> {
        let guard = self.running.lock().await;
        if let Some(rs) = guard.as_ref() {
            // The orchestrator doesn't yet expose a pause channel in M1; the
            // reducer has a Pause command, but we'd need to plumb a cancel
            // sender through. Placeholder for M2.
            drop(rs);
        }
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        let mut guard = self.running.lock().await;
        if let Some(rs) = guard.take() {
            rs.handle.abort();
        }
        Ok(())
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
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
