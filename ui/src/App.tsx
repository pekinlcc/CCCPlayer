import { useState } from 'react'
import { Welcome } from './components/Welcome'
import { RunningView } from './components/RunningView'
import { TerminalState } from './components/TerminalState'
import type { SessionState, SessionSummary } from './types'

type Screen =
  | { kind: 'welcome'; initialWorkdir?: string }
  | { kind: 'running'; workdir: string; goal: string }
  | {
      kind: 'terminal'
      state: SessionState
      workdir: string
      goal: string
      summary: SessionSummary | null
    }

export function App() {
  const [screen, setScreen] = useState<Screen>({ kind: 'welcome' })

  return (
    <div className="app-shell">
      <main className="app-main">
        {screen.kind === 'welcome' && (
          <Welcome
            initialWorkdir={screen.initialWorkdir}
            onStart={(workdir, goal) =>
              setScreen({ kind: 'running', workdir, goal })
            }
          />
        )}
        {screen.kind === 'running' && (
          <RunningView
            workdir={screen.workdir}
            goal={screen.goal}
            onDone={(finalState, summary) =>
              setScreen({
                kind: 'terminal',
                state: finalState,
                workdir: screen.workdir,
                goal: screen.goal,
                summary,
              })
            }
          />
        )}
        {screen.kind === 'terminal' && (
          <TerminalState
            state={screen.state}
            workdir={screen.workdir}
            goal={screen.goal}
            summary={screen.summary}
            onBackToStart={() => setScreen({ kind: 'welcome' })}
            onRetrySameWorkdir={() =>
              setScreen({ kind: 'welcome', initialWorkdir: screen.workdir })
            }
          />
        )}
      </main>
    </div>
  )
}
