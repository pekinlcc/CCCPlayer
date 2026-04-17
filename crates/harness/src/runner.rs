//! Process runner: spawn CLI, stream stdout line-by-line, drive stall watcher.
//!
//! See PRD §9, §16.1, §16.4, §16.8.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cccplayer_core::events::{Agent, TurnOutcome};
use cccplayer_core::state::Phase;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::stall::StallWatcher;
use crate::turn::{is_auth_failure, is_refusal, TurnResult};

/// One-shot turn input: what to spawn and what prompt to feed.
#[derive(Debug, Clone)]
pub struct TurnInput {
    pub agent: Agent,
    pub phase: Phase,
    pub cli_path: PathBuf,
    pub workdir: PathBuf,
    /// The rendered prompt; harness is responsible for handing it to the CLI
    /// via stdin.
    pub prompt: String,
    /// Extra flags to pass, e.g. `["--phase", "planning"]` for fake CLIs or
    /// real ones. The harness already prepends the auto-approve flag.
    pub args: Vec<String>,
    /// Auto-approve flag name (`--dangerously-skip-permissions` or similar).
    pub auto_approve_flag: Option<String>,
    pub stall_threshold: Duration,
}

/// Stream event surfaced to subscribers.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    Stdout(String),
    Stderr(String),
    Heartbeat,
    /// A `usage` stream-json line was parsed; caller should accumulate.
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    Finished {
        exit_code: Option<i32>,
        duration_ms: u64,
    },
}

/// Look for token usage on a single stream-json line. Handles two shapes:
///
/// 1. Flat (fake CLIs, legacy):
///    `{"type":"usage","input_tokens":N,"output_tokens":M}`
/// 2. Nested (real Claude Code `--output-format stream-json --verbose`):
///    `{"type":"assistant","message":{…,"usage":{"input_tokens":N,
///     "output_tokens":M,"cache_creation_input_tokens":…,
///     "cache_read_input_tokens":…}}}`
///
/// Each `assistant` message represents one API call; callers should sum
/// deltas across the turn. `result` messages (session totals) are
/// deliberately ignored to avoid double counting.
pub fn parse_usage_line(line: &str) -> Option<(u64, u64)> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let ty = v.get("type").and_then(|x| x.as_str())?;
    let usage = match ty {
        "usage" => &v,
        "assistant" => v.get("message")?.get("usage")?,
        _ => return None,
    };
    let i = usage.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
    let o = usage
        .get("output_tokens")
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    if i == 0 && o == 0 {
        return None;
    }
    Some((i, o))
}

/// Codex `exec` writes a plain-text total near the end of the run:
///
/// ```text
/// tokens used
/// 160,317
/// ```
///
/// Scan the combined stdout+stderr transcript for that pair and return the
/// last occurrence (later runs overwrite earlier ones in the same stream).
/// Returns `None` if no such pair is present.
pub fn parse_codex_total_tokens(text: &str) -> Option<u64> {
    let mut last: Option<u64> = None;
    let mut expect_number = false;
    for raw in text.lines() {
        let line = raw.trim();
        if expect_number {
            if line.is_empty() {
                continue;
            }
            let digits: String = line.chars().filter(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                if let Ok(n) = digits.parse::<u64>() {
                    last = Some(n);
                }
            }
            expect_number = false;
            continue;
        }
        if line == "tokens used" {
            expect_number = true;
        }
    }
    last
}

pub struct HarnessRunner;

