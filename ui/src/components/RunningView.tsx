import { useEffect, useMemo, useRef, useState } from 'react'
import { pauseSession, stopSession, subscribeEvents, subscribeRawLog } from '../api'
import type { Event, Phase, RawLogLine, SessionState } from '../types'
import { Meter, PauseIcon, PlayIcon, Shell, StopIcon } from './Shell'

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
          const to = String((ev as { to?: string }).to ?? '')
          const match = /^([A-Z]+)\/([A-Z_]+)$/.exec(to)
          if (match) {
            const nextPhase = match[2] as Phase
            setPhase(nextPhase)
            if (to.startsWith('Done')) props.onDone('DONE')
            if (to.startsWith('Abandoned')) props.onDone('ABANDONED')
            if (to.startsWith('Errored')) props.onDone('ERRORED')
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

  const sinceLastActivity = useMemo(() => {
    void nowTick
    return Math.max(0, Math.floor((Date.now() - lastActivityAt) / 1000))
  }, [nowTick, lastActivityAt])

  const phaseIdx = PHASE_ORDER.indexOf(phase)

  const trackLabel = props.goal.split('\n')[0].slice(0, 60) || 'session'

  return (
    <Shell status="running" statusLabel="Running">
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
        <button type="button" className="tbtn play big" disabled title="Running">
          <PlayIcon />
        </button>
        <button
          type="button"
          className="tbtn"
          title="Pause"
          onClick={async () => {
            setPausing(true)
            try { await pauseSession() } finally { setPausing(false) }
          }}
          disabled={pausing}
        >
          <PauseIcon />
        </button>
        <button
          type="button"
          className="tbtn stop"
          title="Stop"
          onClick={async () => {
            setStopping(true)
            try {
              await stopSession()
              props.onDone('ABANDONED')
            } finally {
              setStopping(false)
            }
          }}
          disabled={stopping}
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

        <div className="kv-grid">
          <KV label="Elapsed" big={formatElapsed(elapsed)} />
          <KV
            label="Tokens"
            big={`${claudeTokens.toLocaleString()} + ${codexTokens.toLocaleString()}`}
            color="lime"
          />
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

function formatRawTs(iso: string): string {
  try {
    const d = new Date(iso)
    return d.toLocaleTimeString()
  } catch {
    return iso
  }
}
