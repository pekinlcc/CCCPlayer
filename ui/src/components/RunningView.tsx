import { useEffect, useMemo, useRef, useState } from 'react'
import {
  pauseSession,
  startSession,
  stopSession,
  subscribeEvents,
  subscribeRawLog,
} from '../api'
import type { Event, GoalCheckSnapshot, Phase, RawLogLine, SessionState } from '../types'
import { Meter, PauseIcon, PlayIcon, Shell, StopIcon } from './Shell'
import type { ShellStatus } from './Shell'

const RAW_LOG_CAP = 2000

const FAILING_OUTCOMES = new Set([
  'crashed',
  'auth_failed',
  'stalled',
  'refused',
  'output_malformed',
  'flapping',
])

const PHASE_ORDER: Phase[] = [
  'PLANNING',
  'IMPLEMENTING',
  'REVIEWING',
  'REFINING',
  'GOAL_CHECK',
]
const PHASE_SHORT: Record<Phase, string> = {
  IDLE: '—',
  PLANNING: 'PLAN',
  IMPLEMENTING: 'IMPL',
  REVIEWING: 'REV',
  REFINING: 'REFN',
  GOAL_CHECK: 'GC',
}

export function RunningView(props: {
  workdir: string
  goal: string
  onDone: (state: SessionState) => void
}) {
  const [phase, setPhase] = useState<Phase>('PLANNING')
  const [round, setRound] = useState(1)
  const [events, setEvents] = useState<Event[]>([])
  const [elapsed, setElapsed] = useState(0)
  const [claudeTokens, setClaudeTokens] = useState(0)
  const [codexTokens, setCodexTokens] = useState(0)
  const [rawLog, setRawLog] = useState<RawLogLine[]>([])
  const [lastActivityAt, setLastActivityAt] = useState<number>(Date.now())
  const [failCount, setFailCount] = useState(0)
  const [nowTick, setNowTick] = useState(0)
  const [pausing, setPausing] = useState(false)
  const [stopping, setStopping] = useState(false)
  const [sessionState, setSessionState] = useState<SessionState>('RUNNING')
  // Latest goal-check snapshot per agent, powering the "Remaining to goal"
  // block. Updated whenever a goal_check event lands. See PRD §6.1 / §7.
  const [claudeGc, setClaudeGc] = useState<GoalCheckSnapshot | null>(null)
  const [codexGc, setCodexGc] = useState<GoalCheckSnapshot | null>(null)
  const rawScrollRef = useRef<HTMLPreElement | null>(null)

  useEffect(() => {
    const t = setInterval(() => {
      setElapsed((e) => e + 1)
      setNowTick((n) => n + 1)
    }, 1000)
    return () => clearInterval(t)
  }, [])

  useEffect(() => {
    let alive = true
    let unsubscribe: (() => void) | null = null
    void (async () => {
      unsubscribe = await subscribeEvents((ev) => {
        if (!alive) return
        setEvents((prev) => [...prev, ev])
        setLastActivityAt(Date.now())
        const k = String(ev.kind)
        if (k === 'state_changed') {
          // reducer emits the payload via `format!("{to:?}/{phase:?}")`, so
          // both halves are Rust Debug — CamelCase ("Errored/Refining",
          // "Running/GoalCheck", …). Accept any case and normalize to the
          // SCREAMING_SNAKE_CASE Phase string the UI uses.
          const to = String((ev as { to?: string }).to ?? '')
          const match = /^([A-Za-z]+)\/([A-Za-z]+)$/.exec(to)
          if (match) {
            const state = match[1].toLowerCase()
            setPhase(camelToScreamingSnake(match[2]) as Phase)
            if (state === 'done') props.onDone('DONE')
            if (state === 'abandoned') props.onDone('ABANDONED')
            if (state === 'errored') props.onDone('ERRORED')
            // Paused/Running/Created stay on the Running screen; the
            // titlebar status follows `sessionState`.
            setSessionState(state.toUpperCase() as SessionState)
          }
        }
        if (typeof ev.round === 'number') setRound(ev.round + 1)
        if (k === 'heartbeat') {
          const c = (ev as { claude_tokens?: number }).claude_tokens ?? 0
          const x = (ev as { codex_tokens?: number }).codex_tokens ?? 0
          setClaudeTokens(c)
          setCodexTokens(x)
        }
        if (k === 'agent_finished') {
          const outcome = String((ev as { outcome?: string }).outcome ?? '')
          if (FAILING_OUTCOMES.has(outcome)) setFailCount((n) => n + 1)
        }
        if (k === 'goal_check') {
          const agent = String((ev as { agent?: string }).agent ?? '')
          const snap: GoalCheckSnapshot = {
            at: String(ev.at),
            round: typeof ev.round === 'number' ? ev.round : 0,
            done: Boolean((ev as { done?: boolean }).done),
            missing:
              (ev as { missing?: string[] }).missing ??
              // Older logs (pre-v1.1) only carry missing_count; fall back.
              [],
            rationale: String((ev as { rationale?: string }).rationale ?? ''),
          }
          if (agent === 'claude') setClaudeGc(snap)
          else if (agent === 'codex') setCodexGc(snap)
        }
      })
    })()
    return () => {
      alive = false
      if (unsubscribe) unsubscribe()
    }
  }, [props])

  useEffect(() => {
    let alive = true
    let unsubscribe: (() => void) | null = null
    void (async () => {
      unsubscribe = await subscribeRawLog((line) => {
        if (!alive) return
        setRawLog((prev) => {
          const next = prev.length >= RAW_LOG_CAP ? prev.slice(-RAW_LOG_CAP + 1) : prev
          return [...next, line]
        })
        setLastActivityAt(Date.now())
      })
    })()
    return () => {
      alive = false
      if (unsubscribe) unsubscribe()
    }
  }, [])

  useEffect(() => {
    if (rawScrollRef.current) {
      rawScrollRef.current.scrollTop = rawScrollRef.current.scrollHeight
    }
  }, [rawLog])

  // Clear the optimistic pause/stop indicator when the reducer confirms the
  // real state transition. `pausing`/`stopping` are "I asked for this but
  // haven't seen it yet"; once we do, drop them so the shell stops showing
  // the `…` suffix.
  useEffect(() => {
    if (sessionState === 'PAUSED') setPausing(false)
    if (sessionState === 'ABANDONED') setStopping(false)
  }, [sessionState])

  const sinceLastActivity = useMemo(() => {
    void nowTick
    return Math.max(0, Math.floor((Date.now() - lastActivityAt) / 1000))
  }, [nowTick, lastActivityAt])

  const phaseIdx = PHASE_ORDER.indexOf(phase)

  const trackLabel = props.goal.split('\n')[0].slice(0, 60) || 'session'

  // Map reducer-reported SessionState to the shell's status badge. Done /
  // Abandoned / Errored route away via props.onDone, so the only values we
  // really render here are CREATED / RUNNING / PAUSED. While we wait for
  // the reducer to actually apply a user-requested pause/stop (can take a
  // few seconds — the orchestrator has to signal SIGINT to the child and
  // let the current turn unwind), show an optimistic PAUSING / STOPPING
  // state so the button click feels responsive. `pausing`/`stopping` reset
  // once we see the real state transition via state_changed events.
  const isPausing = pausing && sessionState !== 'PAUSED'
  const isStopping = stopping && sessionState !== 'ABANDONED'
  const shellStatus: ShellStatus =
    isStopping ? 'stopped'
    : isPausing ? 'paused'
    : sessionState === 'PAUSED' ? 'paused'
    : sessionState === 'ERRORED' ? 'errored'
    : sessionState === 'DONE' ? 'done'
    : sessionState === 'ABANDONED' ? 'stopped'
    : 'running'
  const shellLabel =
    isStopping ? 'Stopping…'
    : isPausing ? 'Pausing…'
    : sessionState === 'PAUSED' ? 'Paused'
    : sessionState === 'ERRORED' ? 'Errored'
    : sessionState === 'DONE' ? 'Done'
    : sessionState === 'ABANDONED' ? 'Stopped'
    : 'Running'

  const canResume = sessionState === 'PAUSED' && !isPausing

  return (
    <Shell status={shellStatus} statusLabel={shellLabel}>
      <div className="display">
        <div className="dline">
          <span className="label">Track</span>
          <span className="value magenta">{trackLabel}</span>
        </div>
        <div className="dline">
          <span className="label">Folder</span>
          <span className="pathfield" style={{ flex: 1 }}>{props.workdir}</span>
        </div>
        <div className="phase-row">
          <span className="label">Phase</span>
          <div className="phase-segs">
            {PHASE_ORDER.map((p, i) => (
              <span
                key={p}
                className={`seg ${i <= phaseIdx ? 'on mag' : ''}`}
                title={PHASE_SHORT[p]}
              />
            ))}
          </div>
          <span className="value magenta" style={{ marginLeft: 6 }}>{phase}</span>
          <span style={{ flex: 1 }} />
          <span className="label">Round</span>
          <span className="value">{String(round).padStart(2, '0')}</span>
        </div>
      </div>

      <div className="transport">
        <button
          type="button"
          className="tbtn play big"
          title={canResume ? 'Resume' : 'Running'}
          disabled={!canResume}
          onClick={async () => {
            if (!canResume) return
            // Resuming is just a fresh Start — AppState.start is
            // idempotent and reuses the session dir, so the reducer picks
            // up at the recorded state/phase.
            await startSession(props.goal, props.workdir)
          }}
        >
          <PlayIcon />
        </button>
        <button
          type="button"
          className="tbtn"
          title="Pause"
          onClick={async () => {
            if (isPausing || sessionState === 'PAUSED') return
            setPausing(true)
            // Intentionally don't reset `pausing` here on IPC ack — it
            // stays true until we see state_changed → PAUSED land in the
            // timeline (see useEffect above). Otherwise the UI "blinks"
            // out of Pausing… back to Running while the orchestrator is
            // still unwinding the current turn.
            try {
              await pauseSession()
            } catch {
              setPausing(false)
            }
          }}
          disabled={isPausing || sessionState === 'PAUSED'}
        >
          <PauseIcon />
        </button>
        <button
          type="button"
          className="tbtn stop"
          title="Stop"
          onClick={async () => {
            if (isStopping) return
            setStopping(true)
            try {
              await stopSession()
            } catch {
              setStopping(false)
            }
          }}
          disabled={isStopping}
        >
          <StopIcon />
        </button>
        <span className="sep" />
        <span className="pill info">T+ {formatElapsed(elapsed)}</span>
        <span className={`pill ${sinceLastActivity > 60 ? 'warn' : ''}`}>
          {formatSince(sinceLastActivity)}
        </span>
        {failCount > 0 && <span className="pill danger">{failCount} fail</span>}
        <span className="spacer" />
        <div className="meters">
          <Meter label="CLAUDE" active={claudeTokens > 0 || phase !== 'REVIEWING'} />
          <Meter label="CODEX" active={codexTokens > 0 || phase === 'REVIEWING'} />
        </div>
      </div>

      <div className="progress-panel">
        <div className="progress-head">
          <span>▼ Progress</span>
          <span style={{ flex: 1 }} />
          <span style={{ color: 'var(--dim)', letterSpacing: '1.4px' }}>
            live · {rawLog.length.toLocaleString()} raw lines
          </span>
        </div>

        <RemainingBlock claude={claudeGc} codex={codexGc} />

        <div className="kv-grid">
          <KV label="Elapsed" big={formatElapsed(elapsed)} />
          <div className="kv tokens-kv">
            <span className="label">Tokens</span>
            <div className="tokens-split">
              <div className="pair">
                <span className="who claude">Claude</span>
                <span className="num">{claudeTokens.toLocaleString()}</span>
              </div>
              <div className="pair">
                <span className="who codex">Codex</span>
                <span className="num">{codexTokens.toLocaleString()}</span>
              </div>
            </div>
          </div>
          <KV
            label="Last activity"
            big={formatSince(sinceLastActivity)}
            color={sinceLastActivity > 60 ? 'amber' : undefined}
          />
          <KV label="Round" big={String(round).padStart(2, '0')} />
          <KV
            label="Failures"
            big={String(failCount)}
            color={failCount > 0 ? 'danger' : undefined}
          />
        </div>

        <div className="split">
          <div className="pane">
            <h4>Timeline <span className="count">· {events.length}</span></h4>
            {events.length === 0 ? (
              <ul className="timeline-list">
                <li className="hint">Waiting for first event…</li>
              </ul>
            ) : (
              <ul className="timeline-list">
                {events.map((ev, i) => {
                  const kind = String(ev.kind)
                  const classes = ['evt', `evt-${kind}`]
                  if (kind === 'agent_finished') {
                    const outcome = String((ev as { outcome?: string }).outcome ?? '')
                    if (FAILING_OUTCOMES.has(outcome)) classes.push('evt-fail')
                    else if (outcome === 'ok') classes.push('evt-ok')
                  }
                  if (kind === 'error') classes.push('evt-fail')
                  return (
                    <li key={i} className={classes.join(' ')}>
                      <span className="ts">{new Date(ev.at).toLocaleTimeString()}</span>
                      <span className="kind">{kind}</span>
                      <span className="detail">{renderEventDetail(ev)}</span>
                    </li>
                  )
                })}
              </ul>
            )}
          </div>
          <div className="pane">
            <h4>Raw stream <span className="count">· {rawLog.length}</span></h4>
            <pre className="raw-log" ref={rawScrollRef}>
              {rawLog.length === 0 ? (
                <span className="raw-empty">(waiting for CLI output…)</span>
              ) : (
                rawLog.map((l, i) => (
                  <span key={i}>
                    <span className="raw-ts">[{formatRawTs(l.at)}]</span>{' '}
                    <span className={`raw-source-${l.stream}`}>{l.agent}/{l.stream}</span>{' '}
                    {l.line}
                    {'\n'}
                  </span>
                ))
              )}
            </pre>
          </div>
        </div>
      </div>

      <div className="shell-foot">
        <span>Workdir</span>
        <code>{props.workdir}</code>
        <span className="spacer" />
        <span className="live">● LIVE</span>
      </div>
    </Shell>
  )
}

