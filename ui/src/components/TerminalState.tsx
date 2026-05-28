import { revealInFinder } from '../api'
import type { Event, SessionState, SessionSummary } from '../types'
import { Meter, PauseIcon, PlayIcon, Shell, StopIcon } from './Shell'

/**
 * End-of-session execution report (v1.5.0+).
 *
 * Replaces the previous minimal terminal screen with a structured
 * recap: duration · rounds · tokens · per-agent final goal-check
 * disagreement · chronological highlights · artifacts · actions.
 *
 * Data comes from the SessionSummary handed over by RunningView. If
 * `summary` is null (legacy path / direct navigation), the screen
 * degrades gracefully to the old plain banner.
 */
export function TerminalState(props: {
  state: SessionState
  workdir: string
  goal: string
  summary: SessionSummary | null
  onBackToStart: () => void
  onRetrySameWorkdir: () => void
}) {
  const { state, summary } = props
  const variant: 'done' | 'stopped' | 'errored' =
    state === 'DONE' ? 'done' : state === 'ABANDONED' ? 'stopped' : 'errored'
  const titleMap = { done: 'Done', stopped: 'Stopped', errored: 'Errored' } as const

  return (
    <Shell status={variant} statusLabel={titleMap[variant]}>
      <div className="display">
        <div className="dline">
          <span className="label">Track</span>
          <span
            className={`value ${variant === 'done' ? 'lime' : 'amber'} track-value`}
            title={props.goal}
          >
            {props.goal.trim() || `session · ${state.toLowerCase()}`}
          </span>
        </div>
        <div className="dline">
          <span className="label">Folder</span>
          <span className="pathfield" style={{ flex: 1 }}>{props.workdir}</span>
        </div>
      </div>

      <div className="transport">
        <button
          type="button"
          className="tbtn play big"
          title="New session (same folder)"
          onClick={props.onRetrySameWorkdir}
        >
          <PlayIcon />
        </button>
        <button type="button" className="tbtn" disabled title="Pause">
          <PauseIcon />
        </button>
        <button type="button" className="tbtn stop" disabled title="Stop">
          <StopIcon />
        </button>
        <span className="sep" />
        <span className={`pill ${variant === 'done' ? 'ok' : variant === 'errored' ? 'danger' : 'warn'}`}>
          {state}
        </span>
        <span className="spacer" />
        <div className="meters">
          <Meter label="CLAUDE" active={false} />
          <Meter label="CODEX" active={false} />
        </div>
      </div>

      <div className={`session-report ${variant}`}>
        <div className="report-head">
          <span className="lbl">▼ Session report</span>
          <span className="state">{titleMap[variant].toUpperCase()}</span>
          {summary?.reason && (
            <span className="reason">
              <span className="dim">reason:</span> {summary.reason}
            </span>
          )}
        </div>

        {summary ? (
          <>
            <div className="kv-grid">
              <div className="kv">
                <span className="label">Duration</span>
                <span className="big">{formatElapsed(summary.elapsedSecs)}</span>
              </div>
              <div className="kv">
                <span className="label">Rounds</span>
                <span className="big">{summary.round}</span>
              </div>
              <div className="kv tokens-kv" style={{ gridColumn: 'span 2' }}>
                <span className="label">Tokens</span>
                <div className="tokens-split">
                  <div className="pair">
                    <span className="who claude">Claude</span>
                    <span className="num">{summary.claudeTokens.toLocaleString()}</span>
                  </div>
                  <div className="pair">
                    <span className="who codex">Codex</span>
                    <span className="num">{summary.codexTokens.toLocaleString()}</span>
                  </div>
                </div>
              </div>
            </div>

            <div className="final-gc">
              <div className="final-gc-head">
                <span className="lbl">Final goal check</span>
                <span className="dim">
                  {agreementLabel(summary)}
                </span>
              </div>
              <div className="final-gc-cols">
                <FinalColumn agent="claude" snap={summary.claudeGc} />
                <FinalColumn agent="codex" snap={summary.codexGc} />
              </div>
            </div>

            <div className="highlights">
              <div className="highlights-head">
                <span className="lbl">Highlights</span>
                <span className="dim">{summary.events.length} events</span>
              </div>
              <HighlightList events={summary.events} />
            </div>
          </>
        ) : (
          <p className="hint">
            No session summary available for this terminal state (happens when
            you navigate here directly). Review the event log under{' '}
            <code>.cccplayer/events.log</code>.
          </p>
        )}

        <div className="artifacts">
          <div className="artifacts-head">
            <span className="lbl">Artifacts</span>
            <span className="dim">click to reveal in Finder</span>
          </div>
          <ul>
            <ArtifactRow label="workspace" path={props.workdir} />
            <ArtifactRow
              label="event log"
              path={`${props.workdir}/.cccplayer/events.log`}
            />
            <ArtifactRow
              label="transcripts"
              path={`${props.workdir}/.cccplayer/transcripts/`}
            />
            <ArtifactRow
              label="snapshots"
              path={`${props.workdir}/.cccplayer/snapshots/`}
            />
          </ul>
        </div>

        <div className="result-actions">
          <button
            type="button"
            className="minibtn primary"
            onClick={props.onRetrySameWorkdir}
          >
            New session (same folder)
          </button>
          <button type="button" className="minibtn" onClick={props.onBackToStart}>
            ← Back to start
          </button>
        </div>
      </div>

      <div className="shell-foot">
        <span>Workdir</span>
        <code>{props.workdir}</code>
        <span className="spacer" />
        <span>v1.7.5</span>
      </div>
    </Shell>
  )
}