impl HarnessRunner {
    /// Run one turn to completion (or cancellation). Returns the classified
    /// outcome. Pushes [`StreamEvent`]s to `tx` as they arrive.
    pub async fn run(
        input: TurnInput,
        tx: mpsc::Sender<StreamEvent>,
        mut cancel: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<TurnResult> {
        let start = Instant::now();
        let watcher = StallWatcher::new(input.stall_threshold);

        let mut cmd = Command::new(&input.cli_path);
        if let Some(flag) = &input.auto_approve_flag {
            cmd.arg(flag);
        }
        // Harness always scopes the agent to the workdir (see §16.10) by
        // setting cwd. Some real CLIs also want an --add-dir arg; the caller
        // puts that in `args`.
        cmd.args(&input.args);
        cmd.current_dir(&input.workdir);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.kill_on_drop(true);

        // Create an independent process group so we can signal the entire
        // tree on cancel. See §16.1.
        #[cfg(target_family = "unix")]
        unsafe {
            cmd.pre_exec(|| {
                // setsid() puts the child in a new session (and new pgid).
                if libc_stub::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let mut child = cmd.spawn()?;

        // Feed the prompt via stdin.
        if let Some(mut stdin) = child.stdin.take() {
            let prompt = input.prompt.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(prompt.as_bytes()).await;
                let _ = stdin.shutdown().await;
            });
        }

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let stdout_tx = tx.clone();
        let stderr_tx = tx.clone();
        let watcher_out = watcher.clone();
        let watcher_err = watcher.clone();

        let stdout_acc = Arc::new(tokio::sync::Mutex::new(String::new()));
        let stderr_acc = Arc::new(tokio::sync::Mutex::new(String::new()));

        let stdout_acc_cl = stdout_acc.clone();
        let stdout_handle = tokio::spawn(async move {
            if let Some(s) = stdout {
                let mut reader = BufReader::new(s).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    watcher_out.bump();
                    let redacted = cccplayer_core::redact::redact(&line);
                    {
                        let mut acc = stdout_acc_cl.lock().await;
                        acc.push_str(&redacted);
                        acc.push('\n');
                    }
                    // Extract usage events before forwarding the raw line.
                    if let Some((i, o)) = parse_usage_line(&redacted) {
                        let _ = stdout_tx
                            .send(StreamEvent::Usage {
                                input_tokens: i,
                                output_tokens: o,
                            })
                            .await;
                    }
                    let _ = stdout_tx.send(StreamEvent::Stdout(redacted)).await;
                }
            }
        });

        let stderr_acc_cl = stderr_acc.clone();
        let stderr_handle = tokio::spawn(async move {
            if let Some(s) = stderr {
                let mut reader = BufReader::new(s).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    watcher_err.bump();
                    let redacted = cccplayer_core::redact::redact(&line);
                    {
                        let mut acc = stderr_acc_cl.lock().await;
                        acc.push_str(&redacted);
                        acc.push('\n');
                    }
                    let _ = stderr_tx.send(StreamEvent::Stderr(redacted)).await;
                }
            }
        });

        // Supervise: wait on the child while also watching cancel + stall.
        let exit = loop {
            tokio::select! {
                biased;
                _ = cancel.changed() => {
                    if *cancel.borrow() {
                        signal_child_group(&child, Signal::Int);
                        // Give it 5s to exit cleanly.
                        if tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_err() {
                            signal_child_group(&child, Signal::Term);
                            if tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_err() {
                                signal_child_group(&child, Signal::Kill);
                            }
                        }
                        break child.wait().await.ok();
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(500)) => {
                    if watcher.is_stalled() {
                        signal_child_group(&child, Signal::Int);
                        if tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_err() {
                            signal_child_group(&child, Signal::Term);
                            if tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_err() {
                                signal_child_group(&child, Signal::Kill);
                            }
                        }
                        break child.wait().await.ok();
                    }
                }
                status = child.wait() => {
                    break status.ok();
                }
            }
        };

        let _ = stdout_handle.await;
        let _ = stderr_handle.await;
        let duration_ms = start.elapsed().as_millis() as u64;
        let _ = tx
            .send(StreamEvent::Finished {
                exit_code: exit.as_ref().and_then(|e| e.code()),
                duration_ms,
            })
            .await;

        let stdout_tail = stdout_acc.lock().await.clone();
        let stderr_tail = stderr_acc.lock().await.clone();

        let outcome = classify(
            exit.as_ref().and_then(|e| e.code()),
            &stdout_tail,
            &stderr_tail,
            watcher.is_stalled(),
            input.workdir.as_path(),
            input.phase,
            input.agent,
        );

        Ok(TurnResult {
            agent: input.agent,
            phase: input.phase,
            outcome,
            exit_code: exit.and_then(|e| e.code()),
            duration_ms,
            stdout_tail,
            stderr_tail,
        })
    }
}

fn classify(
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
    stalled: bool,
    workdir: &Path,
    phase: Phase,
    agent: Agent,
) -> TurnOutcome {
    // Order matters per §16.8: auth_failed > refused > stalled > crashed >
    // output_malformed > ok.
    if is_auth_failure(stderr) || is_auth_failure(stdout) {
        return TurnOutcome::AuthFailed;
    }
    if exit_code == Some(0) && is_refusal(stdout) && !files_touched(workdir, phase) {
        return TurnOutcome::Refused;
    }
    if stalled {
        return TurnOutcome::Stalled;
    }
    if exit_code.map(|c| c != 0).unwrap_or(true) {
        return TurnOutcome::Crashed;
    }
    if !output_well_formed(workdir, phase, stdout, agent) {
        return TurnOutcome::OutputMalformed;
    }
    TurnOutcome::Ok
}

fn output_well_formed(workdir: &Path, phase: Phase, stdout: &str, agent: Agent) -> bool {
    use crate::parsers::{extract_claude_text, parse_goal_check, parse_review, prd_is_well_formed};
    match phase {
        Phase::Planning => {
            let Ok(prd) = std::fs::read_to_string(workdir.join("PRD.md")) else {
                return false;
            };
            prd_is_well_formed(&prd)
        }
        Phase::Implementing => {
            // "At least one non-.cccplayer file was created/modified" is
            // approximated by checking the stdout summary line format.
            // Works for plain-text fakes and real Claude stream-json alike
            // ("files: …" appears literally in the envelope's "text" field).
            let _ = agent;
            stdout.contains("files:")
        }
        Phase::Refining => {
            let Some(n) = latest_review_version(workdir) else {
                return false;
            };
            let path = workdir.join(format!("codex_review_v{n}.md"));
            let Ok(body) = std::fs::read_to_string(path) else {
                return false;
            };
            body.contains("## Claude Code 回应")
        }
        Phase::Reviewing => {
            let Some(n) = latest_review_version(workdir) else {
                return false;
            };
            let path = workdir.join(format!("codex_review_v{n}.md"));
            let Ok(body) = std::fs::read_to_string(path) else {
                return false;
            };
            parse_review(&body).is_some()
        }
        Phase::GoalCheck => {
            // Claude emits stream-json; goal-check JSON lives inside the
            // model's text reply (assistant.content or result.result).
            // Codex emits plain text, parse stdout directly.
            let text = match agent {
                Agent::Claude => extract_claude_text(stdout),
                Agent::Codex => stdout.to_string(),
            };
            parse_goal_check(&text).is_some()
        }
        Phase::Idle => true,
    }
}

fn files_touched(workdir: &Path, phase: Phase) -> bool {
    // Approximation: for any non-readonly phase, we'd need diff tracking.
    // Here we trust that the agent that wrote nothing is the only one
    // classified as "refused". During output_malformed detection we already
    // check the artifact. For safety we just return true so that `refused`
    // classification requires BOTH a refusal keyword AND no files touched —
    // and here we default to false so refusal must be a clear stdout signal.
    match phase {
        Phase::GoalCheck => true, // read-only, so "no files touched" is expected
        _ => latest_mtime_after_workdir_created(workdir).is_some(),
    }
}

fn latest_mtime_after_workdir_created(workdir: &Path) -> Option<std::time::SystemTime> {
    walkdir::WalkDir::new(workdir)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            !e.path()
                .components()
                .any(|c| c.as_os_str() == ".cccplayer")
        })
        .filter_map(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
        .max()
}

