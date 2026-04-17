//! End-to-end orchestrator that drives a Session from start to DONE using
//! the reducer and the harness. This is the code the Tauri app invokes; it
//! is also the target of the integration tests.
//!
//! See PRD §5 state machine and §7 "开始"按钮决策树.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use cccplayer_core::events::{Agent, Event, EventKind};
use cccplayer_core::persistence::{
    append_event, fingerprint, load_usage, save_session_meta, save_usage, FileFingerprint,
};
use cccplayer_core::prompt::{render, PromptSet, RenderContext};
use cccplayer_core::reducer::{Effect, Reducer, StateCommand};
use cccplayer_core::session::{Session, SessionMeta, UsageTotals};
use cccplayer_core::snapshot;
use cccplayer_core::state::{Phase, SessionState, Verdict};
use tokio::sync::{mpsc, watch};

use crate::parsers::{parse_goal_check, parse_review};
use crate::runner::{HarnessRunner, StreamEvent, TurnInput};

/// Configuration for the orchestrator — where the CLIs live, which flags to
/// pass, stall threshold, etc. Populated from preflight + settings.
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    pub claude_path: PathBuf,
    pub codex_path: PathBuf,
    pub claude_auto_approve_flag: Option<String>,
    pub codex_auto_approve_flag: Option<String>,
    pub stall_threshold: Duration,
    /// Extra args the fake CLIs accept (`--phase <name>`). The real CLIs do
    /// not need these; this lets the integration test target our fakes while
    /// staying on the same code path.
    pub fake_mode: bool,
}

pub struct Orchestrator {
    session: Session,
    meta: SessionMeta,
    reducer: Reducer,
    config: OrchestratorConfig,
    prompts: PromptSet,
    usage: UsageTotals,
    /// Fingerprint of `PRD.md` recorded at the start of the current Round so
    /// we can detect user edits between rounds and avoid silent clobbers.
    /// See PRD §16.5.
    prd_fingerprint: Option<FileFingerprint>,
    /// Same for `GOAL.md` — used to detect both deletion and edits.
    goal_fingerprint: Option<FileFingerprint>,
}

impl Orchestrator {
    pub fn new(session: Session, config: OrchestratorConfig) -> Result<Self> {
        let meta = cccplayer_core::persistence::load_session_meta(&session)?
            .unwrap_or_else(|| {
                SessionMeta::new(
                    uuid_like(),
                    session.workdir().to_path_buf(),
                )
            });
        let reducer = Reducer::new(meta.clone());
        let prompts = PromptSet::defaults();
        let usage = load_usage(&session).unwrap_or_default();
        Ok(Self {
            session,
            meta,
            reducer,
            config,
            prompts,
            usage,
            prd_fingerprint: None,
            goal_fingerprint: None,
        })
    }

    /// Create `round-00` snapshot if it doesn't exist yet, and emit the
    /// initial `SessionCreated` event. Safe to call on resume.
    pub fn ensure_initialized(&mut self, goal_preview: &str) -> Result<()> {
        let round0 = snapshot::snapshot_path(&self.session.snapshots_dir(), 0);
        if !round0.exists() {
            snapshot::create(
                self.session.workdir(),
                &round0,
                snapshot::DEFAULT_EXCLUDES,
            )
            .context("create round-00 snapshot")?;
        }
        let event = Event::new(
            0,
            EventKind::SessionCreated {
                goal_preview: goal_preview.to_string(),
                workdir: self.session.workdir().display().to_string(),
            },
        );
        let _ = append_event(&self.session, &event);
        save_session_meta(&self.session, &self.meta)?;
        Ok(())
    }

    pub fn meta(&self) -> &SessionMeta {
        &self.meta
    }

