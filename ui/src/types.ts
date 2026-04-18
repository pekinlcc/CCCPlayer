// Types mirror the Rust side. Kept hand-written for M1; a codegen step
// (e.g. ts-rs) can replace this later.

export type SessionState =
  | 'CREATED'
  | 'RUNNING'
  | 'PAUSED'
  | 'ERRORED'
  | 'DONE'
  | 'ABANDONED'

export type Phase =
  | 'IDLE'
  | 'PLANNING'
  | 'IMPLEMENTING'
  | 'REVIEWING'
  | 'REFINING'
  | 'GOAL_CHECK'

export type Verdict = 'approved' | 'changes_requested' | 'blocked'

export type Agent = 'claude' | 'codex'

export type TurnOutcome =
  | 'ok'
  | 'output_malformed'
  | 'stalled'
  | 'crashed'
  | 'auth_failed'
  | 'refused'
  | 'flapping'

export interface Event {
  at: string
  round: number
  kind: string
  [key: string]: unknown
}

export interface CliInfo {
  path: string
  version_line: string
  supports_auto_approve: boolean
  auto_approve_flag: string | null
}

export interface PreflightReport {
  claude: CliInfo | null
  codex: CliInfo | null
  all_ok: boolean
}

export type SafetyVerdict =
  | { level: 'ok' }
  | { level: 'soft_warn'; reasons: string[] }
  | { level: 'strong_warn'; reason: string }
  | { level: 'blocked'; reason: string }

export interface ClassifyResp {
  verdict: SafetyVerdict
  gitignore_appended: boolean
}

export type RawStream = 'stdout' | 'stderr'

export interface RawLogLine {
  at: string
  round: number
  agent: Agent
  phase: Phase
  stream: RawStream
  line: string
}

// Snapshot of one agent's most recent goal-check output (v1.1+; shelved v1.3+).
export interface GoalCheckSnapshot {
  at: string
  round: number
  done: boolean
  missing: string[]
  shelved: string[]
  rationale: string
}
