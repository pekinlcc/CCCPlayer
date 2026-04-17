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

#[derive(Debug, Deserialize)]
pub struct StartReq {
    pub goal: String,
    pub workdir: String,
}

#[tauri::command]
pub async fn start_session(
    app: AppHandle,
    state: State<'_, AppState>,
    req: StartReq,
) -> Result<(), String> {
    let cfg = default_config().map_err(|e| e.to_string())?;
    state
        .start(app, PathBuf::from(req.workdir), req.goal, cfg)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pause_session(state: State<'_, AppState>) -> Result<(), String> {
    state.pause().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_session(state: State<'_, AppState>) -> Result<(), String> {
    state.stop().await.map_err(|e| e.to_string())
}
