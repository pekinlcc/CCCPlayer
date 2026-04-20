//! Tauri IPC commands. See PRD §14 modules.
//!
//! Each command is a thin wrapper that delegates to `cccplayer-core` or
//! `cccplayer-harness` and returns a JSON-serializable payload.

use std::path::PathBuf;

use cccplayer_core::preflight::{run_preflight, PreflightReport};
use cccplayer_core::workdir::{classify, maybe_append_gitignore, SafetyVerdict};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::app_state::{default_config, AppState};

#[derive(Debug, Serialize)]
pub struct PreflightResp {
    #[serde(flatten)]
    pub report: PreflightReport,
}

#[tauri::command]
pub fn preflight(
    claude_path_override: Option<String>,
    codex_path_override: Option<String>,
) -> PreflightResp {
    let claude = claude_path_override.as_ref().map(PathBuf::from);
    let codex = codex_path_override.as_ref().map(PathBuf::from);
    PreflightResp {
        report: run_preflight(claude.as_deref(), codex.as_deref()),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ClassifyResp {
    pub verdict: SafetyVerdict,
    pub gitignore_appended: bool,
}

#[tauri::command]
pub fn classify_workdir(path: String) -> Result<ClassifyResp, String> {
    let p = PathBuf::from(&path);
    let verdict = classify(&p).map_err(|e| e.to_string())?;
    let appended = match verdict {
        SafetyVerdict::Ok | SafetyVerdict::SoftWarn { .. } => {
            maybe_append_gitignore(&p).map_err(|e| e.to_string())?
        }
        _ => false,
    };
    Ok(ClassifyResp {
        verdict,
        gitignore_appended: appended,
    })
}

/// v1.6.1 UX: on workdir selection the UI pre-loads the existing `GOAL.md`
/// so the user can see the current goal and either keep it verbatim or edit
/// on top of it. Returns `None` when the file is absent; on read failure we
/// surface the error so the UI can show a small warning instead of silently
/// blanking the goal field.
#[tauri::command]
pub fn read_goal_md(path: String) -> Result<Option<String>, String> {
    let goal_path = PathBuf::from(&path).join("GOAL.md");
    if !goal_path.exists() {
        return Ok(None);
    }
    std::fs::read_to_string(&goal_path)
        .map(Some)
        .map_err(|e| format!("{e}"))
}

#[derive(Debug, Deserialize)]
pub struct StartReq {
    pub goal: String,
    pub workdir: String,
    /// When true, the provided `goal` replaces any existing `GOAL.md` in the
    /// workdir. When false (default), a mismatch between existing GOAL.md and
    /// `goal` is returned as `StartError::GoalConflict` so the UI can prompt.
    #[serde(default)]
    pub overwrite_goal: bool,
}

/// Structured start errors. Serialized to the frontend as
/// `{ "kind": "goal_conflict", "existing": "..." }` or
/// `{ "kind": "other", "message": "..." }` — the UI pattern-matches on `kind`.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartError {
    /// `GOAL.md` already exists in the workdir and its content differs from
    /// what the user typed. The UI shows a three-way modal (use existing /
    /// overwrite / cancel) before re-invoking `start_session`.
    GoalConflict { existing: String },
    /// Any other start failure (missing CLI, bad workdir, lock contention).
    Other { message: String },
}

impl From<anyhow::Error> for StartError {
    fn from(e: anyhow::Error) -> Self {
        StartError::Other {
            message: format!("{e:#}"),
        }
    }
}

#[tauri::command]
pub async fn start_session(
    app: AppHandle,
    state: State<'_, AppState>,
    req: StartReq,
) -> Result<(), StartError> {
    let cfg = default_config()?;
    state
        .start(
            app,
            PathBuf::from(req.workdir),
            req.goal,
            req.overwrite_goal,
            cfg,
        )
        .await
}

#[tauri::command]
pub async fn pause_session(state: State<'_, AppState>) -> Result<(), String> {
    state.pause().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_session(state: State<'_, AppState>) -> Result<(), String> {
    state.stop().await.map_err(|e| e.to_string())
}
