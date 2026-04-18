//! Single-writer state-machine reducer. See PRD §5, §7, §16.13.
//!
//! All state changes go through this reducer via an mpsc channel. The reducer
//! is synchronous and single-threaded; its handler returns a list of side
//! effects for the caller to execute (usually on a separate task).

use serde::{Deserialize, Serialize};

use crate::events::{Agent, Event, EventKind, TurnOutcome};
use crate::session::SessionMeta;
use crate::state::{Phase, SessionState, Verdict};

/// Commands that can mutate session state. Produced by UI clicks, watchdogs,
/// harness completions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum StateCommand {
    /// Start / resume: see PRD §7 "开始"按钮决策树.
    Start {
        goal_check_done: Option<(bool, bool)>, // (claude_done, codex_done) if pre-computed
    },
    Pause,
    Stop,
    /// A turn finished with the given outcome.
    TurnFinished {
        agent: Agent,
        phase: Phase,
        outcome: TurnOutcome,
    },
    /// A Codex review parsed successfully.
    ReviewParsed {
        version: u32,
        verdict: Verdict,
        blocking_count: u32,
    },
    /// Goal-Check returned for one agent. The full `missing` list and
    /// `rationale` are carried through so the UI can show a side-by-side
    /// "distance to goal" view (v1.1+). `shelved` was added in v1.3 for
    /// items both agents agreed to disagree on.
    GoalCheckResult {
        agent: Agent,
        done: bool,
        missing_count: u32,
        missing: Vec<String>,
        shelved: Vec<String>,
        rationale: String,
    },
    /// External event required immediate pause.
    ForcePause { reason: String },
    /// Harness reports authentication failure.
    AuthFailed { agent: Agent },
    /// Harness reports the CLI's provider-side rate/usage limit has been
    /// exhausted. Unlike `AuthFailed`, this self-heals when the provider's
    /// window rolls over. Added v1.3 — see PRD §16.8.
    RateLimited {
        agent: Agent,
        /// RFC3339 time the CLI said it could be retried, if any was
        /// parseable from the error. `None` means "unknown window; user
        /// must hit Resume manually once things unclog".
        retry_at: Option<String>,
    },
}

/// Side effects produced by the reducer. Consumers perform these outside the
/// reducer to keep the reducer pure & fast.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum Effect {
    /// Persist the session meta & emit a state-change event.
    Emit(Event),
    /// Spin up a specific turn next.
    LaunchTurn { agent: Agent, phase: Phase },
    /// Cancel the currently-running turn, if any.
    CancelCurrent,
    /// Transitioned to DONE — notify user, stop loop.
    NotifyDone,
    /// Transitioned to ERRORED or PAUSED — notify user.
    NotifyAttention { reason: String },
}

pub struct Reducer {
    meta: SessionMeta,
    /// Latest review verdict observed for the current round, if any.
    last_verdict: Option<Verdict>,
    /// Latest Goal-Check results for current round.
    gc_claude: Option<bool>,
    gc_codex: Option<bool>,
    /// Missing list sizes across the last few rounds for stagnation detection.
    missing_history: Vec<u32>,
    /// retries counter per (agent, rolling-last-5-turns) for §16.8 flapping.
    recent_retries: Vec<(Agent, u32)>,
    /// Per-phase retry count for the CURRENT turn. Reset on Ok / phase change.
    /// Implements §16.3 "retry-once" for output_malformed and §16.8
    /// stalled/crashed retry-once.
    turn_retries: u32,
}

impl Reducer {
    pub fn new(meta: SessionMeta) -> Self {
        Self {
            meta,
            last_verdict: None,
            gc_claude: None,
            gc_codex: None,
            missing_history: Vec::new(),
            recent_retries: Vec::new(),
            turn_retries: 0,
        }
    }

    pub fn meta(&self) -> &SessionMeta {
        &self.meta
    }

    pub fn meta_mut(&mut self) -> &mut SessionMeta {
        &mut self.meta
    }

