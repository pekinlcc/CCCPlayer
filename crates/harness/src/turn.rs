//! Turn-level types and output classifier. See PRD §16.8.

use cccplayer_core::events::{Agent, TurnOutcome};
use cccplayer_core::state::Phase;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnResult {
    pub agent: Agent,
    pub phase: Phase,
    pub outcome: TurnOutcome,
    /// Final exit code from the CLI, if the process did exit.
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub stdout_tail: String,
    pub stderr_tail: String,
    /// For `TurnOutcome::RateLimited`, the wall-clock time the CLI said
    /// to retry at (if one was parseable from the error message). Used
    /// to schedule an auto-resume. `None` means "try again later, we
    /// don't know when". Added v1.3.
    #[serde(default)]
    pub retry_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Patterns that indicate the CLI refused to carry out the task. See §16.8
/// `refused` row.
pub fn is_refusal(text: &str) -> bool {
    let t = text.to_lowercase();
    const NEEDLES: &[&str] = &[
        "i can't help",
        "i can't assist",
        "i won't",
        "i cannot help",
        "unable to help",
        "i refuse",
        "i'm unable to",
        "i am unable to",
    ];
    NEEDLES.iter().any(|n| t.contains(n))
}

/// Patterns that indicate an authentication or authorization failure. See
/// §16.8 `auth_failed` row.
pub fn is_auth_failure(stderr: &str) -> bool {
    let t = stderr.to_lowercase();
    const NEEDLES: &[&str] = &[
        "unauthorized",
        "unauthenticated",
        "http 401",
        "status 401",
        "not logged in",
        "please login",
        "please run `claude login`",
        "please run `codex login`",
        "invalid api key",
        "api key not found",
    ];
    NEEDLES.iter().any(|n| t.contains(n))
}
