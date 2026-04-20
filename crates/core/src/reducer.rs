//! Single-writer state-machine reducer. See PRD §5, §7, §16.13.
//!
//! All state changes go through this reducer via an mpsc channel. The reducer
//! is synchronous and single-threaded; its handler returns a list of side
//! effects for the caller to execute (usually on a separate task).

use serde::{Deserialize, Serialize};

use crate::events::{Agent, Event, EventKind, TurnOutcome};
use crate::jaccard::{self, DEFAULT_THRESHOLD};
use crate::session::SessionMeta;
use crate::state::{Phase, SessionState, Verdict};

/// How many rounds the v1.6 stagnation detector needs in-window before it
/// will even consider firing. All three of A (count non-decreasing) + B
/// (same items) + ¬C (neither agent tried new angles) must hold across the
/// whole window. Loosened from v1.5's 3 to v1.6's 5 because the triple gate
/// is strict enough that 3 feels jumpy on large sessions — see PRD §11.6.
const STAGNATION_WINDOW: usize = 5;

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
    /// After REFINING completes the orchestrator reads the just-finished
    /// `codex_review_v{N}.md` (which by then contains both Codex's findings
    /// and Claude's response section) and extracts two booleans: did either
    /// agent write a *concrete* `attempted_alternatives` / `counter_argument`
    /// this round (non-empty, non-sentinel)? Reducer stamps those booleans
    /// onto the latest missing-history entry and re-evaluates the v1.6
    /// stagnation detector. Added v1.6.
    RoundAttemptsParsed {
        round: u32,
        claude_tried_new_angle: bool,
        codex_tried_new_angle: bool,
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

/// Per-round signals captured for the v1.6 stagnation detector. One entry is
/// pushed onto `Reducer::missing_history` the moment both agents' goal-checks
/// are in for a round; `attempted_*` fields get stamped later, after REFINING
/// completes and the orchestrator parses the just-finalized review file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundProbe {
    pub round: u32,
    pub claude_missing: Vec<String>,
    pub codex_missing: Vec<String>,
    pub claude_done: bool,
    pub codex_done: bool,
    /// Set later via `StateCommand::RoundAttemptsParsed`.
    pub claude_tried_new_angle: bool,
    pub codex_tried_new_angle: bool,
    /// `true` once the orchestrator has fed us the attempted-alternatives
    /// signals for this round. If false when the detector runs, treat the
    /// round as not-yet-scored (agent may still be writing) — never kill on
    /// rounds we don't have full data for.
    pub attempts_recorded: bool,
}

impl RoundProbe {
    /// max(claude_missing.len(), codex_missing.len()) — the "worst" of the
    /// two per-round counts, matching the v1.4.1-vintage scalar history.
    /// Retained for the NotifyAttention reason string and UI reports.
    pub fn worst_missing_count(&self) -> u32 {
        self.claude_missing.len().max(self.codex_missing.len()) as u32
    }
}

