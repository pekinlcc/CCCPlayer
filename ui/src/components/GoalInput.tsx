import { useMemo, useState } from 'react'
import type { PreflightReport } from '../types'
import { startSession } from '../api'

// Implements PRD §6.5 目标输入校验.
const MAX_GOAL_LEN = 10_000

export function GoalInput(props: {
  workdir: string
  preflight: PreflightReport
  onStart: (goal: string) => void
}) {
  const [goal, setGoal] = useState('')
  const valid = useMemo(() => validateGoal(goal), [goal])

  async function handleStart() {
    await startSession(goal, props.workdir)
    props.onStart(goal)
  }

  const preflightOk = props.preflight.all_ok
  const canStart = valid.ok && preflightOk

  return (
    <section className="goal-input">
      <div className="card">
        <h2>Goal</h2>
        <p className="hint">
          Describe what you want built, changed, or fixed in{' '}
          <code>{props.workdir}</code>. One or two paragraphs is usually
          enough. The text becomes <code>GOAL.md</code> — it's immutable for
          the rest of the session.
        </p>
        <textarea
          value={goal}
          onChange={(e) => setGoal(e.target.value)}
          rows={8}
          placeholder="e.g. Add CSV import to the existing app and write integration tests."
          spellCheck={false}
        />
        <div className="goal-meta">
          <span className={goal.length > MAX_GOAL_LEN ? 'error' : 'hint'}>
            {goal.length} / {MAX_GOAL_LEN}
          </span>
          {!valid.ok && <span className="error">· {valid.reason}</span>}
          {!preflightOk && (
            <span className="error">
              · preflight not green — resolve Welcome page issues first
            </span>
          )}
        </div>
        <div className="row">
          <button
            type="button"
            className="primary"
            disabled={!canStart}
            onClick={handleStart}
          >
            Start
          </button>
        </div>
      </div>
    </section>
  )
}

function validateGoal(text: string): { ok: true } | { ok: false; reason: string } {
  const trimmed = text.trim()
  if (!trimmed) return { ok: false, reason: 'goal is empty' }
  if (text.length > MAX_GOAL_LEN)
    return { ok: false, reason: `goal is longer than ${MAX_GOAL_LEN} chars` }
  // Reject strings that are all punctuation / whitespace / markdown dashes.
  if (/^[\s\p{P}\p{S}]+$/u.test(trimmed))
    return { ok: false, reason: 'goal has no words' }
  return { ok: true }
}
