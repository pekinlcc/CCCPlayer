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

/// Probe whether the CLI accepts the auto-approve flag. Strategy: run
/// `<cli> --help` once, then scan its combined stdout+stderr for each
/// candidate flag as a whole word. This is more robust than running
/// `<cli> <flag> --help` and checking the exit code, which gives both
/// false positives (many CLIs make `--help` override any flag and always
/// exit 0) and false negatives (some CLIs reject even valid flag
/// combinations with non-zero exits). See PRD §16.15.
pub fn probe_auto_approve(cli: &Path, candidates: &[&str]) -> Option<String> {
    let output = std::process::Command::new(cli).arg("--help").output().ok()?;
    let mut help = String::new();
    help.push_str(&String::from_utf8_lossy(&output.stdout));
    help.push_str(&String::from_utf8_lossy(&output.stderr));
    for flag in candidates {
        // Whole-word match so "--yes" doesn't falsely match "--yessir".
        let pat = format!(r"(^|\s){}(\s|$|=|,)", regex::escape(flag));
        if regex::Regex::new(&pat).ok()?.is_match(&help) {
            return Some(flag.to_string());
        }
    }
    None
}

/// Candidate auto-approve flags for Claude Code, in priority order. Includes
/// the historical name and the current `--allow-...` variant shown in Claude
/// Code 2.x `--help`.
pub const CLAUDE_AUTO_APPROVE_CANDIDATES: &[&str] = &[
    "--dangerously-skip-permissions",
    "--allow-dangerously-skip-permissions",
    "--yes",
    "--auto-approve",
];

/// Candidate auto-approve flags for Codex, in priority order. Codex 0.1x
/// uses `--dangerously-bypass-approvals-and-sandbox`.
pub const CODEX_AUTO_APPROVE_CANDIDATES: &[&str] = &[
    "--dangerously-bypass-approvals-and-sandbox",
    "--yes",
    "--auto-approve",
];

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tiny shell script that prints a fake help text and exits 0,
    /// then point `probe_auto_approve` at it.
    fn fake_cli(help_body: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cli");
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = \"--help\" ]; then\n  cat <<'EOF'\n{help_body}\nEOF\n  exit 0\nfi\nexit 0\n"
        );
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&path).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&path, perm).unwrap();
        }
        dir
    }

    #[test]
    fn probe_detects_codex_bypass_flag() {
        let dir = fake_cli(
            "Usage: codex [OPTIONS]\n  --dangerously-bypass-approvals-and-sandbox  Skip approvals.\n",
        );
        let cli = dir.path().join("cli");
        let got = probe_auto_approve(&cli, CODEX_AUTO_APPROVE_CANDIDATES);
        assert_eq!(got.as_deref(), Some("--dangerously-bypass-approvals-and-sandbox"));
    }

    #[test]
    fn probe_detects_claude_skip_permissions() {
        let dir = fake_cli(
            "Usage: claude [OPTIONS]\n  --dangerously-skip-permissions  Bypass.\n  --print  One-shot.\n",
        );
        let cli = dir.path().join("cli");
        let got = probe_auto_approve(&cli, CLAUDE_AUTO_APPROVE_CANDIDATES);
        assert_eq!(got.as_deref(), Some("--dangerously-skip-permissions"));
    }

    #[test]
    fn probe_rejects_whole_word_mismatch() {
        // "--yessir" must not match "--yes".
        let dir = fake_cli(
            "Usage: cli [OPTIONS]\n  --yessir   Do yessir things.\n  --nopeoperation  No-op.\n",
        );
        let cli = dir.path().join("cli");
        let got = probe_auto_approve(&cli, &["--yes"]);
        assert_eq!(got, None);
    }

    #[test]
    fn probe_returns_none_when_no_candidate_present() {
        let dir = fake_cli("Usage: cli [OPTIONS]\n  --print-banner   Print banner.\n");
        let cli = dir.path().join("cli");
        let got = probe_auto_approve(&cli, &["--dangerously-skip-permissions", "--yes"]);
        assert_eq!(got, None);
    }
}