    /// Handle a single command and return side effects.
    pub fn handle(&mut self, cmd: StateCommand) -> Vec<Effect> {
        let mut effects = Vec::new();
        match cmd {
            StateCommand::Start { goal_check_done } => {
                if matches!(self.meta.state, SessionState::Running) {
                    // Already running; idempotent.
                    return effects;
                }
                match goal_check_done {
                    Some((true, true)) => {
                        self.transition(&mut effects, SessionState::Done, Phase::Idle);
                        effects.push(Effect::NotifyDone);
                    }
                    _ => {
                        // Decide entry phase based on artifacts present.
                        let next = if self.meta.workdir.join("PRD.md").exists() {
                            if self
                                .meta
                                .workdir
                                .read_dir()
                                .ok()
                                .map(|rd| {
                                    rd.filter_map(|e| e.ok())
                                        .any(|e| {
                                            e.file_name()
                                                .to_string_lossy()
                                                .starts_with("codex_review_v")
                                        })
                                })
                                .unwrap_or(false)
                            {
                                Phase::Refining
                            } else {
                                Phase::Implementing
                            }
                        } else {
                            Phase::Planning
                        };
                        self.transition(&mut effects, SessionState::Running, next);
                        effects.push(Effect::LaunchTurn {
                            agent: Agent::Claude,
                            phase: next,
                        });
                    }
                }
            }
            StateCommand::Pause => {
                if matches!(self.meta.state, SessionState::Running) {
                    effects.push(Effect::CancelCurrent);
                    self.transition(&mut effects, SessionState::Paused, self.meta.phase);
                    effects.push(Effect::NotifyAttention {
                        reason: "paused by user".into(),
                    });
                }
            }
            StateCommand::Stop => {
                if matches!(self.meta.state, SessionState::Running) {
                    effects.push(Effect::CancelCurrent);
                }
                self.transition(&mut effects, SessionState::Abandoned, Phase::Idle);
            }
            StateCommand::ForcePause { reason } => {
                if matches!(self.meta.state, SessionState::Running) {
                    effects.push(Effect::CancelCurrent);
                }
                self.transition(&mut effects, SessionState::Paused, self.meta.phase);
                effects.push(Effect::NotifyAttention { reason });
            }
            StateCommand::AuthFailed { .. } => {
                if matches!(self.meta.state, SessionState::Running) {
                    effects.push(Effect::CancelCurrent);
                }
                self.transition(&mut effects, SessionState::Paused, self.meta.phase);
                effects.push(Effect::NotifyAttention {
                    reason: "CLI reported authentication failure; please re-login".into(),
                });
            }
            StateCommand::RateLimited { agent, retry_at } => {
                if matches!(self.meta.state, SessionState::Running) {
                    effects.push(Effect::CancelCurrent);
                }
                self.meta.retry_at = retry_at.clone();
                self.transition(&mut effects, SessionState::Paused, self.meta.phase);
                let reason = match (agent, retry_at.as_deref()) {
                    (Agent::Claude, Some(t)) => {
                        format!("claude rate-limited; auto-resume at {t}")
                    }
                    (Agent::Codex, Some(t)) => {
                        format!("codex rate-limited; auto-resume at {t}")
                    }
                    (Agent::Claude, None) => {
                        "claude rate-limited; resume when provider window rolls over"
                            .to_string()
                    }
                    (Agent::Codex, None) => {
                        "codex rate-limited; resume when provider window rolls over"
                            .to_string()
                    }
                };
                effects.push(Effect::NotifyAttention { reason });
            }
            StateCommand::TurnFinished { agent, phase, outcome } => {
                self.track_retry(agent, outcome);
                // Per §16.3 and §16.8: some outcomes retry once before giving up.
                let is_retryable = matches!(
                    outcome,
                    TurnOutcome::OutputMalformed
                        | TurnOutcome::Stalled
                        | TurnOutcome::Crashed
                );
                if is_retryable && self.turn_retries == 0 {
                    self.turn_retries += 1;
                    // Stay in the same state; retry the same phase.
                    effects.push(Effect::Emit(Event::new(
                        self.meta.round,
                        EventKind::Note {
                            message: format!(
                                "turn returned {outcome:?}; retrying once in {phase:?}"
                            ),
                        },
                    )));
                    effects.push(Effect::LaunchTurn { agent, phase });
                    return effects;
                }
                // Ok or terminal failure: clear the per-turn retry counter.
                self.turn_retries = 0;
                match outcome {
                    TurnOutcome::Ok => {
                        let next = match phase {
                            Phase::Planning => Some((Agent::Claude, Phase::Implementing)),
                            Phase::Implementing => Some((Agent::Codex, Phase::Reviewing)),
                            Phase::Reviewing => None, // wait for ReviewParsed
                            Phase::Refining => Some((Agent::Codex, Phase::Reviewing)),
                            Phase::GoalCheck => None,
                            Phase::Idle => None,
                        };
                        if let Some((a, p)) = next {
                            self.transition(&mut effects, SessionState::Running, p);
                            effects.push(Effect::LaunchTurn { agent: a, phase: p });
                        }
                    }
                    TurnOutcome::AuthFailed => {
                        self.transition(&mut effects, SessionState::Paused, phase);
                        effects.push(Effect::NotifyAttention {
                            reason: "authentication failure".into(),
                        });
                    }
                    TurnOutcome::RateLimited => {
                        // Rate-limit is recoverable: pause with the
                        // retry_at from meta (set by the orchestrator
                        // dispatching StateCommand::RateLimited before
                        // handling TurnFinished). Caller is expected to
                        // route RateLimited via StateCommand::RateLimited
                        // *first*, but if it slips in as a raw
                        // TurnFinished we still want to pause (not
                        // Errored) so the session can self-heal.
                        self.transition(&mut effects, SessionState::Paused, phase);
                        effects.push(Effect::NotifyAttention {
                            reason: "rate-limited".into(),
                        });
                    }
                    TurnOutcome::Refused
                    | TurnOutcome::Flapping
                    | TurnOutcome::Stalled
                    | TurnOutcome::Crashed
                    | TurnOutcome::OutputMalformed => {
                        self.transition(&mut effects, SessionState::Errored, phase);
                        effects.push(Effect::NotifyAttention {
                            reason: format!("{outcome:?}"),
                        });
                    }
                }
            }
            StateCommand::ReviewParsed {
                version,
                verdict,
                blocking_count,
            } => {
                self.last_verdict = Some(verdict);
                effects.push(Effect::Emit(Event::new(
                    self.meta.round,
                    EventKind::ReviewWritten {
                        version,
                        verdict,
                        blocking_count,
                    },
                )));
                // v1.2+: Run GOAL_CHECK after *every* REVIEWING, regardless
                // of verdict. This gives the user a fresh "distance to goal"
                // signal every cycle (2-10 min) instead of only when Codex
                // happens to approve (which can take many rounds of
                // changes_requested or never happen). The GOAL_CHECK
                // outcome branch (below) still decides DONE vs REFINING:
                //   - both agents done=true → DONE
                //   - either says not-done → REFINING (next round)
                // Blocked verdict stays the legacy Refining path since it
                // signals the review setup itself is malformed; no point
                // asking "are we done?" at that point.
                match verdict {
                    Verdict::Approved | Verdict::ChangesRequested => {
                        self.gc_claude = None;
                        self.gc_codex = None;
                        self.transition(&mut effects, SessionState::Running, Phase::GoalCheck);
                        effects.push(Effect::LaunchTurn {
                            agent: Agent::Claude,
                            phase: Phase::GoalCheck,
                        });
                        effects.push(Effect::LaunchTurn {
                            agent: Agent::Codex,
                            phase: Phase::GoalCheck,
                        });
                    }
                    Verdict::Blocked => {
                        self.meta.round += 1;
                        self.transition(
                            &mut effects,
                            SessionState::Running,
                            Phase::Refining,
                        );
                        effects.push(Effect::LaunchTurn {
                            agent: Agent::Claude,
                            phase: Phase::Refining,
                        });
                    }
                }
            }
            StateCommand::GoalCheckResult {
                agent,
                done,
                missing_count,
                missing,
                shelved,
                rationale,
            } => {
                effects.push(Effect::Emit(Event::new(
                    self.meta.round,
                    EventKind::GoalCheck {
                        agent,
                        done,
                        missing_count,
                        missing,
                        shelved,
                        rationale,
                    },
                )));
                match agent {
                    Agent::Claude => self.gc_claude = Some(done),
                    Agent::Codex => self.gc_codex = Some(done),
                }
                if let (Some(c), Some(d)) = (self.gc_claude, self.gc_codex) {
                    if c && d {
                        self.transition(&mut effects, SessionState::Done, Phase::Idle);
                        effects.push(Effect::NotifyDone);
                    } else {
                        // Record missing trend for stagnation detection.
                        self.missing_history.push(missing_count);
                        if self.is_stagnating() {
                            self.transition(
                                &mut effects,
                                SessionState::Errored,
                                self.meta.phase,
                            );
                            effects.push(Effect::NotifyAttention {
                                reason:
                                    "progress stagnated across 3 rounds; please intervene"
                                        .into(),
                            });
                        } else {
                            self.meta.round += 1;
                            self.transition(
                                &mut effects,
                                SessionState::Running,
                                Phase::Refining,
                            );
                            effects.push(Effect::LaunchTurn {
                                agent: Agent::Claude,
                                phase: Phase::Refining,
                            });
                        }
                    }
                }
            }
        }
        effects
    }