    /// Run the orchestration loop until it reaches a terminal or paused state.
    /// Produces [`Event`]s on `events_tx` for UI consumption.
    pub async fn run(&mut self, events_tx: mpsc::UnboundedSender<Event>) -> Result<()> {
        // Start command: let the reducer decide where to go.
        let effs = self.reducer.handle(StateCommand::Start {
            goal_check_done: None,
        });
        self.apply_effects(effs, &events_tx).await?;
        while matches!(self.reducer.meta().state, SessionState::Running) {
            self.step(&events_tx).await?;
        }
        save_session_meta(&self.session, self.reducer.meta())?;
        self.meta = self.reducer.meta().clone();
        Ok(())
    }

    async fn step(&mut self, events_tx: &mpsc::UnboundedSender<Event>) -> Result<()> {
        // §16.5 / §16.11 pre-flight: GOAL.md must still exist, and if we have
        // a prior PRD.md fingerprint, make sure the file hasn't been edited
        // from under us. Either violation forces a PAUSED transition with a
        // clear reason, and we return without running the turn.
        if !self.session.goal_path().exists() {
            let effs = self.reducer.handle(StateCommand::ForcePause {
                reason: "GOAL.md is missing; please restore or start a new session".into(),
            });
            self.apply_effects(effs, events_tx).await?;
            return Ok(());
        }
        // If GOAL.md changed since we first saw it, that violates the §9.3
        // immutability contract; pause.
        if let Some(prev) = self.goal_fingerprint.clone() {
            if let Some(now) = fingerprint(&self.session.goal_path()).unwrap_or(None) {
                if now != prev {
                    let effs = self.reducer.handle(StateCommand::ForcePause {
                        reason: "GOAL.md was modified externally; session goal is immutable"
                            .into(),
                    });
                    self.apply_effects(effs, events_tx).await?;
                    return Ok(());
                }
            }
        } else {
            self.goal_fingerprint = fingerprint(&self.session.goal_path()).unwrap_or(None);
        }
        // For PLANNING and REFINING specifically, a user edit to PRD.md
        // between rounds must not be silently overwritten. We emit a Note
        // event so the UI can show a conflict banner. M2 will upgrade this
        // to an interactive 3-way dialog per §16.5.
        let phase = self.reducer.meta().phase;
        if matches!(phase, Phase::Planning | Phase::Refining) {
            if let Some(prev) = self.prd_fingerprint.clone() {
                if let Some(now) = fingerprint(&self.session.prd_path()).unwrap_or(None) {
                    if now != prev {
                        let ev = Event::new(
                            self.reducer.meta().round,
                            EventKind::Note {
                                message: format!(
                                    "PRD.md was edited externally since round start \
                                     (prev sha={}, cur sha={}). Agent will proceed and \
                                     may overwrite. See PRD §16.5.",
                                    &prev.sha256[..12],
                                    &now.sha256[..12]
                                ),
                            },
                        );
                        let _ = append_event(&self.session, &ev);
                        let _ = events_tx.send(ev);
                    }
                }
            }
        }
        match phase {
            Phase::Planning | Phase::Implementing | Phase::Refining => {
                let (result, delta) = self.run_turn(Agent::Claude, phase, events_tx).await?;
                self.apply_usage_delta(Agent::Claude, delta, events_tx);
                self.handle_turn_result(result, events_tx).await?;
            }
            Phase::Reviewing => {
                let (result, delta) = self.run_turn(Agent::Codex, phase, events_tx).await?;
                self.apply_usage_delta(Agent::Codex, delta, events_tx);
                self.handle_turn_result(result, events_tx).await?;
                // If the turn produced a new review, parse it and feed
                // ReviewParsed.
                if let Some(n) = latest_review(self.session.workdir()) {
                    let path = self
                        .session
                        .workdir()
                        .join(format!("codex_review_v{n}.md"));
                    if let Ok(body) = std::fs::read_to_string(&path) {
                        if let Some(r) = parse_review(&body) {
                            let effs = self.reducer.handle(StateCommand::ReviewParsed {
                                version: n,
                                verdict: r.verdict,
                                blocking_count: r.blocking.len() as u32,
                            });
                            self.apply_effects(effs, events_tx).await?;
                        }
                    }
                }
            }
            Phase::GoalCheck => {
                // Run both in parallel. §5/§7.
                let claude_fut = self.run_turn(Agent::Claude, Phase::GoalCheck, events_tx);
                let codex_fut = self.run_turn(Agent::Codex, Phase::GoalCheck, events_tx);
                let (claude_res, codex_res) = tokio::join!(claude_fut, codex_fut);
                let (claude_res, claude_delta) = claude_res?;
                let (codex_res, codex_delta) = codex_res?;
                self.apply_usage_delta(Agent::Claude, claude_delta, events_tx);
                self.apply_usage_delta(Agent::Codex, codex_delta, events_tx);
                let claude_gc = parse_goal_check(&claude_res.stdout_tail);
                let codex_gc = parse_goal_check(&codex_res.stdout_tail);
                self.handle_turn_result(claude_res, events_tx).await?;
                self.handle_turn_result(codex_res, events_tx).await?;
                if let Some(g) = claude_gc {
                    let effs = self.reducer.handle(StateCommand::GoalCheckResult {
                        agent: Agent::Claude,
                        done: g.done,
                        missing_count: g.missing.len() as u32,
                    });
                    self.apply_effects(effs, events_tx).await?;
                }
                if let Some(g) = codex_gc {
                    let effs = self.reducer.handle(StateCommand::GoalCheckResult {
                        agent: Agent::Codex,
                        done: g.done,
                        missing_count: g.missing.len() as u32,
                    });
                    self.apply_effects(effs, events_tx).await?;
                }
            }
            Phase::Idle => {
                // Should not happen mid-Running; treat as done.
                return Ok(());
            }
        }
        // After the turn, refresh the PRD fingerprint so the next round's
        // external-edit guard is relative to what the agent just wrote.
        self.prd_fingerprint = fingerprint(&self.session.prd_path()).unwrap_or(None);
        Ok(())
    }

