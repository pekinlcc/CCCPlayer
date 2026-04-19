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
/// optional override (from settings). v1.5.0+: if the hardcoded search and
/// process `PATH` both miss, fall back to asking the user's login +
/// interactive shell via `$SHELL -l -i -c 'command -v <name>'`. This
/// catches installs in nvm / fnm / asdf / volta / any shell-rc `export
/// PATH=…` customization — none of which a Finder-launched `.app`
/// naturally sees.
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
    login_shell_which(name)
}

/// Ask the user's login + interactive shell where a binary named `name`
/// lives. Runs `$SHELL -l -i -c 'command -v <name>'` with a 5-second
/// timeout, parses the output, and returns the resolved absolute path
/// (or `None` on timeout, non-zero exit, alias output, or a non-file
/// path). Added v1.5.0 to fix CLI-not-on-PATH false negatives for
/// Finder-launched bundles when the user has installed claude / codex
/// via a tool manager (nvm, fnm, asdf, volta).
///
/// Accepts only `a-zA-Z0-9_.-` in `name` to avoid shell injection; anything
/// else returns `None` without executing.
pub fn login_shell_which(name: &str) -> Option<PathBuf> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    // Defense in depth: names must be safe identifiers. Our callers only
    // pass "claude" / "codex", but this keeps the function reusable.
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return None;
    }

    let shell_env =
        std::env::var_os("SHELL").unwrap_or_else(|| "/bin/zsh".into());
    let shell_path = PathBuf::from(&shell_env);
    if !shell_path.is_file() {
        return None;
    }

    // `-l` sources login-only files (.zprofile, .bash_profile, .profile).
    // `-i` sources interactive files (.zshrc, .bashrc) — nvm / fnm install
    // hooks typically land there. Combining both is how `iTerm` and other
    // apps get the user's "real" PATH. stdin null + stderr null so a
    // chatty rc file doesn't pollute our parse or hang on a prompt.
    let script = format!("command -v {name} 2>/dev/null");
    let mut child = Command::new(&shell_path)
        .args(["-l", "-i", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    // Poll-and-kill timeout loop. 5s is generous for a cold-start
    // shell-rc on a slow disk; most invocations return in <500ms.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    tracing::warn!(
                        "login_shell_which({name}) timed out after 5s; \
                         shell `{shell_env:?}` rc may have a slow init"
                    );
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_command_v_output(&stdout)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

/// Parse the first line of `command -v <name>` output. Returns the path
/// on success, `None` for:
/// - empty / whitespace-only output,
/// - alias declarations (`command -v` prints `alias foo='…'` for shell
///   aliases; those aren't spawnable),
/// - shell function definitions,
/// - relative paths (can't be trusted out of shell context).
pub fn parse_command_v_output(stdout: &str) -> Option<String> {
    let first = stdout.lines().next()?.trim();
    if first.is_empty() {
        return None;
    }
    // `alias foo='bar'` or `foo: aliased to bar` — not a binary.
    let lower = first.to_ascii_lowercase();
    if lower.starts_with("alias ")
        || lower.contains(" aliased to ")
        || first.contains('=')
    {
        return None;
    }
    // `foo is a shell builtin` / `foo is a function` — not a binary.
    if lower.contains(" is a ") || lower.contains(" is an ") {
        return None;
    }
    // Absolute paths only. A relative path in `command -v` output means the
    // shell found the binary relative to cwd, which won't work in our
    // context.
    if !first.starts_with('/') {
        return None;
    }
    Some(first.to_string())
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

    // ─── login_shell_which (v1.5.0) ────────────────────────────────────

    #[test]
    fn parse_command_v_accepts_absolute_path() {
        let got = parse_command_v_output("/usr/local/bin/claude\n");
        assert_eq!(got.as_deref(), Some("/usr/local/bin/claude"));
    }

    #[test]
    fn parse_command_v_rejects_empty() {
        assert_eq!(parse_command_v_output(""), None);
        assert_eq!(parse_command_v_output("\n"), None);
        assert_eq!(parse_command_v_output("   \n"), None);
    }

    #[test]
    fn parse_command_v_rejects_alias_forms() {
        // bash / zsh print "alias foo='…'".
        assert_eq!(
            parse_command_v_output("alias claude='/opt/anthropic/claude'"),
            None
        );
        // zsh's `type` form (seen with some configurations).
        assert_eq!(
            parse_command_v_output("claude: aliased to /opt/foo/claude"),
            None
        );
        // An inlined equals sign is a signal too.
        assert_eq!(
            parse_command_v_output("CLAUDE_PATH=/opt/x"),
            None
        );
    }

    #[test]
    fn parse_command_v_rejects_builtins_and_functions() {
        assert_eq!(
            parse_command_v_output("claude is a shell builtin"),
            None
        );
        assert_eq!(
            parse_command_v_output("claude is a function"),
            None
        );
        assert_eq!(
            parse_command_v_output("claude is an alias"),
            None
        );
    }

    #[test]
    fn parse_command_v_rejects_relative_paths() {
        // Some `command -v` implementations may return a relative path if
        // the binary was found via a non-absolute PATH entry. Reject.
        assert_eq!(parse_command_v_output("./bin/claude"), None);
        assert_eq!(parse_command_v_output("bin/claude"), None);
        assert_eq!(parse_command_v_output("claude"), None);
    }

    #[test]
    fn parse_command_v_takes_first_line_only() {
        // Noisy shell RC might emit extra lines; we only trust the first.
        let got = parse_command_v_output("/usr/local/bin/claude\nwarning: weird\n");
        assert_eq!(got.as_deref(), Some("/usr/local/bin/claude"));
    }

    #[test]
    fn login_shell_which_rejects_unsafe_names() {
        // Defense against shell injection: names must be a safe subset.
        assert!(login_shell_which("").is_none());
        assert!(login_shell_which("; rm -rf /").is_none());
        assert!(login_shell_which("claude && echo x").is_none());
        assert!(login_shell_which("claude$(whoami)").is_none());
        assert!(login_shell_which("claude`id`").is_none());
    }

    #[test]
    fn login_shell_which_finds_ls() {
        // Integration smoke test: `/bin/ls` exists on every macOS. If our
        // login-shell probe works at all, it must find `ls`. If this test
        // fails, either login_shell_which is broken or the test host's
        // shell RC is blocking -l -i invocations entirely.
        let got = login_shell_which("ls");
        assert!(
            got.is_some(),
            "login_shell_which('ls') returned None — check $SHELL rc files for \
             blocking behavior, or run `$SHELL -l -i -c 'command -v ls'` by \
             hand to see what's happening"
        );
        let path = got.unwrap();
        assert!(path.is_file(), "resolved path should be a real file: {path:?}");
        assert!(
            path.file_name().and_then(|s| s.to_str()) == Some("ls"),
            "path should end in 'ls': {path:?}"
        );
    }
}
