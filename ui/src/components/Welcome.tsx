import { useEffect, useMemo, useRef, useState } from 'react'
import {
  classifyWorkdir,
  GoalConflictError,
  pickFolder,
  readGoalMd,
  runPreflight,
  startSession,
} from '../api'
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
  const [goalConflict, setGoalConflict] = useState<{ existing: string } | null>(null)
  /**
   * v1.6.1: when the workdir has an existing `GOAL.md`, we pre-populate the
   * goal textarea with its content. `autoLoadedGoal` remembers what we
   * loaded so we can tell whether the current `goal` is "still verbatim
   * what we loaded" (safe to replace on a new workdir pick) vs. "the user
   * has typed on top of it" (leave it alone — the Play-time conflict
   * modal will catch the mismatch).
   */
  const [autoLoadedGoal, setAutoLoadedGoal] = useState<string | null>(null)
  // Stale-closure guards — the workdir-driven effect reads current goal /
  // autoLoadedGoal without re-running when they change.
  const goalRef = useRef(goal)
  const autoLoadedRef = useRef(autoLoadedGoal)

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

  // Sync refs so the workdir-change effect reads current values without
  // being in its dep list (we don't want it to re-fire on every keystroke).
  useEffect(() => {
    goalRef.current = goal
  }, [goal])
  useEffect(() => {
    autoLoadedRef.current = autoLoadedGoal
  }, [autoLoadedGoal])

  // v1.6.1 auto-load: when the user picks / types a workdir whose
  // `GOAL.md` already exists, pre-populate the textarea with its content
  // so they can see the current goal and either keep it verbatim (reuse)
  // or edit on top of it (Play-time modal will then surface the mismatch).
  // Preserves an in-progress user edit across workdir switches — we only
  // replace the textarea content when it's empty or still matches what
  // we previously auto-loaded (i.e. the user hasn't diverged).
  useEffect(() => {
    const path = workdir.trim()
    if (!path) {
      setAutoLoadedGoal(null)
      return
    }
    let cancelled = false
    const t = setTimeout(() => {
      void readGoalMd(path).then((content) => {
        if (cancelled) return
        const curGoal = goalRef.current
        const curAuto = autoLoadedRef.current
        if (content == null) {
          // No GOAL.md: clear only if current content was auto-loaded.
          if (curAuto !== null && curGoal === curAuto) setGoal('')
          setAutoLoadedGoal(null)
          return
        }
        // GOAL.md exists: replace only if textarea is empty or current
        // content is verbatim what we previously auto-loaded.
        if (curGoal.trim() === '' || curGoal === curAuto) {
          setGoal(content)
          setAutoLoadedGoal(content)
        }
      })
    }, 300)
    return () => {
      cancelled = true
      clearTimeout(t)
    }
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
    } catch (e) {
      if (e instanceof GoalConflictError) {
        // Surface the three-way modal and leave `starting` reset so the user
        // isn't locked out of the UI while they decide.
        setGoalConflict({ existing: e.existing })
        return
      }
      throw e
    } finally {
      setStarting(false)
    }
  }

  async function resolveConflict(choice: 'use-existing' | 'overwrite' | 'cancel') {
    if (!goalConflict) return
    const existing = goalConflict.existing
    setGoalConflict(null)
    if (choice === 'cancel') return
    setStarting(true)
    try {
      if (choice === 'use-existing') {
        // Adopt the existing GOAL.md content as the session goal. Backend
        // will see matching content and skip the rewrite.
        setGoal(existing)
        await startSession(existing, workdir.trim())
        props.onStart(workdir.trim(), existing)
      } else {
        await startSession(goal, workdir.trim(), { overwriteGoal: true })
        props.onStart(workdir.trim(), goal)
      }
    } finally {
      setStarting(false)
    }
  }

  return (
    <Shell status="idle" statusLabel={preflight ? 'No session' : 'Preflight…'}>
      <div className="display">
        <div className="dline">
          <span className="label">Track</span>
          <span className="value magenta track-value" title={goal}>
            {goal.trim() || '— · — · —'}
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
          {autoLoadedGoal !== null && goal === autoLoadedGoal && goal.length > 0 && (
            <span className="loaded">
              · loaded existing GOAL.md — keep to reuse, edit to overwrite
            </span>
          )}
          {autoLoadedGoal !== null && goal !== autoLoadedGoal && goal.length > 0 && (
            <span className="diverged">
              · diverged from existing GOAL.md — Play will confirm overwrite
            </span>
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
        <span>v1.6.1</span>
      </div>
      {goalConflict && (
        <GoalConflictModal
          existing={goalConflict.existing}
          proposed={goal}
          onPick={resolveConflict}
        />
      )}
    </Shell>
  )
}

function GoalConflictModal(props: {
  existing: string
  proposed: string
  onPick: (choice: 'use-existing' | 'overwrite' | 'cancel') => void
}) {
  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true">
      <div className="modal goal-conflict">
        <div className="modal-title">GOAL.md already exists</div>
        <div className="modal-body">
          <p>
            The workspace already has a <code>GOAL.md</code>. Pick one to continue.
            The chosen goal becomes the session's immutable benchmark.
          </p>
          <div className="goal-diff">
            <div className="goal-col">
              <div className="goal-col-title">Existing GOAL.md</div>
              <pre className="goal-preview">{props.existing.trim() || '(empty)'}</pre>
            </div>
            <div className="goal-col">
              <div className="goal-col-title">Your new goal</div>
              <pre className="goal-preview">{props.proposed.trim() || '(empty)'}</pre>
            </div>
          </div>
        </div>
        <div className="modal-actions">
          <button
            type="button"
            className="btn secondary"
            onClick={() => props.onPick('cancel')}
          >
            Cancel
          </button>
          <button
            type="button"
            className="btn"
            onClick={() => props.onPick('use-existing')}
          >
            Use existing
          </button>
          <button
            type="button"
            className="btn danger"
            onClick={() => props.onPick('overwrite')}
          >
            Overwrite with new
          </button>
        </div>
      </div>
    </div>
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