    async fn run_turn(
        &self,
        agent: Agent,
        phase: Phase,
        events_tx: &mpsc::UnboundedSender<Event>,
    ) -> Result<(crate::turn::TurnResult, (u64, u64))> {
        // Take snapshot at round start if this is the first turn of a round
        // and we haven't yet.
        let round = self.reducer.meta().round;
        let round_snap = snapshot::snapshot_path(&self.session.snapshots_dir(), round);
        let is_fresh_round = !round_snap.exists();
        if is_fresh_round && round > 0 {
            snapshot::create(
                self.session.workdir(),
                &round_snap,
                snapshot::DEFAULT_EXCLUDES,
            )
            .context("create round snapshot")?;
        }
        let _ = is_fresh_round; // fingerprint refresh is handled at the end of step()

        let ctx = RenderContext {
            workdir: self.session.workdir().to_path_buf(),
            round,
            latest_review_version: latest_review(self.session.workdir()),
            next_review_version: latest_review(self.session.workdir()).map(|n| n + 1).or(Some(1)),
        };
        let template = match phase {
            Phase::Planning => &self.prompts.planning,
            Phase::Implementing => &self.prompts.implementing,
            Phase::Refining => &self.prompts.refining,
            Phase::Reviewing => &self.prompts.reviewing,
            Phase::GoalCheck => &self.prompts.goal_check,
            Phase::Idle => {
                return Err(anyhow::anyhow!("cannot run turn in Idle phase"));
            }
        };
        let prompt = render(template, &self.prompts.common, &ctx);

        let (cli_path, flag, phase_name) = match (agent, phase) {
            (Agent::Claude, Phase::Planning) => {
                (self.config.claude_path.clone(), self.config.claude_auto_approve_flag.clone(), "planning")
            }
            (Agent::Claude, Phase::Implementing) => (
                self.config.claude_path.clone(),
                self.config.claude_auto_approve_flag.clone(),
                "implementing",
            ),
            (Agent::Claude, Phase::Refining) => (
                self.config.claude_path.clone(),
                self.config.claude_auto_approve_flag.clone(),
                "refining",
            ),
            (Agent::Claude, Phase::GoalCheck) => (
                self.config.claude_path.clone(),
                self.config.claude_auto_approve_flag.clone(),
                "goal-check",
            ),
            (Agent::Codex, Phase::Reviewing) => (
                self.config.codex_path.clone(),
                self.config.codex_auto_approve_flag.clone(),
                "reviewing",
            ),
            (Agent::Codex, Phase::GoalCheck) => (
                self.config.codex_path.clone(),
                self.config.codex_auto_approve_flag.clone(),
                "goal-check",
            ),
            _ => {
                return Err(anyhow::anyhow!(
                    "unsupported (agent,phase) combination: {agent:?}/{phase:?}"
                ));
            }
        };

        let mut args = Vec::new();
        if self.config.fake_mode {
            args.push("--phase".to_string());
            args.push(phase_name.to_string());
            args.push("--workdir".to_string());
            args.push(self.session.workdir().display().to_string());
        } else {
            // Real CLIs: scope the agent to the workdir. The exact flag name
            // varies across versions; we pick the most common. The production
            // path could read this from `cli-info.json`.
            args.push("-p".to_string());
            args.push(prompt.0.clone());
            args.push("--add-dir".to_string());
            args.push(self.session.workdir().display().to_string());
            args.push("--output-format".to_string());
            args.push("stream-json".to_string());
        }

        let input = TurnInput {
            agent,
            phase,
            cli_path,
            workdir: self.session.workdir().to_path_buf(),
            prompt: prompt.0,
            args,
            auto_approve_flag: flag,
            stall_threshold: self.config.stall_threshold,
        };

        // Emit AgentStarted.
        let _ = append_event(
            &self.session,
            &Event::new(round, EventKind::AgentStarted { agent, phase }),
        );
        let _ = events_tx.send(Event::new(round, EventKind::AgentStarted { agent, phase }));

        let (stream_tx, mut stream_rx) = mpsc::channel::<StreamEvent>(128);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let session_events_dir = self.session.transcripts_dir();
        std::fs::create_dir_all(&session_events_dir).ok();
        let transcript_path = session_events_dir.join(format!(
            "round-{round:02}-{}-{}.txt",
            agent_name(agent),
            phase_short(phase)
        ));
        let mut transcript = Vec::new();

        let runner = HarnessRunner::run(input, stream_tx, cancel_rx);
        tokio::pin!(runner);

        // Drain stream events while the runner runs. We keep reading until the
        // runner completes or sends a Finished event.
        let mut session_usage_delta = (0u64, 0u64);
        loop {
            tokio::select! {
                biased;
                maybe = stream_rx.recv() => {
                    match maybe {
                        Some(StreamEvent::Stdout(line)) => {
                            transcript.extend_from_slice(line.as_bytes());
                            transcript.push(b'\n');
                        }
                        Some(StreamEvent::Stderr(line)) => {
                            transcript.extend_from_slice(line.as_bytes());
                            transcript.push(b'\n');
                        }
                        Some(StreamEvent::Usage {
                            input_tokens,
                            output_tokens,
                        }) => {
                            session_usage_delta.0 += input_tokens;
                            session_usage_delta.1 += output_tokens;
                        }
                        Some(StreamEvent::Finished { .. }) => break,
                        Some(StreamEvent::Heartbeat) => {}
                        None => break,
                    }
                }
                res = &mut runner => {
                    let result = res?;
                    let _ = std::fs::write(&transcript_path, &transcript);
                    let _ = append_event(
                        &self.session,
                        &Event::new(
                            round,
                            EventKind::AgentFinished {
                                agent,
                                phase,
                                duration_ms: result.duration_ms,
                                outcome: result.outcome,
                            },
                        ),
                    );
                    let _ = events_tx.send(Event::new(
                        round,
                        EventKind::AgentFinished {
                            agent,
                            phase,
                            duration_ms: result.duration_ms,
                            outcome: result.outcome,
                        },
                    ));
                    return Ok((result, session_usage_delta));
                }
            }
        }
        // Got Finished — wait on runner to retrieve the TurnResult.
        let result = runner.await?;
        let _ = std::fs::write(&transcript_path, &transcript);
        let _ = append_event(
            &self.session,
            &Event::new(
                round,
                EventKind::AgentFinished {
                    agent,
                    phase,
                    duration_ms: result.duration_ms,
                    outcome: result.outcome,
                },
            ),
        );
        let _ = events_tx.send(Event::new(
            round,
            EventKind::AgentFinished {
                agent,
                phase,
                duration_ms: result.duration_ms,
                outcome: result.outcome,
            },
        ));
        Ok((result, session_usage_delta))
    }

