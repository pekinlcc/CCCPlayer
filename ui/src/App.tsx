import { useEffect, useState } from 'react'
import { Welcome } from './components/Welcome'
import { GoalInput } from './components/GoalInput'
import { RunningView } from './components/RunningView'
import { TerminalState } from './components/TerminalState'
import { classifyWorkdir, runPreflight } from './api'
import type { PreflightReport, SafetyVerdict, SessionState } from './types'

type Screen =
  | { kind: 'welcome' }
  | { kind: 'goal'; workdir: string; preflight: PreflightReport }
  | {
      kind: 'running'
      workdir: string
      goal: string
    }
  | { kind: 'terminal'; state: SessionState; workdir: string }

export function App() {
  const [screen, setScreen] = useState<Screen>({ kind: 'welcome' })
  const [preflight, setPreflight] = useState<PreflightReport | null>(null)
  const [workdir, setWorkdir] = useState<string>('')
  const [safety, setSafety] = useState<SafetyVerdict | null>(null)

  useEffect(() => {
    void runPreflight().then(setPreflight)
  }, [])

  async function onChooseWorkdir(path: string) {
    setWorkdir(path)
    const resp = await classifyWorkdir(path)
    setSafety(resp.verdict)
    if (resp.verdict.level === 'blocked') return
    // Only advance if preflight is fully green. Welcome's "Use this folder"
    // button is already disabled in that case, but we guard here too so the
    // invariant holds even when tests or future code paths call this
    // directly.
    if (!preflight?.all_ok) return
    setScreen({ kind: 'goal', workdir: path, preflight })
  }

  function onStart(goal: string) {
    if (screen.kind !== 'goal') return
    setScreen({ kind: 'running', workdir: screen.workdir, goal })
  }

  function onComplete(finalState: SessionState) {
    if (screen.kind !== 'running') return
    setScreen({ kind: 'terminal', state: finalState, workdir: screen.workdir })
  }

  return (
    <div className="app-shell">
      <header className="app-header">
        <div className="brand">CCCPlayer</div>
        <div className="subtitle">Claude Code × Codex — burn compute, not your time.</div>
      </header>
      <main className="app-main">
        {screen.kind === 'welcome' && (
          <Welcome
            preflight={preflight}
            workdir={workdir}
            safety={safety}
            onChooseWorkdir={onChooseWorkdir}
          />
        )}
        {screen.kind === 'goal' && (
          <GoalInput
            workdir={screen.workdir}
            preflight={screen.preflight}
            onStart={onStart}
            onBack={() => setScreen({ kind: 'welcome' })}
          />
        )}
        {screen.kind === 'running' && (
          <RunningView
            workdir={screen.workdir}
            goal={screen.goal}
            onDone={onComplete}
          />
        )}
        {screen.kind === 'terminal' && (
          <TerminalState state={screen.state} workdir={screen.workdir} />
        )}
      </main>
    </div>
  )
}
