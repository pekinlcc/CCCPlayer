//! Preflight checks. See PRD §16.2 (PATH probe), §16.14 (auto-approve flag),
//! §2 (CLI presence & login).

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Candidate absolute directories to search for a CLI, in priority order.
/// See PRD §16.2.
pub fn default_cli_search_paths() -> Vec<PathBuf> {
    let mut v = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        v.push(PathBuf::from(&home).join(".local/bin"));
        v.push(PathBuf::from(&home).join("bin"));
    }
    // Also honor current process PATH.
    if let Some(p) = std::env::var_os("PATH") {
        for p in std::env::split_paths(&p) {
            v.push(p);
        }
    }
    v
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliInfo {
    pub path: PathBuf,
    pub version_line: String,
    pub supports_auto_approve: bool,
    /// Exact flag name detected for auto-approve, e.g. `--dangerously-skip-permissions`.
    pub auto_approve_flag: Option<String>,
}

/// Find an executable named `name` using [`default_cli_search_paths`] plus an
/// optional override (from settings).
pub fn find_cli(name: &str, override_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = override_path {
        if p.is_file() {
            return Some(p.to_path_buf());
        }
    }
    for dir in default_cli_search_paths() {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Run `<cli> --version` with a short timeout to collect the first line.
pub fn probe_version(cli: &Path) -> Result<String> {
    let output = std::process::Command::new(cli).arg("--version").output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let first = stdout.lines().next().unwrap_or("").to_string();
    Ok(first)
}

/// Probe whether the CLI accepts the auto-approve flag. We try the candidate
/// flags in order; the first that results in a zero-exit `--help` acceptance
/// wins. See PRD §16.15.
pub fn probe_auto_approve(cli: &Path, candidates: &[&str]) -> Option<String> {
    for flag in candidates {
        let output = std::process::Command::new(cli)
            .arg(flag)
            .arg("--help")
            .output();
        if let Ok(o) = output {
            if o.status.success() {
                return Some(flag.to_string());
            }
            // Some CLIs exit non-zero on --help + flag combination but don't
            // emit "unknown flag" style errors; we conservatively only accept
            // success.
        }
    }
    None
}

/// Candidate auto-approve flags for Claude Code, in priority order.
pub const CLAUDE_AUTO_APPROVE_CANDIDATES: &[&str] =
    &["--dangerously-skip-permissions", "--yes", "--auto-approve"];

/// Candidate auto-approve flags for Codex, in priority order.
pub const CODEX_AUTO_APPROVE_CANDIDATES: &[&str] = &["--auto-approve", "--yes"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightReport {
    pub claude: Option<CliInfo>,
    pub codex: Option<CliInfo>,
    pub all_ok: bool,
}

pub fn run_preflight(
    claude_override: Option<&Path>,
    codex_override: Option<&Path>,
) -> PreflightReport {
    let claude = find_cli("claude", claude_override).and_then(|p| probe_cli(&p, "claude"));
    let codex = find_cli("codex", codex_override).and_then(|p| probe_cli(&p, "codex"));
    let all_ok = claude
        .as_ref()
        .map(|c| c.supports_auto_approve)
        .unwrap_or(false)
        && codex
            .as_ref()
            .map(|c| c.supports_auto_approve)
            .unwrap_or(false);
    PreflightReport {
        claude,
        codex,
        all_ok,
    }
}

fn probe_cli(path: &Path, kind: &str) -> Option<CliInfo> {
    let version = probe_version(path).ok()?;
    let flag = match kind {
        "claude" => probe_auto_approve(path, CLAUDE_AUTO_APPROVE_CANDIDATES),
        "codex" => probe_auto_approve(path, CODEX_AUTO_APPROVE_CANDIDATES),
        _ => None,
    };
    Some(CliInfo {
        path: path.to_path_buf(),
        version_line: version,
        supports_auto_approve: flag.is_some(),
        auto_approve_flag: flag,
    })
}