fn latest_review_version(workdir: &Path) -> Option<u32> {
    let re = regex::Regex::new(r"^codex_review_v(\d+)\.md$").unwrap();
    let mut max = 0u32;
    for e in std::fs::read_dir(workdir).ok()?.flatten() {
        if let Some(n) = e.file_name().to_str().and_then(|s| {
            re.captures(s)
                .and_then(|c| c.get(1).and_then(|m| m.as_str().parse().ok()))
        }) {
            max = max.max(n);
        }
    }
    if max == 0 {
        None
    } else {
        Some(max)
    }
}

#[cfg(target_family = "unix")]
enum Signal {
    Int,
    Term,
    Kill,
}
#[cfg(not(target_family = "unix"))]
#[allow(dead_code)]
enum Signal {
    Int,
    Term,
    Kill,
}

#[cfg(target_family = "unix")]
fn signal_child_group(child: &tokio::process::Child, sig: Signal) {
    let Some(pid) = child.id() else {
        return;
    };
    let sig = match sig {
        Signal::Int => libc_stub::SIGINT,
        Signal::Term => libc_stub::SIGTERM,
        Signal::Kill => libc_stub::SIGKILL,
    };
    unsafe {
        libc_stub::killpg(pid as i32, sig);
    }
}
#[cfg(not(target_family = "unix"))]
fn signal_child_group(_child: &tokio::process::Child, _sig: Signal) {}

#[cfg(target_family = "unix")]
mod libc_stub {
    extern "C" {
        pub fn killpg(pgrp: i32, sig: i32) -> i32;
        pub fn setsid() -> i32;
    }
    pub const SIGINT: i32 = 2;
    pub const SIGTERM: i32 = 15;
    pub const SIGKILL: i32 = 9;
}

#[cfg(test)]
mod tests {
    use super::{parse_codex_total_tokens, parse_usage_line};

    #[test]
    fn parses_flat_usage_line() {
        let line = r#"{"type":"usage","input_tokens":12,"output_tokens":34}"#;
        assert_eq!(parse_usage_line(line), Some((12, 34)));
    }

    #[test]
    fn parses_nested_assistant_usage() {
        let line = r#"{"type":"assistant","message":{"id":"msg_01","model":"claude-opus","content":[],"usage":{"input_tokens":5,"cache_creation_input_tokens":8533,"cache_read_input_tokens":11718,"output_tokens":77}}}"#;
        assert_eq!(parse_usage_line(line), Some((5, 77)));
    }

    #[test]
    fn skips_zero_usage() {
        let line = r#"{"type":"assistant","message":{"usage":{"input_tokens":0,"output_tokens":0}}}"#;
        assert_eq!(parse_usage_line(line), None);
    }

    #[test]
    fn ignores_unrelated_types() {
        assert_eq!(parse_usage_line(r#"{"type":"thinking"}"#), None);
        assert_eq!(parse_usage_line(r#"{"type":"result","usage":{"input_tokens":999}}"#), None);
    }

    #[test]
    fn codex_total_picks_last_pair() {
        let text = "some output\ntokens used\n1,234\n...more...\ntokens used\n160,317\nthen trailing text\n";
        assert_eq!(parse_codex_total_tokens(text), Some(160_317));
    }

    #[test]
    fn codex_total_returns_none_when_missing() {
        assert_eq!(parse_codex_total_tokens("no tokens here\n"), None);
    }
}
