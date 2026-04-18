import { useEffect, useMemo, useState } from 'react'
import { classifyWorkdir, pickFolder, runPreflight, startSession } from '../api'
import type { PreflightReport, SafetyVerdict } from '../types'
import { Meter, PauseIcon, PlayIcon, Shell, StopIcon } from './Shell'

const MAX_GOAL_LEN = 10_000

// Idle screen: folder + goal in one panel. Play button lights up when
// preflight is green, folder is not blocked, and goal passes validation.
export function Welcome(props: {
  initialWorkdir?: string
  onStart: (workdir: string, goal: string) => void
}) {
  const [preflight, setPreflight] = useState<PreflightReport | null>(null)
  const [workdir, setWorkdir] = useState(props.initialWorkdir ?? '')
  const [goal, setGoal] = useState('')
  const [safety, setSafety] = useState<SafetyVerdict | null>(null)
  const [starting, setStarting] = useState(false)

  useEffect(() => {
    void runPreflight().then(setPreflight)
  }, [])

  // Re-classify workdir on debounce so the safety banner updates as the
  // user types / pastes a path.
  useEffect(() => {
    const path = workdir.trim()
    if (!path) {
      setSafety(null)
      return
    }
    const t = setTimeout(() => {
      void classifyWorkdir(path).then((r) => setSafety(r.verdict))
    }, 300)
    return () => clearTimeout(t)
  }, [workdir])

  const claudeOk = preflight?.claude?.supports_auto_approve === true
  const codexOk = preflight?.codex?.supports_auto_approve === true
  const preflightGreen = Boolean(preflight?.all_ok)
  const safetyOk = !safety || safety.level === 'ok' || safety.level === 'soft_warn'
  const goalCheck = useMemo(() => validateGoal(goal), [goal])
  const canPlay =
    workdir.trim().length > 0 &&
    preflightGreen &&
    safetyOk &&
    goalCheck.ok &&
    !starting

  async function handlePlay() {
    if (!canPlay) return
    setStarting(true)
    try {
      await startSession(goal, workdir.trim())
      props.onStart(workdir.trim(), goal)
    } finally {
      setStarting(false)
    }
  }

  return (
    <Shell status="idle" statusLabel={preflight ? 'No session' : 'Preflight…'}>
      <div className="display">
        <div className="dline">
          <span className="label">Track</span>
          <span className="value magenta">
            {goal.trim() ? goal.split('\n')[0].slice(0, 60) : '— · — · —'}
          </span>
        </div>

        <div className="dline">
          <span className="label">Folder</span>
          <input
            type="text"
            className="pathfield"
            placeholder="/Users/you/dev/my-project"
            value={workdir}
            onChange={(e) => setWorkdir(e.target.value)}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
          />
          <button
            type="button"
            className="browse-btn"
            title="Pick a local folder"
            onClick={async () => {
              const picked = await pickFolder(workdir.trim() || undefined)
              if (picked) setWorkdir(picked)
            }}
          >
            Browse…
          </button>
        </div>

        <div className="goal-box">
          <span className="prompt">&gt;</span>
          <textarea
            value={goal}
            onChange={(e) => setGoal(e.target.value)}
            placeholder="Describe what to build / change / fix. One or two paragraphs is plenty."
            spellCheck={false}
            rows={5}
          />
        </div>
        <div className="goal-meta">
          <span>{goal.length.toLocaleString()} / {MAX_GOAL_LEN.toLocaleString()}</span>
          {!goalCheck.ok && goal.length > 0 && (
            <span className="warn">· {goalCheck.reason}</span>
          )}
        </div>

        <ul className="preflight-list">
          <PreflightRow
            label="Claude Code CLI"
            ok={claudeOk}
            detail={
              preflight?.claude
                ? preflight.claude.version_line +
                  (preflight.claude.auto_approve_flag
                    ? ` · flag: ${preflight.claude.auto_approve_flag}`
                    : '')
                : preflight
                ? 'Not on PATH. Install Claude Code CLI.'
                : 'probing…'
            }
          />
          <PreflightRow
            label="Codex CLI"
            ok={codexOk}
            detail={
              preflight?.codex
                ? preflight.codex.version_line +
                  (preflight.codex.auto_approve_flag
                    ? ` · flag: ${preflight.codex.auto_approve_flag}`
                    : '')
                : preflight
                ? 'Not on PATH. Install Codex CLI.'
                : 'probing…'
            }
          />
        </ul>

        {safety && <SafetyNotice verdict={safety} />}
      </div>

      <div className="transport">
        <button
          type="button"
          className="tbtn play big"
          title={
            !preflightGreen
              ? 'Preflight not green — fix CLI issues above'
              : !safetyOk
              ? 'Workspace is blocked'
              : !goalCheck.ok
              ? 'Goal is empty or invalid'
              : 'Start session'
          }
          onClick={handlePlay}
          disabled={!canPlay}
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
        <span className={`pill ${preflightGreen ? 'ok' : 'danger'}`}>
          Preflight {preflightGreen ? '✓' : '×'}
        </span>
        <span className={`pill ${safetyOk ? 'ok' : 'danger'}`}>
          Workspace {safetyOk ? '✓' : '×'}
        </span>
        <span className="spacer" />
        <div className="meters">
          <Meter label="CLAUDE" active={false} />
          <Meter label="CODEX" active={false} />
        </div>
      </div>

      <div className="shell-foot">
        <span>Workdir</span>
        <code>{workdir.trim() || '—'}</code>
        <span className="spacer" />
        <span>v1.3.0</span>
      </div>
    </Shell>
  )
}

function PreflightRow(props: { label: string; ok: boolean; detail: string }) {
  return (
    <li className={`preflight-row ${props.ok ? 'ok' : 'bad'}`}>
      <span className="dot">●</span>
      <span className="label">{props.label}</span>
      <span className="detail">{props.detail}</span>
    </li>
  )
}

function SafetyNotice(props: { verdict: SafetyVerdict }) {
  switch (props.verdict.level) {
    case 'ok':
      return null // green state is already implicit — no need to shout
    case 'soft_warn':
      return (
        <div className="safety soft-warn">
          Heads up: {props.verdict.reasons.join(' · ')}
        </div>
      )
    case 'strong_warn':
      return (
        <div className="safety strong-warn">
          ⚠ Confirm: {props.verdict.reason}
        </div>
      )
    case 'blocked':
      return (
        <div className="safety blocked">
          Refused: {props.verdict.reason}
        </div>
      )
  }
}

function validateGoal(text: string): { ok: true } | { ok: false; reason: string } {
  const trimmed = text.trim()
  if (!trimmed) return { ok: false, reason: 'goal is empty' }
  if (text.length > MAX_GOAL_LEN)
    return { ok: false, reason: `goal longer than ${MAX_GOAL_LEN} chars` }
  if (/^[\s\p{P}\p{S}]+$/u.test(trimmed))
    return { ok: false, reason: 'goal has no words' }
  return { ok: true }
}