    /// Apply a turn's usage delta to the session's running totals, persist
    /// `usage.json`, and emit a `Heartbeat` event so the UI can redraw.
    fn apply_usage_delta(
        &mut self,
        agent: Agent,
        delta: (u64, u64),
        events_tx: &mpsc::UnboundedSender<Event>,
    ) {
        let (i, o) = delta;
        match agent {
            Agent::Claude => {
                self.usage.claude.input_tokens += i;
                self.usage.claude.output_tokens += o;
            }
            Agent::Codex => {
                self.usage.codex.input_tokens += i;
                self.usage.codex.output_tokens += o;
            }
        }
        let _ = save_usage(&self.session, &self.usage);
        let ev = Event::new(
            self.reducer.meta().round,
            EventKind::Heartbeat {
                claude_tokens: Some(self.usage.claude_total()),
                codex_tokens: Some(self.usage.codex_total()),
            },
        );
        let _ = append_event(&self.session, &ev);
        let _ = events_tx.send(ev);
    }

    async fn handle_turn_result(
        &mut self,
        result: crate::turn::TurnResult,
        events_tx: &mpsc::UnboundedSender<Event>,
    ) -> Result<()> {
        let cmd = StateCommand::TurnFinished {
            agent: result.agent,
            phase: result.phase,
            outcome: result.outcome,
        };
        let effs = self.reducer.handle(cmd);
        self.apply_effects(effs, events_tx).await?;
        Ok(())
    }