function KV(props: { label: string; big: string; color?: 'lime' | 'amber' | 'magenta' | 'danger' }) {
  return (
    <div className="kv">
      <span className="label">{props.label}</span>
      <span className={`big ${props.color ?? ''}`}>{props.big}</span>
    </div>
  )
}

function renderEventDetail(ev: Event): string {
  const k = String(ev.kind)
  const pick = (key: string) => (ev as Record<string, unknown>)[key]
  switch (k) {
    case 'agent_started':
      return `${pick('agent')} → ${pick('phase')}`
    case 'agent_finished': {
      const outcome = String(pick('outcome') ?? '')
      const dur = Number(pick('duration_ms') ?? 0)
      return `${pick('agent')} ${pick('phase')} — ${outcome} (${formatMs(dur)})`
    }
    case 'state_changed':
      return String(pick('to') ?? '')
    case 'note':
      return String(pick('message') ?? '')
    case 'error':
      return `[${pick('code')}] ${pick('message')}`
    case 'heartbeat':
      return `claude=${pick('claude_tokens') ?? 0} codex=${pick('codex_tokens') ?? 0}`
    case 'stall':
      return `no output for ${pick('seconds')}s`
    case 'prd_written':
      return `v${pick('version')} (${pick('bytes')} bytes)`
    case 'review_written':
      return `v${pick('version')} — ${pick('verdict')} (${pick('blocking_count')} blocking)`
    case 'goal_check':
      return `${pick('agent')} done=${pick('done')} missing=${pick('missing_count')}`
    case 'file_edited':
      return `${pick('path')} (+${pick('added')} -${pick('removed')})`
    case 'file_created':
    case 'file_read':
      return String(pick('path') ?? '')
    case 'tool_invoked':
      return `${pick('name')}: ${pick('summary')}`
    case 'session_created':
      return String(pick('goal_preview') ?? '')
    default:
      return ''
  }
}

