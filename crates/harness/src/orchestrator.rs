//! End-to-end orchestrator that drives a Session from start to DONE using
//! the reducer and the harness. This is the code the Tauri app invokes; it
//! is also the target of the integration tests.
//!
//! See PRD §5 state machine and §7 "开始"按钮决策树.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use cccplayer_core::events::{Agent, Event, EventKind, RawLogLine, RawStream};
use cccplayer_core::persistence::{
    append_event, fingerprint, load_usage, save_session_meta, save_usage, FileFingerprint,
};
use cccplayer_core::prompt::{render, PromptSet, RenderContext};
use cccplayer_core::reducer::{Effect, Reducer, StateCommand};
use cccplayer_core::session::{Session, SessionMeta, UsageTotals};
use cccplayer_core::snapshot;
use cccplayer_core::state::{Phase, SessionState, Verdict};
use tokio::sync::{mpsc, watch};

use crate::parsers::{
    extract_claude_text, parse_goal_check, parse_review, parse_round_attempts,
};
use crate::runner::{parse_codex_total_tokens, HarnessRunner, StreamEvent, TurnInput};

/// External handle to pause or stop a running Orchestrator without holding
/// the `Orchestrator` itself. Cloneable; all clones point at the same
/// underlying atomic flag.
#[derive(Clone)]
pub struct CancelHandle {
    cancel: Arc<AtomicBool>,
    stop_not_pause: Arc<AtomicBool>,
}

