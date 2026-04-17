import { useEffect, useState } from 'react'
import { pauseSession, stopSession } from '../api'
import type { Event, Phase, SessionState } from '../types'

// Implements PRD §6.1 three-zone progress display: status band, event
// timeline, raw log drawer.
export function RunningView(props: {
  workdir: string
  goal: string
  onDone: (state: SessionState) => void
}) {
  const [phase, setPhase] = useState<Phase>('PLANNING')
  const [round, setRound] = useState(1)
  const [events, setEvents] = useState<Event[]>([])
  const [elapsed, setElapsed] = useState(0)
  const [showRaw, setShowRaw] = useState(false)
  const [claudeTokens, setClaudeTokens] = useState(0)
  const [codexTokens, setCodexTokens] = useState(0)

  useEffect(() => {
    const t = setInterval(() => setElapsed((e) => e + 1), 1000)
    return () => clearInterval(t)
  }, [])

  // In M1 the event subscription is not yet wired through Tauri; the
  // orchestrator pushes events to a Rust-side channel which the app layer
  // will forward via `emit`/`listen` once the app_state plumbing lands.
  // Until then this view renders its initial scaffolding — the Rust side is
  // fully functional and can be exercised via the integration test.
  useEffect(() => {
    // placeholder: in a real invocation we'd `listen('event', …)` here.
    void setEvents
    void setPhase
    void setRound
    void setClaudeTokens
    void setCodexTokens
  }, [])

  return (
    <section className="running">
      <div className="status-band">
        <div className="status-item">
          <label>Phase</label>
          <span className={`phase-${phase}`}>{phase}</span>
        </div>
        <div className="status-item">
          <label>Round</label>
          <span>{round}</span>
        </div>
        <div className="status-item">
          <label>Elapsed</label>
          <span>{formatElapsed(elapsed)}</span>
        </div>
        <div className="status-item">
          <label>Tokens</label>
          <span>
            {claudeTokens.toLocaleString()} + {codexTokens.toLocaleString()}
          </span>
        </div>
        <div className="spacer" />
        <button type="button" onClick={() => void pauseSession()}>
          Pause
        </button>
        <button
          type="button"
          className="danger"
          onClick={async () => {
            await stopSession()
            props.onDone('ABANDONED')
          }}
        >
          Stop
        </button>
      </div>

      <div className="main-area">
        <div className="timeline">
          <h3>Timeline</h3>
          {events.length === 0 ? (
            <p className="hint">
              Waiting for first event. Planning usually starts within a few
              seconds.
            </p>
          ) : (
            <ul>
              {events.map((ev, i) => (
                <li key={i}>
                  <span className="ts">{new Date(ev.at).toLocaleTimeString()}</span>
                  <span className="kind">{String(ev.kind)}</span>
                </li>
              ))}
            </ul>
          )}
        </div>

        <aside className={`raw-drawer ${showRaw ? 'open' : ''}`}>
          <button type="button" onClick={() => setShowRaw((v) => !v)}>
            {showRaw ? '▸ Hide raw log' : '◂ Show raw log'}
          </button>
          {showRaw && (
            <pre className="raw-log">
              (raw CLI output streams here — currently disabled in this build)
            </pre>
          )}
        </aside>
      </div>

      <footer className="workdir-footer">
        <code>{props.workdir}</code>
      </footer>
    </section>
  )
}

function formatElapsed(secs: number): string {
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60
  if (h > 0) return `${h}h ${m}m`
  if (m > 0) return `${m}m ${s}s`
  return `${s}s`
}