function formatElapsed(secs: number): string {
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
}

function formatMs(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  const s = Math.round(ms / 100) / 10
  return `${s}s`
}

function formatSince(secs: number): string {
  if (secs < 60) return `${secs}s ago`
  const m = Math.floor(secs / 60)
  const s = secs % 60
  return `${m}m ${s}s ago`
}

// "Refining" → "REFINING", "GoalCheck" → "GOAL_CHECK", "Idle" → "IDLE".
// The reducer emits state_changed payloads via Rust Debug, which for fieldless
// enum variants is CamelCase; the UI Phase type is SCREAMING_SNAKE_CASE.
function camelToScreamingSnake(camel: string): string {
  return camel
    .replace(/([A-Z])/g, '_$1')
    .replace(/^_/, '')
    .toUpperCase()
}

function formatRawTs(iso: string): string {
  try {
    const d = new Date(iso)
    return d.toLocaleTimeString()
  } catch {
    return iso
  }
}

// ─── Remaining to goal (v1.1) ─────────────────────────────────────────────
//
// Dual-column view of the latest goal-check result from each agent, plus an
// agreement badge summarising whether both say the goal is met. We do not
// attempt fuzzy cross-matching between the two missing[] lists — PRD §6.6:
// the contract is "both agents must independently answer done=true for the
// session to transition to DONE", so showing each list as-is is both the
// most honest and the simplest possible surface. Once both are short (0–3
// items each) the user can eyeball the overlap.
function RemainingBlock(props: {
  claude: GoalCheckSnapshot | null
  codex: GoalCheckSnapshot | null
}) {
  const { claude, codex } = props
  const hasAny = claude != null || codex != null

  if (!hasAny) {
    return (
      <div className="remaining remaining-empty">
        <div className="rhead">
          <span className="lbl">▸ Remaining to goal</span>
          <span className="badge badge-waiting">○ waiting for first goal check</span>
        </div>
        <div className="rhint">
          Goal checks run at the end of each review cycle. First one usually
          lands a few minutes in.
        </div>
      </div>
    )
  }

  const badge = agreementBadge(claude, codex)

  return (
    <div className="remaining">
      <div className="rhead">
        <span className="lbl">▸ Remaining to goal</span>
        <span className={`badge ${badge.cls}`}>{badge.text}</span>
      </div>
      <div className="rcols">
        <RemainingColumn agent="claude" snap={claude} />
        <RemainingColumn agent="codex" snap={codex} />
      </div>
    </div>
  )
}