// ─── Helpers ──────────────────────────────────────────────────────────────

/**
 * v1.7.5 AUDIT.md #7: one row of the Artifacts list, with a click handler
 * that reveals the path in Finder via the Rust `reveal_in_finder` command.
 * Rendered as a button (not an <a>) because there's no URL semantics and
 * keyboard/AT users get button behavior for free.
 */
function ArtifactRow(props: { label: string; path: string }) {
  return (
    <li>
      <span className="art-label">{props.label}:</span>{' '}
      <button
        type="button"
        className="art-path"
        title={`Reveal ${props.path} in Finder`}
        onClick={() => {
          void revealInFinder(props.path)
        }}
      >
        <code>{props.path}</code>
      </button>
    </li>
  )
}

function FinalColumn(props: {
  agent: 'claude' | 'codex'
  snap: SessionSummary['claudeGc']
}) {
  const { agent, snap } = props
  const label = agent === 'claude' ? 'Claude' : 'Codex'
  if (!snap) {
    return (
      <div className={`rcol rcol-${agent}`}>
        <div className="rcol-head">
          <span className="who">{label}</span>
          <span className="dim">no goal check ran</span>
        </div>
      </div>
    )
  }
  return (
    <div className={`rcol rcol-${agent}`}>
      <div className="rcol-head">
        <span className="who">{label}</span>
        <span className={snap.done ? 'done' : 'notdone'}>
          {snap.done
            ? '✓ done'
            : `${snap.missing.length} missing`}
        </span>
        <span className="dim">round {snap.round}</span>
      </div>
      {snap.rationale && (
        <div className="rcol-rationale">{snap.rationale}</div>
      )}
      {snap.missing.length > 0 && (
        <>
          <div className="rcol-sub">Missing:</div>
          <ol className="rcol-list">
            {snap.missing.map((m, i) => (
              <li key={i}>{m}</li>
            ))}
          </ol>
        </>
      )}
      {snap.shelved.length > 0 && (
        <>
          <div className="rcol-sub">Shelved:</div>
          <ol className="rcol-list rcol-shelved">
            {snap.shelved.map((m, i) => (
              <li key={i}>{m}</li>
            ))}
          </ol>
        </>
      )}
    </div>
  )
}

function agreementLabel(s: SessionSummary): string {
  const c = s.claudeGc
  const x = s.codexGc
  if (!c && !x) return 'no goal check ran'
  if (c && x && c.done && x.done) return '✓ both agents agreed goal is met'
  if (c && x && !c.done && !x.done) return '✖ both agents flagged missing items'
  if (c && x) return '◐ disagreed — one done, one not'
  return '◐ only one agent reported'
}

function HighlightList(props: { events: Event[] }) {
  const highlights = extractHighlights(props.events)
  if (highlights.length === 0) {
    return <p className="hint">No notable events recorded.</p>
  }
  return (
    <ol className="highlights-list">
      {highlights.map((h, i) => (
        <li key={i} className={`highlight ${h.severity}`}>
          <span className="ts">{formatTs(h.at)}</span>
          <span className="round">r{h.round}</span>
          <span className="kind">{h.kind}</span>
          <span className="detail">{h.detail}</span>
        </li>
      ))}
    </ol>
  )
}