impl CancelHandle {
    /// Ask the orchestrator to stop after cancelling the current turn and
    /// transition to ABANDONED.
    pub fn stop(&self) {
        self.stop_not_pause.store(true, Ordering::SeqCst);
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Ask the orchestrator to pause after cancelling the current turn and
    /// transition to PAUSED (resumable).
    pub fn pause(&self) {
        self.stop_not_pause.store(false, Ordering::SeqCst);
        self.cancel.store(true, Ordering::SeqCst);
    }
}

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
    /// External signal for pause/stop: set to `true` by `AppState::pause()` or
    /// `AppState::stop()`. Checked at the top of each `step()` and wired into
    /// every turn via a `watch::Receiver`.
    cancel: Arc<AtomicBool>,
    /// `true` if `cancel` was a *stop* (abandon), `false` if it was a *pause*.
    stop_not_pause: Arc<AtomicBool>,
    /// Optional sink for per-line raw stdout/stderr from spawned CLIs. The
    /// Tauri app layer sets this so the UI can render a live raw-log drawer.
    /// When `None`, the runner's stream output is still written to the
    /// on-disk transcripts but not forwarded anywhere else.
    raw_sink: Option<mpsc::UnboundedSender<RawLogLine>>,
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
            cancel: Arc::new(AtomicBool::new(false)),
            stop_not_pause: Arc::new(AtomicBool::new(false)),
            raw_sink: None,
        })
    }

    /// Attach a sink that receives every stdout/stderr line produced by
    /// spawned CLIs, tagged with agent/phase/round. See [`RawLogLine`].
    pub fn set_raw_sink(&mut self, tx: mpsc::UnboundedSender<RawLogLine>) {
        self.raw_sink = Some(tx);
    }

    /// Returns a handle the caller (AppState) can use to trigger pause/stop
    /// from outside the orchestrator task.
    pub fn cancel_handle(&self) -> CancelHandle {
        CancelHandle {
            cancel: self.cancel.clone(),
            stop_not_pause: self.stop_not_pause.clone(),
        }
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
        // External cancel? Decide PAUSED vs ABANDONED and exit the loop.
        if self.cancel.load(Ordering::SeqCst) {
            if self.stop_not_pause.load(Ordering::SeqCst) {
                let effs = self.reducer.handle(StateCommand::Stop);
                self.apply_effects(effs, events_tx).await?;
            } else {
                let effs = self.reducer.handle(StateCommand::Pause);
                self.apply_effects(effs, events_tx).await?;
            }
            return Ok(());
        }
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
                // v1.6 stagnation: after a successful REFINING, the latest
                // codex_review_v{N}.md now contains BOTH Codex's findings
                // (with any `counter_argument:` lines) AND Claude's response
                // section (with any `attempted_alternatives:` lines). Parse
                // it into per-round attempt signals and feed the reducer
                // before transitioning out of Refining — the detector runs
                // on that command. Only fire on Ok outcomes; partial /
                // malformed turns go through the usual retry/error path.
                if matches!(phase, Phase::Refining)
                    && matches!(
                        result.outcome,
                        cccplayer_core::events::TurnOutcome::Ok
                    )
                {
                    if let Some(n) = latest_review(self.session.workdir()) {
                        let path = self
                            .session
                            .workdir()
                            .join(format!("codex_review_v{n}.md"));
                        if let Ok(body) = std::fs::read_to_string(&path) {
                            let attempts = parse_round_attempts(&body);
                            let effs = self.reducer.handle(
                                StateCommand::RoundAttemptsParsed {
                                    round: self.reducer.meta().round,
                                    claude_tried_new_angle: attempts
                                        .claude_tried_new_angle,
                                    codex_tried_new_angle: attempts
                                        .codex_tried_new_angle,
                                },
                            );
                            self.apply_effects(effs, events_tx).await?;
                        }
                    }
                }
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
                // Claude's goal-check JSON is buried inside stream-json
                // envelopes; extract the model text first. Codex emits
                // plain text, parse directly.
                let claude_gc = parse_goal_check(&extract_claude_text(&claude_res.stdout_tail));
                let codex_gc = parse_goal_check(&codex_res.stdout_tail);
                self.handle_turn_result(claude_res, events_tx).await?;
                self.handle_turn_result(codex_res, events_tx).await?;
                if let Some(g) = claude_gc {
                    let effs = self.reducer.handle(StateCommand::GoalCheckResult {
                        agent: Agent::Claude,
                        done: g.done,
                        missing_count: g.missing.len() as u32,
                        missing: g.missing.clone(),
                        shelved: g.shelved.clone(),
                        rationale: g.rationale.clone(),
                    });
                    self.apply_effects(effs, events_tx).await?;
                }
                if let Some(g) = codex_gc {
                    let effs = self.reducer.handle(StateCommand::GoalCheckResult {
                        agent: Agent::Codex,
                        done: g.done,
                        missing_count: g.missing.len() as u32,
                        missing: g.missing.clone(),
                        shelved: g.shelved.clone(),
                        rationale: g.rationale.clone(),
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
            // Real CLIs: flag strategy differs per vendor.
            match agent {
                Agent::Claude => {
                    // `claude -p <prompt>` is headless print mode. With
                    // --output-format=stream-json, --verbose is mandatory
                    // (Claude Code 2.x enforces this and exits 1 otherwise).
                    args.push("-p".to_string());
                    args.push(prompt.0.clone());
                    args.push("--add-dir".to_string());
                    args.push(self.session.workdir().display().to_string());
                    args.push("--output-format".to_string());
                    args.push("stream-json".to_string());
                    args.push("--verbose".to_string());
                }
                Agent::Codex => {
                    // Codex 0.1x uses `codex exec <prompt>` for non-
                    // interactive mode. It takes no --add-dir or
                    // --output-format flags; stdout is plain text.
                    args.push("exec".to_string());
                    args.push(prompt.0.clone());
                }
            }
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

        // Snapshot the latest review version BEFORE the turn runs so we
        // can detect the case where Codex "completed" a REVIEWING turn
        // but did not actually produce a new codex_review_v{N+1}.md.
        // That slips past the per-file output_well_formed check because
        // an older review still exists and still parses. See decision
        // #70 in PRD: without this, a Codex that quietly hit rate-limit
        // and wrote nothing spirals the state machine by re-ingesting
        // stale reviews as fresh verdicts. Added v1.3.
        let pre_turn_review_max = if matches!(phase, Phase::Reviewing) {
            latest_review(self.session.workdir())
        } else {
            None
        };

        // Emit AgentStarted.
        let _ = append_event(
            &self.session,
            &Event::new(round, EventKind::AgentStarted { agent, phase }),
        );
        let _ = events_tx.send(Event::new(round, EventKind::AgentStarted { agent, phase }));

        let (stream_tx, mut stream_rx) = mpsc::channel::<StreamEvent>(128);
        // Plumb the orchestrator-wide cancel atomic into a per-turn watch
        // channel so the runner's select! can pick it up. We poll the atomic
        // on an interval; cheap relative to the turn itself.
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let cancel_atomic = self.cancel.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(200)).await;
                if cancel_atomic.load(Ordering::SeqCst) {
                    let _ = cancel_tx.send(true);
                    break;
                }
                if cancel_tx.is_closed() {
                    break;
                }
            }
        });

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
                            if let Some(tx) = &self.raw_sink {
                                let _ = tx.send(RawLogLine {
                                    at: chrono::Utc::now(),
                                    round,
                                    agent,
                                    phase,
                                    stream: RawStream::Stdout,
                                    line: line.clone(),
                                });
                            }
                        }
                        Some(StreamEvent::Stderr(line)) => {
                            transcript.extend_from_slice(line.as_bytes());
                            transcript.push(b'\n');
                            if let Some(tx) = &self.raw_sink {
                                let _ = tx.send(RawLogLine {
                                    at: chrono::Utc::now(),
                                    round,
                                    agent,
                                    phase,
                                    stream: RawStream::Stderr,
                                    line: line.clone(),
                                });
                            }
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
                    let mut result = res?;
                    require_new_review(
                        self.session.workdir(),
                        phase,
                        pre_turn_review_max,
                        &mut result,
                    );
                    let _ = std::fs::write(&transcript_path, &transcript);
                    codex_fallback_usage(
                        agent,
                        &transcript,
                        &mut session_usage_delta,
                    );
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
        let mut result = runner.await?;
        require_new_review(
            self.session.workdir(),
            phase,
            pre_turn_review_max,
            &mut result,
        );
        let _ = std::fs::write(&transcript_path, &transcript);
        codex_fallback_usage(agent, &transcript, &mut session_usage_delta);
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
        use cccplayer_core::events::TurnOutcome;

        // Rate-limit is handled through a distinct StateCommand so the
        // reducer can park `retry_at` on SessionMeta for the auto-resume
        // scheduler. We still dispatch the TurnFinished afterwards to
        // keep the per-turn retry counter and missing_history consistent
        // — the reducer's TurnFinished/RateLimited branch is careful to
        // transition only if the session is still Running.
        if matches!(result.outcome, TurnOutcome::RateLimited) {
            let retry_at = result.retry_at.map(|t| t.to_rfc3339());
            let effs = self.reducer.handle(StateCommand::RateLimited {
                agent: result.agent,
                retry_at,
            });
            self.apply_effects(effs, events_tx).await?;
            return Ok(());
        }

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

/// Codex `exec` does not emit per-line stream-json usage; it prints a single
/// `tokens used\n<N>` pair near the end. If the turn produced no structured
/// usage events, scan the collected transcript for that pattern and apply it
/// as a one-shot input-token delta. No-op for Claude or if nothing matches.
/// Reject "silent" REVIEWING turns — ones that classified Ok but did
/// not actually produce a `codex_review_v{N+1}.md`. This catches Codex
/// hitting its usage limit and exiting 0 with only an ERROR line, and
/// any other case where the agent politely did nothing. Without this
/// the orchestrator re-ingests the *previous* review as if it were
/// fresh every cycle and round ticks indefinitely (observed in the
/// wild: 166 rounds vs 28 actual reviews).
///
/// No-op for non-Reviewing phases and for outcomes already classified
/// as a failure or rate-limit — those have their own handling paths.
fn require_new_review(
    workdir: &Path,
    phase: Phase,
    pre_turn_max: Option<u32>,
    result: &mut crate::turn::TurnResult,
) {
    use cccplayer_core::events::TurnOutcome;
    if !matches!(phase, Phase::Reviewing) {
        return;
    }
    // Only downgrade Ok-classified turns; other outcomes already
    // tell a truer story.
    if !matches!(result.outcome, TurnOutcome::Ok) {
        return;
    }
    let post_turn_max = latest_review(workdir);
    if post_turn_max <= pre_turn_max {
        result.outcome = TurnOutcome::OutputMalformed;
    }
}

fn codex_fallback_usage(agent: Agent, transcript: &[u8], delta: &mut (u64, u64)) {
    if agent != Agent::Codex {
        return;
    }
    if delta.0 != 0 || delta.1 != 0 {
        return;
    }
    let Ok(text) = std::str::from_utf8(transcript) else {
        return;
    };
    if let Some(n) = parse_codex_total_tokens(text) {
        // Codex's total is not split into input/output; bucket it as input
        // for display. UX will show "claude=I+O codex=N+0".
        delta.0 = n;
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