function RemainingColumn(props: {
  agent: 'claude' | 'codex'
  snap: GoalCheckSnapshot | null
}) {
  const { agent, snap } = props
  const label = agent === 'claude' ? 'Claude' : 'Codex'
  if (!snap) {
    return (
      <div className={`rcol rcol-${agent}`}>
        <div className="rcol-head">
          <span className="who">{label}</span>
          <span className="dim">not yet reported</span>
        </div>
      </div>
    )
  }
  const count = snap.missing.length
  return (
    <div className={`rcol rcol-${agent}`}>
      <div className="rcol-head">
        <span className="who">{label}</span>
        <span className={snap.done ? 'done' : 'notdone'}>
          {snap.done ? '✓ done' : `${count} missing`}
        </span>
        <span className="dim">round {snap.round} · {formatAge(snap.at)}</span>
      </div>
      {snap.done ? (
        <div className="rcol-rationale">
          {snap.rationale || 'reports goal met'}
        </div>
      ) : count === 0 ? (
        <div className="rcol-rationale dim">
          no specific items listed — {snap.rationale || 'no rationale given'}
        </div>
      ) : (
        <ol className="rcol-list">
          {snap.missing.slice(0, 8).map((m, i) => (
            <li key={i}>{m}</li>
          ))}
          {snap.missing.length > 8 && (
            <li className="dim">+ {snap.missing.length - 8} more…</li>
          )}
        </ol>
      )}
    </div>
  )
}

function agreementBadge(
  claude: GoalCheckSnapshot | null,
  codex: GoalCheckSnapshot | null,
): { cls: string; text: string } {
  if (!claude || !codex) {
    return { cls: 'badge-partial', text: '◐ waiting for both agents' }
  }
  if (claude.done && codex.done) {
    return { cls: 'badge-agree-done', text: '✓ both agree · goal met' }
  }
  if (!claude.done && !codex.done) {
    return { cls: 'badge-agree-work', text: '✖ both say not done' }
  }
  return { cls: 'badge-split', text: '◐ 1 agent done · 1 disagrees' }
}

function formatAge(iso: string): string {
  try {
    const ageMs = Date.now() - new Date(iso).getTime()
    const secs = Math.max(0, Math.floor(ageMs / 1000))
    if (secs < 60) return `${secs}s ago`
    const mins = Math.floor(secs / 60)
    if (mins < 60) return `${mins}m ago`
    const hrs = Math.floor(mins / 60)
    return `${hrs}h ${mins % 60}m ago`
  } catch {
    return iso
  }
}
