//! State machine types. See PRD §5 and §11.

use serde::{Deserialize, Serialize};

/// The top-level state of a Session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionState {
    /// Session created, never started.
    Created,
    /// A turn is currently executing. The [`Phase`] carries the detail.
    Running,
    /// User paused or an external event (auth failed, workdir lost, external
    /// edit conflict) paused the loop. Resumable.
    Paused,
    /// Loop hit an unrecoverable state and needs user input. Resumable only
    /// after the user decides.
    Errored,
    /// Goal achieved. Terminal.
    Done,
    /// User abandoned the session. Terminal.
    Abandoned,
}

/// Which phase of the Round we are in (or were last in).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Phase {
    Idle,
    Planning,
    Implementing,
    Reviewing,
    Refining,
    GoalCheck,
}

/// Codex review verdict as parsed from `codex_review_v{n}.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Approved,
    ChangesRequested,
    Blocked,
}
