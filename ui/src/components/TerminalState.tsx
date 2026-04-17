import type { SessionState } from '../types'

export function TerminalState(props: { state: SessionState; workdir: string }) {
  const isGood = props.state === 'DONE'
  return (
    <section className="terminal">
      <div className={`result ${isGood ? 'ok' : 'bad'}`}>
        <h2>{props.state}</h2>
        {isGood ? (
          <p>Goal achieved. Both agents agreed the work is complete.</p>
        ) : (
          <p>
            Session ended before completion. Review the artifacts under{' '}
            <code>{props.workdir}</code> and the events log under{' '}
            <code>.cccplayer/events.log</code>.
          </p>
        )}
      </div>
    </section>
  )
}