interface Highlight {
  at: string
  round: number
  kind: string
  detail: string
  severity: 'ok' | 'warn' | 'fail' | 'info'
}

// Curate a short timeline from the raw event stream: state transitions,
// review verdicts, notes (reasons), goal-check verdicts, rate-limit /
// stagnation events. Heartbeats and agent_started/finished for routine
// OK turns are omitted to keep the list glanceable.
function extractHighlights(events: Event[]): Highlight[] {
  const out: Highlight[] = []
  for (const ev of events) {
    const k = String(ev.kind)
    const round = typeof ev.round === 'number' ? ev.round : 0
    const at = String(ev.at)
    if (k === 'session_created') {
      out.push({
        at, round, kind: 'session_created',
        detail: String((ev as { goal_preview?: string }).goal_preview ?? '').slice(0, 80),
        severity: 'info',
      })
    } else if (k === 'review_written') {
      const verdict = String((ev as { verdict?: string }).verdict ?? '')
      const blocking = (ev as { blocking_count?: number }).blocking_count ?? 0
      const version = (ev as { version?: number }).version ?? 0
      out.push({
        at, round,
        kind: 'review_written',
        detail: `v${version} — ${verdict}${blocking ? ` (${blocking} blocking)` : ''}`,
        severity: verdict === 'approved' ? 'ok'
          : verdict === 'blocked' ? 'fail'
          : 'warn',
      })
    } else if (k === 'goal_check') {
      const agent = String((ev as { agent?: string }).agent ?? '')
      const done = Boolean((ev as { done?: boolean }).done)
      const missing = (ev as { missing_count?: number }).missing_count ?? 0
      const shelved = ((ev as { shelved?: string[] }).shelved ?? []).length
      out.push({
        at, round,
        kind: `goal_check ${agent}`,
        detail: done
          ? `✓ done${shelved ? ` · ${shelved} shelved` : ''}`
          : `✖ ${missing} missing${shelved ? ` · ${shelved} shelved` : ''}`,
        severity: done ? 'ok' : 'warn',
      })
    } else if (k === 'note') {
      const msg = String((ev as { message?: string }).message ?? '')
      out.push({
        at, round, kind: 'note',
        detail: msg.length > 120 ? msg.slice(0, 117) + '…' : msg,
        severity: /stagnat|rate.?limit|error|fail/i.test(msg) ? 'fail' : 'info',
      })
    } else if (k === 'error') {
      out.push({
        at, round, kind: 'error',
        detail: `[${(ev as { code?: string }).code ?? ''}] ${(ev as { message?: string }).message ?? ''}`,
        severity: 'fail',
      })
    } else if (k === 'agent_finished') {
      const outcome = String((ev as { outcome?: string }).outcome ?? '')
      if (outcome !== 'ok') {
        const agent = String((ev as { agent?: string }).agent ?? '')
        const phase = String((ev as { phase?: string }).phase ?? '')
        out.push({
          at, round, kind: 'agent_finished',
          detail: `${agent} ${phase} — ${outcome}`,
          severity:
            outcome === 'auth_failed' || outcome === 'crashed' || outcome === 'stalled' ? 'fail'
            : outcome === 'rate_limited' ? 'warn'
            : 'warn',
        })
      }
    } else if (k === 'state_changed') {
      const to = String((ev as { to?: string }).to ?? '')
      const [st] = to.split('/')
      // Only flag the "interesting" transitions — entering Errored,
      // Paused, or Done is noteworthy; intra-round cycling isn't.
      if (/^(Errored|Paused|Done|Abandoned)$/i.test(st)) {
        out.push({
          at, round, kind: 'state_changed',
          detail: to,
          severity: st.toLowerCase() === 'done' ? 'ok'
            : st.toLowerCase() === 'errored' ? 'fail'
            : 'warn',
        })
      }
    }
  }
  return out
}

function formatElapsed(secs: number): string {
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60
  if (h > 0) return `${h}h ${m}m ${s}s`
  if (m > 0) return `${m}m ${s}s`
  return `${s}s`
}

function formatTs(iso: string): string {
  try {
    return new Date(iso).toLocaleTimeString()
  } catch {
    return iso
  }
}