    fn transition(&mut self, effects: &mut Vec<Effect>, to: SessionState, phase: Phase) {
        self.meta.state = to;
        self.meta.phase = phase;
        self.meta.bump();
        effects.push(Effect::Emit(Event::new(
            self.meta.round,
            EventKind::StateChanged {
                to: format!("{to:?}/{phase:?}"),
            },
        )));
    }

    fn track_retry(&mut self, agent: Agent, outcome: TurnOutcome) {
        let is_retry = matches!(
            outcome,
            TurnOutcome::OutputMalformed | TurnOutcome::Stalled | TurnOutcome::Crashed
        );
        if is_retry {
            if let Some((_, n)) = self
                .recent_retries
                .iter_mut()
                .find(|(a, _)| *a == agent)
            {
                *n += 1;
            } else {
                self.recent_retries.push((agent, 1));
            }
        }
        // Keep the list bounded; naive trim.
        if self.recent_retries.len() > 20 {
            self.recent_retries.drain(..10);
        }
    }

    fn is_stagnating(&self) -> bool {
        // Per PRD §11: connected 3 rounds where combined missing size does not
        // strictly decrease.
        if self.missing_history.len() < 3 {
            return false;
        }
        let last3 = &self.missing_history[self.missing_history.len() - 3..];
        !(last3[0] > last3[1] && last3[1] > last3[2])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn mk() -> Reducer {
        Reducer::new(SessionMeta::new("test".into(), PathBuf::from("/tmp/unused")))
    }

    #[test]
    fn start_from_empty_goes_to_planning() {
        let mut r = mk();
        let eff = r.handle(StateCommand::Start {
            goal_check_done: None,
        });
        assert!(matches!(r.meta().state, SessionState::Running));
        assert!(matches!(r.meta().phase, Phase::Planning));
        assert!(eff
            .iter()
            .any(|e| matches!(e, Effect::LaunchTurn { phase: Phase::Planning, .. })));
    }

    #[test]
    fn start_with_both_goal_check_done_goes_to_done() {
        let mut r = mk();
        let eff = r.handle(StateCommand::Start {
            goal_check_done: Some((true, true)),
        });
        assert!(matches!(r.meta().state, SessionState::Done));
        assert!(eff.iter().any(|e| matches!(e, Effect::NotifyDone)));
    }

    #[test]
    fn changes_requested_also_fires_goal_check_on_both() {
        // v1.2+: GOAL_CHECK now runs after every REVIEWING outcome (not
        // just Approved) so the user sees a fresh "distance to goal"
        // signal each cycle.
        let mut r = mk();
        r.handle(StateCommand::Start {
            goal_check_done: None,
        });
        r.handle(StateCommand::TurnFinished {
            agent: Agent::Claude,
            phase: Phase::Planning,
            outcome: TurnOutcome::Ok,
        });
        r.handle(StateCommand::TurnFinished {
            agent: Agent::Claude,
            phase: Phase::Implementing,
            outcome: TurnOutcome::Ok,
        });
        let eff = r.handle(StateCommand::ReviewParsed {
            version: 1,
            verdict: Verdict::ChangesRequested,
            blocking_count: 2,
        });
        let launches: Vec<_> = eff
            .iter()
            .filter_map(|e| match e {
                Effect::LaunchTurn { agent, phase } => Some((*agent, *phase)),
                _ => None,
            })
            .collect();
        assert!(launches.contains(&(Agent::Claude, Phase::GoalCheck)));
        assert!(launches.contains(&(Agent::Codex, Phase::GoalCheck)));
        // Should NOT have jumped straight to Refining.
        assert!(!launches.contains(&(Agent::Claude, Phase::Refining)));
    }

    #[test]
    fn approved_verdict_fires_goal_check_on_both() {
        let mut r = mk();
        r.handle(StateCommand::Start {
            goal_check_done: None,
        });
        // Fast-forward by simulating turn completions.
        r.handle(StateCommand::TurnFinished {
            agent: Agent::Claude,
            phase: Phase::Planning,
            outcome: TurnOutcome::Ok,
        });
        r.handle(StateCommand::TurnFinished {
            agent: Agent::Claude,
            phase: Phase::Implementing,
            outcome: TurnOutcome::Ok,
        });
        let eff = r.handle(StateCommand::ReviewParsed {
            version: 1,
            verdict: Verdict::Approved,
            blocking_count: 0,
        });
        let launches: Vec<_> = eff
            .iter()
            .filter_map(|e| match e {
                Effect::LaunchTurn { agent, phase } => Some((*agent, *phase)),
                _ => None,
            })
            .collect();
        assert!(launches.contains(&(Agent::Claude, Phase::GoalCheck)));
        assert!(launches.contains(&(Agent::Codex, Phase::GoalCheck)));
    }

    #[test]
    fn both_goal_checks_done_yields_done() {
        let mut r = mk();
        r.handle(StateCommand::Start {
            goal_check_done: None,
        });
        r.handle(StateCommand::TurnFinished {
            agent: Agent::Claude,
            phase: Phase::Planning,
            outcome: TurnOutcome::Ok,
        });
        r.handle(StateCommand::TurnFinished {
            agent: Agent::Claude,
            phase: Phase::Implementing,
            outcome: TurnOutcome::Ok,
        });
        r.handle(StateCommand::ReviewParsed {
            version: 1,
            verdict: Verdict::Approved,
            blocking_count: 0,
        });
        r.handle(StateCommand::GoalCheckResult {
            agent: Agent::Claude,
            done: true,
            missing_count: 0,
            missing: Vec::new(),
            shelved: Vec::new(),
            rationale: String::new(),
        });
        let eff = r.handle(StateCommand::GoalCheckResult {
            agent: Agent::Codex,
            done: true,
            missing_count: 0,
            missing: Vec::new(),
            shelved: Vec::new(),
            rationale: String::new(),
        });
        assert!(matches!(r.meta().state, SessionState::Done));
        assert!(eff.iter().any(|e| matches!(e, Effect::NotifyDone)));
    }

    #[test]
    fn stagnation_triggers_errored() {
        let mut r = mk();
        r.missing_history = vec![3, 3, 3];
        assert!(r.is_stagnating());
    }

    #[test]
    fn output_malformed_retries_once_then_errored() {
        let mut r = mk();
        r.handle(StateCommand::Start {
            goal_check_done: None,
        });
        // First malformed output → reducer schedules a retry, stays Running.
        let eff1 = r.handle(StateCommand::TurnFinished {
            agent: Agent::Claude,
            phase: Phase::Planning,
            outcome: TurnOutcome::OutputMalformed,
        });
        assert!(matches!(r.meta().state, SessionState::Running));
        assert!(
            eff1.iter().any(|e| matches!(
                e,
                Effect::LaunchTurn {
                    phase: Phase::Planning,
                    ..
                }
            )),
            "should re-launch the same phase for retry"
        );
        // Second malformed output → transition to Errored.
        let eff2 = r.handle(StateCommand::TurnFinished {
            agent: Agent::Claude,
            phase: Phase::Planning,
            outcome: TurnOutcome::OutputMalformed,
        });
        assert!(matches!(r.meta().state, SessionState::Errored));
        assert!(eff2
            .iter()
            .any(|e| matches!(e, Effect::NotifyAttention { .. })));
    }

    #[test]
    fn refused_skips_retry_goes_straight_to_errored() {
        let mut r = mk();
        r.handle(StateCommand::Start {
            goal_check_done: None,
        });
        let _ = r.handle(StateCommand::TurnFinished {
            agent: Agent::Claude,
            phase: Phase::Planning,
            outcome: TurnOutcome::Refused,
        });
        assert!(matches!(r.meta().state, SessionState::Errored));
    }
}
