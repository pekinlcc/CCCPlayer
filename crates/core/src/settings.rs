//! Global app settings. Persisted to
//! `~/Library/Application Support/CCCPlayer/settings.json`.
//!
//! See PRD §16.10.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::persistence::atomic_write;

pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub schema_version: u32,
    /// Override for the `claude` CLI absolute path. None → use PATH probing.
    pub claude_cli_path: Option<PathBuf>,
    /// Override for the `codex` CLI absolute path.
    pub codex_cli_path: Option<PathBuf>,
    /// Stall threshold in seconds. See PRD §16.4.
    pub stall_threshold_secs: u64,
    /// Optional hard cap on the number of Rounds per Session. `None` disables.
    pub round_hard_cap: Option<u32>,
    /// Preferred Claude model. Passed as a CLI flag when set.
    pub claude_model: Option<String>,
    /// Preferred Codex reasoning level. Passed as a CLI flag when set.
    pub codex_reasoning: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            claude_cli_path: None,
            codex_cli_path: None,
            stall_threshold_secs: 600,
            round_hard_cap: None,
            claude_model: Some("claude-opus-4-7".to_string()),
            codex_reasoning: Some("high".to_string()),
        }
    }
}

/// Returns the base application-data directory.
///
/// On macOS this is `~/Library/Application Support/CCCPlayer/`.
/// On other platforms we fall back to `$XDG_CONFIG_HOME/cccplayer/` or
/// `~/.config/cccplayer/` so tests and Linux dev work.
pub fn app_data_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    #[cfg(target_os = "macos")]
    {
        Some(
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("CCCPlayer"),
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(xdg).join("cccplayer"));
        }
        Some(PathBuf::from(home).join(".config").join("cccplayer"))
    }
}

pub fn settings_path(base: &Path) -> PathBuf {
    base.join("settings.json")
}

pub fn prompts_dir(base: &Path) -> PathBuf {
    base.join("prompts")
}

/// Load settings from `~/Library/Application Support/CCCPlayer/settings.json`,
/// or return defaults. Refuses to load if `schema_version` exceeds the
/// binary's max — matches the per-session forward-compat rule (§8).
pub fn load(base: &Path) -> Result<Settings> {
    let path = settings_path(base);
    if !path.exists() {
        return Ok(Settings::default());
    }
    let text = std::fs::read_to_string(&path)?;
    let v: serde_json::Value = serde_json::from_str(&text)?;
    let ver = v
        .get("schema_version")
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    if ver > SETTINGS_SCHEMA_VERSION as u64 {
        anyhow::bail!(
            "settings.json was written by a newer CCCPlayer (schema {ver} > {SETTINGS_SCHEMA_VERSION}); \
             please upgrade"
        );
    }
    let parsed: Settings = serde_json::from_value(v).unwrap_or_default();
    Ok(parsed)
}

pub fn save(base: &Path, s: &Settings) -> Result<()> {
    std::fs::create_dir_all(base).ok();
    let bytes = serde_json::to_vec_pretty(s)?;
    atomic_write(&settings_path(base), &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let s = Settings::default();
        save(base, &s).unwrap();
        let loaded = load(base).unwrap();
        assert_eq!(loaded.schema_version, SETTINGS_SCHEMA_VERSION);
        assert_eq!(loaded.stall_threshold_secs, 600);
    }

    #[test]
    fn forward_compat_refuses_newer_schema() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base).unwrap();
        std::fs::write(
            base.join("settings.json"),
            r#"{"schema_version": 9999, "stall_threshold_secs": 1}"#,
        )
        .unwrap();
        let err = load(base).unwrap_err();
        assert!(err.to_string().contains("newer"));
    }

    #[test]
    fn load_missing_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let s = load(dir.path()).unwrap();
        assert_eq!(s.schema_version, SETTINGS_SCHEMA_VERSION);
    }
}
