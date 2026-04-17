//! Harness layer: wraps a child CLI process, parses its stream-json output,
//! exposes cancel + stall detection.
//!
//! Implements PRD §9 (Harness trait), §16.1 (CLI invocation), §16.4 (stall
//! detection), §16.8 (turn classifier), §16.15 (auto-approve flag probe).

pub mod orchestrator;
pub mod parsers;
pub mod runner;
pub mod stall;
pub mod turn;

pub use orchestrator::{Orchestrator, OrchestratorConfig};
pub use runner::{HarnessRunner, TurnInput};
pub use stall::StallWatcher;
pub use turn::TurnResult;
