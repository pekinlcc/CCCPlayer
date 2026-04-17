import type { SessionState } from '../types'
import { Meter, PauseIcon, PlayIcon, Shell, StopIcon } from './Shell'

export function TerminalState(props: {
  state: SessionState
  workdir: string
  onBackToStart: () => void
  onRetrySameWorkdir: () => void
}) {
  const { state } = props
  const variant: 'done' | 'stopped' | 'errored' =
    state === 'DONE' ? 'done' : state === 'ABANDONED' ? 'stopped' : 'errored'
  const titleMap = { done: 'Done', stopped: 'Stopped', errored: 'Errored' } as const

  return (
    <Shell status={variant} statusLabel={titleMap[variant]}>
      <div className="display">
        <div className="dline">
          <span className="label">Track</span>
          <span className={`value ${variant === 'done' ? 'lime' : 'amber'}`}>
            session · {state.toLowerCase()}
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
          title="New session"
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

      <div className={`result-banner ${variant}`}>
        <h2>{titleMap[variant]}</h2>
        <p>
          {variant === 'done'
            ? 'Goal achieved. Both agents agreed the work is complete.'
            : variant === 'stopped'
            ? 'Session stopped. Artifacts and event log are preserved.'
            : 'Session ended before completion. Review the events log and artifacts below.'}
          <br />
          Artifacts: <code>{props.workdir}</code>
          <br />
          Event log: <code>.cccplayer/events.log</code>
        </p>
        <div className="result-actions">
          <button
            type="button"
            className="minibtn"
            onClick={props.onRetrySameWorkdir}
            style={{ borderColor: 'var(--lime)', color: 'var(--lime)' }}
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
        <span>v0.1.0</span>
      </div>
    </Shell>
  )
}