pub struct Reducer {
    meta: SessionMeta,
    /// Latest review verdict observed for the current round, if any.
    last_verdict: Option<Verdict>,
    /// Latest Goal-Check results for current round.
    gc_claude: Option<bool>,
    gc_codex: Option<bool>,
    /// Per-agent missing lists for current round's goal-check. Retained until
    /// both agents report in, then drained into a new `RoundProbe` pushed
    /// onto `missing_history`. Counts are derivable from `.len()`.
    gc_claude_missing: Option<Vec<String>>,
    gc_codex_missing: Option<Vec<String>>,
    /// Rolling per-round signals for stagnation detection. Only the last
    /// `STAGNATION_WINDOW` entries are referenced; older entries are kept so
    /// the session report can show the full trajectory. Upgraded in v1.6
    /// from `Vec<u32>` (counts only) to full probes.
    missing_history: Vec<RoundProbe>,
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
            gc_claude_missing: None,
            gc_codex_missing: None,
            missing_history: Vec::new(),
            recent_retries: Vec::new(),
            turn_retries: 0,
        }
    }

    /// Read-only view of the rolling per-round stagnation probes. Exposed
    /// for session-report rendering (v1.6 terminal-state shows the last
    /// `STAGNATION_WINDOW` entries when the detector fires).
    pub fn missing_history(&self) -> &[RoundProbe] {
        &self.missing_history
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
                // v1.4.1: also emit as a Note so the reason survives into
                // events.log and can be shown on the terminal-state
                // execution report. NotifyAttention alone is currently
                // discarded by apply_effects.
                effects.push(Effect::Emit(Event::new(
                    self.meta.round,
                    EventKind::Note {
                        message: reason.clone(),
                    },
                )));
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
                        self.gc_claude_missing = None;
                        self.gc_codex_missing = None;
                        self.transition(
                            &mut effects,
                            SessionState::Running,
                            Phase::GoalCheck,
                        );
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
                // Emit the full-fidelity event to UI / events.log. We keep a
                // copy of `missing` (clone) in reducer state so we can build
                // a `RoundProbe` once both agents report in; the other fields
                // are consumed solely by the UI.
                let missing_for_event = missing.clone();
                effects.push(Effect::Emit(Event::new(
                    self.meta.round,
                    EventKind::GoalCheck {
                        agent,
                        done,
                        missing_count,
                        missing: missing_for_event,
                        shelved,
                        rationale,
                    },
                )));
                match agent {
                    Agent::Claude => {
                        self.gc_claude = Some(done);
                        self.gc_claude_missing = Some(missing);
                    }
                    Agent::Codex => {
                        self.gc_codex = Some(done);
                        self.gc_codex_missing = Some(missing);
                    }
                }
                if let (Some(c), Some(d)) = (self.gc_claude, self.gc_codex) {
                    if c && d {
                        self.transition(&mut effects, SessionState::Done, Phase::Idle);
                        effects.push(Effect::NotifyDone);
                    } else {
                        // Push ONE RoundProbe per round — the attempts flags
                        // get stamped later via RoundAttemptsParsed once
                        // REFINING has written Claude's response section.
                        // Without this "one per round" discipline a pattern
                        // like Claude=0, Codex=3 (repeating) looks like
                        // [0,3,0,3,0,3] and the stagnation detector
                        // misfires — see decision #84 in PRD. Carried over
                        // from v1.4.1 into the v1.6 RoundProbe shape.
                        let probe = RoundProbe {
                            round: self.meta.round,
                            claude_missing: self
                                .gc_claude_missing
                                .take()
                                .unwrap_or_default(),
                            codex_missing: self
                                .gc_codex_missing
                                .take()
                                .unwrap_or_default(),
                            claude_done: c,
                            codex_done: d,
                            claude_tried_new_angle: false,
                            codex_tried_new_angle: false,
                            attempts_recorded: false,
                        };
                        self.missing_history.push(probe);
                        // Advance to REFINING. Stagnation is NOT checked
                        // here — we need the attempts signals first, which
                        // only arrive after Refining writes its response
                        // section. Detector runs on
                        // `StateCommand::RoundAttemptsParsed`.
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
            StateCommand::RoundAttemptsParsed {
                round,
                claude_tried_new_angle,
                codex_tried_new_angle,
            } => {
                // Find the matching probe by round. Usually the most-recent
                // entry, but orchestrator retries / replays could place it
                // elsewhere — scan from the back.
                if let Some(probe) = self
                    .missing_history
                    .iter_mut()
                    .rev()
                    .find(|p| p.round == round)
                {
                    probe.claude_tried_new_angle = claude_tried_new_angle;
                    probe.codex_tried_new_angle = codex_tried_new_angle;
                    probe.attempts_recorded = true;
                }
                if let Some(reason) = self.stagnation_reason() {
                    effects.push(Effect::Emit(Event::new(
                        self.meta.round,
                        EventKind::Note {
                            message: reason.clone(),
                        },
                    )));
                    self.transition(
                        &mut effects,
                        SessionState::Errored,
                        self.meta.phase,
                    );
                    effects.push(Effect::NotifyAttention { reason });
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

    /// Returns `Some(reason)` if the v1.6 triple-gate (A ∧ B ∧ ¬C) fires on
    /// the trailing `STAGNATION_WINDOW` rounds, `None` otherwise. The reason
    /// string is human-readable and stamped onto both the `Note` event and
    /// the `NotifyAttention` payload so the session report can display it.
    ///
    /// Gates:
    /// - **A**: worst-case missing count across the window did not strictly
    ///   decrease (i.e. at least one round-over-round step was flat or up).
    /// - **B**: each agent's missing list is substantially the same across
    ///   every adjacent pair in the window (Jaccard-based pairing).
    /// - **¬C**: *neither* agent wrote a concrete
    ///   `attempted_alternatives` / `counter_argument` in *any* round of the
    ///   window — per prompt contract, the sentinels "no new angle attempted
    ///   this round" / "conceded; Claude's reason is sound" signal no genuine
    ///   new attempt, and any round without full attempts data is skipped.
    ///
    /// Returns `None` (= do not kill) whenever data is incomplete (fewer
    /// than `STAGNATION_WINDOW` rounds or any round in the window missing
    /// its `attempts_recorded` stamp). Safety bias: we never kill sessions
    /// on partial information.
    fn stagnation_reason(&self) -> Option<String> {
        let n = self.missing_history.len();
        if n < STAGNATION_WINDOW {
            return None;
        }
        let window = &self.missing_history[n - STAGNATION_WINDOW..];

        // Require every round in the window to have its attempts stamp.
        if !window.iter().all(|p| p.attempts_recorded) {
            return None;
        }

        // Gate A — worst-count not strictly decreasing across window.
        let counts: Vec<u32> = window.iter().map(|p| p.worst_missing_count()).collect();
        let strictly_decreasing = counts.windows(2).all(|w| w[0] > w[1]);
        if strictly_decreasing {
            return None;
        }

        // Gate B — per-agent missing lists substantially the same across
        // every adjacent pair in the window.
        let claude_same = window.windows(2).all(|w| {
            jaccard::lists_substantially_same(
                &w[0].claude_missing,
                &w[1].claude_missing,
                DEFAULT_THRESHOLD,
            )
        });
        let codex_same = window.windows(2).all(|w| {
            jaccard::lists_substantially_same(
                &w[0].codex_missing,
                &w[1].codex_missing,
                DEFAULT_THRESHOLD,
            )
        });
        if !(claude_same && codex_same) {
            return None;
        }

        // Gate ¬C — neither agent tried a new angle in ANY window round.
        let any_tried = window
            .iter()
            .any(|p| p.claude_tried_new_angle || p.codex_tried_new_angle);
        if any_tried {
            return None;
        }

        // All gates held — build a reason that tells the user what actually
        // happened. Three facts are load-bearing: window depth, the recurring
        // items from the most recent round, and that NEITHER agent wrote a
        // fresh attempt. Encourages the manual fix (shelve the repeat
        // offenders in PRD) instead of just "we gave up".
        let recurring = {
            let last = window.last().expect("window non-empty");
            let mut items: Vec<&str> = last
                .claude_missing
                .iter()
                .chain(last.codex_missing.iter())
                .map(String::as_str)
                .collect();
            items.sort_unstable();
            items.dedup();
            items.join(" · ")
        };
        let reason = format!(
            "stagnation detector fired: missing-item lists have been \
             substantially unchanged across the last {STAGNATION_WINDOW} \
             rounds ({counts:?} worst-case), and NEITHER agent wrote a \
             fresh `attempted_alternatives` / `counter_argument` in any of \
             those rounds. Recurring items: {recurring}. \
             Consider shelving repeat offenders in PRD's `## Shelved \
             disagreements` so the loop can converge."
        );
        Some(reason)
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

    // ----- v1.6 stagnation detector (A ∧ B ∧ ¬C, 5-round window) ----------

    /// Build a 5-round history where every round has `count` identical
    /// missing items and `attempts_recorded = true` by default; callers
    /// tweak individual rounds via the closure.
    fn history(
        count: u32,
        mut tweak: impl FnMut(&mut RoundProbe, usize),
    ) -> Vec<RoundProbe> {
        (0..STAGNATION_WINDOW)
            .map(|i| {
                let items: Vec<String> = (0..count)
                    .map(|k| format!("repeated item #{k}"))
                    .collect();
                let mut p = RoundProbe {
                    round: i as u32,
                    claude_missing: items.clone(),
                    codex_missing: items,
                    claude_done: false,
                    codex_done: false,
                    claude_tried_new_angle: false,
                    codex_tried_new_angle: false,
                    attempts_recorded: true,
                };
                tweak(&mut p, i);
                p
            })
            .collect()
    }

    #[test]
    fn stagnation_fires_on_repeated_items_without_new_angles() {
        let mut r = mk();
        r.missing_history = history(3, |_, _| {});
        assert!(
            r.stagnation_reason().is_some(),
            "5 rounds with same items and no new angles must fire"
        );
    }

    #[test]
    fn stagnation_skipped_if_count_is_strictly_decreasing() {
        // Gate A fails → not stagnation, regardless of B or C.
        let mut r = mk();
        r.missing_history = (0..STAGNATION_WINDOW)
            .map(|i| {
                let count = (STAGNATION_WINDOW - i) as u32;
                let items: Vec<String> =
                    (0..count).map(|k| format!("item #{k}")).collect();
                RoundProbe {
                    round: i as u32,
                    claude_missing: items.clone(),
                    codex_missing: items,
                    claude_done: false,
                    codex_done: false,
                    claude_tried_new_angle: false,
                    codex_tried_new_angle: false,
                    attempts_recorded: true,
                }
            })
            .collect();
        assert!(r.stagnation_reason().is_none(), "count is decreasing, should not fire");
    }

    #[test]
    fn stagnation_skipped_if_items_drift() {
        // Gate B fails — round 3 swaps in a totally different missing item.
        let mut r = mk();
        r.missing_history = history(2, |p, i| {
            if i == 3 {
                p.claude_missing = vec!["totally unrelated problem".into()];
                p.codex_missing = vec!["totally unrelated problem".into()];
            }
        });
        assert!(
            r.stagnation_reason().is_none(),
            "items drifted, should not fire"
        );
    }

    #[test]
    fn stagnation_skipped_if_any_agent_tried_any_round() {
        // Gate ¬C fails — Claude wrote a concrete attempted_alternatives
        // in one of the rounds, signaling they're still thinking.
        let mut r = mk();
        r.missing_history = history(2, |p, i| {
            if i == 2 {
                p.claude_tried_new_angle = true;
            }
        });
        assert!(
            r.stagnation_reason().is_none(),
            "Claude tried a new angle once; must not fire"
        );
    }

    #[test]
    fn stagnation_skipped_if_any_round_missing_attempts_stamp() {
        // Safety bias: partial data → never kill.
        let mut r = mk();
        r.missing_history = history(2, |p, i| {
            if i == 4 {
                p.attempts_recorded = false;
            }
        });
        assert!(
            r.stagnation_reason().is_none(),
            "incomplete attempts data; must not fire"
        );
    }

    #[test]
    fn stagnation_skipped_below_window_size() {
        let mut r = mk();
        r.missing_history = history(2, |_, _| {})
            .into_iter()
            .take(STAGNATION_WINDOW - 1)
            .collect();
        assert!(
            r.stagnation_reason().is_none(),
            "fewer than window rounds; must not fire"
        );
    }

    #[test]
    fn round_attempts_parsed_transitions_to_errored_when_stagnating() {
        let mut r = mk();
        // Seed 4 rounds of stagnation-looking history, then emit
        // RoundAttemptsParsed for a 5th matching round. Detector should fire.
        r.missing_history = history(2, |_, _| {})
            .into_iter()
            .take(STAGNATION_WINDOW - 1)
            .collect();
        // Push the 5th round with attempts_recorded=false so the command
        // is what fills the signals in.
        r.missing_history.push(RoundProbe {
            round: 99,
            claude_missing: vec!["repeated item #0".into(), "repeated item #1".into()],
            codex_missing: vec!["repeated item #0".into(), "repeated item #1".into()],
            claude_done: false,
            codex_done: false,
            claude_tried_new_angle: false,
            codex_tried_new_angle: false,
            attempts_recorded: false,
        });
        let effs = r.handle(StateCommand::RoundAttemptsParsed {
            round: 99,
            claude_tried_new_angle: false,
            codex_tried_new_angle: false,
        });
        assert!(matches!(r.meta().state, SessionState::Errored));
        assert!(effs
            .iter()
            .any(|e| matches!(e, Effect::NotifyAttention { .. })));
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
