//! Structured event stream. See PRD §6.1.
//!
//! Events are written append-only to `.cccplayer/events.log` (one JSON object
//! per line) and also fanned out to UI subscribers.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::state::{Phase, Verdict};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub at: DateTime<Utc>,
    pub round: u32,
    #[serde(flatten)]
    pub kind: EventKind,
}

impl Event {
    pub fn new(round: u32, kind: EventKind) -> Self {
        Self {
            at: Utc::now(),
            round,
            kind,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    SessionCreated {
        goal_preview: String,
        workdir: String,
    },
    AgentStarted {
        agent: Agent,
        phase: Phase,
    },
    AgentFinished {
        agent: Agent,
        phase: Phase,
        duration_ms: u64,
        outcome: TurnOutcome,
    },
    FileEdited {
        path: String,
        added: u32,
        removed: u32,
    },
    FileCreated {
        path: String,
    },
    FileRead {
        path: String,
    },
    ToolInvoked {
        name: String,
        summary: String,
    },
    ReviewWritten {
        version: u32,
        verdict: Verdict,
        blocking_count: u32,
    },
    GoalCheck {
        agent: Agent,
        done: bool,
        missing_count: u32,
    },
    StateChanged {
        to: String,
    },
    Stall {
        seconds: u64,
    },
    Note {
        message: String,
    },
    Error {
        code: String,
        message: String,
    },
    Heartbeat {
        claude_tokens: Option<u64>,
        codex_tokens: Option<u64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Claude,
    Codex,
}

/// Per §16.8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutcome {
    Ok,
    OutputMalformed,
    Stalled,
    Crashed,
    AuthFailed,
    Refused,
    Flapping,
}