    async fn apply_effects(
        &mut self,
        effs: Vec<Effect>,
        events_tx: &mpsc::UnboundedSender<Event>,
    ) -> Result<()> {
        for eff in effs {
            match eff {
                Effect::Emit(ev) => {
                    let _ = append_event(&self.session, &ev);
                    let _ = events_tx.send(ev);
                }
                Effect::LaunchTurn { .. } => {
                    // The step() loop handles launching based on reducer
                    // state, so Effect::LaunchTurn is informational here.
                }
                Effect::CancelCurrent => {}
                Effect::NotifyDone | Effect::NotifyAttention { .. } => {}
            }
        }
        save_session_meta(&self.session, self.reducer.meta())?;
        Ok(())
    }
}

fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("s{ns:x}")
}

fn latest_review(workdir: &Path) -> Option<u32> {
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

fn agent_name(a: Agent) -> &'static str {
    match a {
        Agent::Claude => "claude",
        Agent::Codex => "codex",
    }
}

fn phase_short(p: Phase) -> &'static str {
    match p {
        Phase::Planning => "plan",
        Phase::Implementing => "impl",
        Phase::Refining => "refine",
        Phase::Reviewing => "review",
        Phase::GoalCheck => "gc",
        Phase::Idle => "idle",
    }
}

// Avoid unused warning when Verdict import unused in some configurations.
#[allow(dead_code)]
fn _verdict_reference() -> Verdict {
    Verdict::Approved
}
