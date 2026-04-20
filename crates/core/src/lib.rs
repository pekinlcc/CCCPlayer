//! CCCPlayer core: state machine, persistence, workdir safety, snapshot.
//!
//! This crate is UI-agnostic. The Tauri app consumes it; fake CLIs and tests
//! also consume it.

pub mod events;
pub mod jaccard;
pub mod persistence;
pub mod preflight;
pub mod prompt;
pub mod redact;
pub mod reducer;
pub mod session;
pub mod settings;
pub mod snapshot;
pub mod state;
pub mod workdir;

pub use events::{Event, EventKind};
pub use reducer::{Reducer, StateCommand};
pub use session::{Session, SessionMeta, SCHEMA_VERSION};
pub use state::{Phase, SessionState, Verdict};
