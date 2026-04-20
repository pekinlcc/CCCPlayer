// IPC wrappers. When running inside Tauri, uses @tauri-apps/api/core.
// When running in a plain browser (vite dev without Tauri), falls back to a
// stub so developers can iterate on the UI without the native shell.

import type {
  ClassifyResp,
  PreflightReport,
  SafetyVerdict,
} from './types'

type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>

async function invoke(): Promise<InvokeFn> {
  try {
    const mod = await import('@tauri-apps/api/core')
    return mod.invoke as InvokeFn
  } catch {
    return stubInvoke as InvokeFn
  }
}

async function stubInvoke<T>(cmd: string, _args?: Record<string, unknown>): Promise<T> {
  // Fallback for vite-standalone dev mode.
  if (cmd === 'preflight') {
    return { claude: null, codex: null, all_ok: false } as unknown as T
  }
  if (cmd === 'classify_workdir') {
    return {
      verdict: { level: 'ok' } as SafetyVerdict,
      gitignore_appended: false,
    } as unknown as T
  }
  return undefined as unknown as T
}

export async function runPreflight(
  claudeOverride?: string,
  codexOverride?: string,
): Promise<PreflightReport> {
  const fn = await invoke()
  return fn<PreflightReport>('preflight', {
    claudePathOverride: claudeOverride,
    codexPathOverride: codexOverride,
  })
}

export async function classifyWorkdir(path: string): Promise<ClassifyResp> {
  const fn = await invoke()
  return fn<ClassifyResp>('classify_workdir', { path })
}

/**
 * v1.6.1: reads the existing `GOAL.md` in the workdir (if any), returning
 * `null` when the file is absent. Used by `Welcome.tsx` to pre-populate the
 * goal textarea so users can see and edit the existing immutable benchmark
 * before starting a session.
 */
export async function readGoalMd(path: string): Promise<string | null> {
  const fn = await invoke()
  try {
    const result = await fn<string | null>('read_goal_md', { path })
    return result ?? null
  } catch {
    // File-read errors are non-fatal for this UX affordance — fall back to
    // "no existing goal" and let the conflict modal pick up any mismatch.
    return null
  }
}

/**
 * Thrown when the workdir already has a `GOAL.md` whose content differs from
 * what the user typed, and `overwriteGoal` was not set. The UI should show a
 * three-way modal (use existing / overwrite / cancel) and re-invoke
 * `startSession` with the chosen resolution.
 */
export class GoalConflictError extends Error {
  constructor(public existing: string) {
    super('GOAL.md already exists with different content')
    this.name = 'GoalConflictError'
  }
}

export async function startSession(
  goal: string,
  workdir: string,
  opts?: { overwriteGoal?: boolean },
): Promise<void> {
  const fn = await invoke()
  try {
    await fn<void>('start_session', {
      req: {
        goal,
        workdir,
        overwrite_goal: opts?.overwriteGoal ?? false,
      },
    })
  } catch (err: unknown) {
    // Tauri serializes our StartError enum as { kind, ... }. Fall through to
    // a generic Error for legacy string errors and anything we don't recognize
    // so the caller still sees a helpful message.
    if (typeof err === 'object' && err !== null && 'kind' in err) {
      const kind = (err as { kind: string }).kind
      if (kind === 'goal_conflict' && 'existing' in err) {
        throw new GoalConflictError((err as { existing: string }).existing)
      }
      if (kind === 'other' && 'message' in err) {
        throw new Error((err as { message: string }).message)
      }
    }
    if (typeof err === 'string') throw new Error(err)
    throw err
  }
}

export async function pauseSession(): Promise<void> {
  const fn = await invoke()
  await fn<void>('pause_session')
}

export async function stopSession(): Promise<void> {
  const fn = await invoke()
  await fn<void>('stop_session')
}

// Subscribe to `cccplayer://event` emissions from the orchestrator. Returns
// an unsubscribe function. Falls back to a no-op when running outside Tauri.
export async function subscribeEvents(
  handler: (ev: import('./types').Event) => void,
): Promise<() => void> {
  try {
    const mod = await import('@tauri-apps/api/event')
    const un = await mod.listen<import('./types').Event>(
      'cccplayer://event',
      (payload) => handler(payload.payload),
    )
    return () => un()
  } catch {
    return () => {}
  }
}

// Subscribe to raw stdout/stderr lines from the spawned CLIs. One event per
// line. Returns an unsubscribe function.
export async function subscribeRawLog(
  handler: (line: import('./types').RawLogLine) => void,
): Promise<() => void> {
  try {
    const mod = await import('@tauri-apps/api/event')
    const un = await mod.listen<import('./types').RawLogLine>(
      'cccplayer://raw',
      (payload) => handler(payload.payload),
    )
    return () => un()
  } catch {
    return () => {}
  }
}

// Open the native macOS folder-picker. Returns the chosen absolute path,
// or `null` if the user cancelled. No-op outside Tauri.
export async function pickFolder(defaultPath?: string): Promise<string | null> {
  try {
    const mod = await import('@tauri-apps/plugin-dialog')
    const result = await mod.open({
      directory: true,
      multiple: false,
      defaultPath,
      title: 'Choose a workspace folder',
    })
    if (result == null) return null
    return typeof result === 'string' ? result : result[0] ?? null
  } catch {
    return null
  }
}
