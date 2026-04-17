import { useState } from 'react'
import type { PreflightReport, SafetyVerdict } from '../types'

// Implements PRD §6.2 "首次启动 / 空白态".
export function Welcome(props: {
  preflight: PreflightReport | null
  workdir: string
  safety: SafetyVerdict | null
  onChooseWorkdir: (path: string) => void
}) {
  const { preflight, workdir, safety, onChooseWorkdir } = props
  const [pathInput, setPathInput] = useState(workdir)

  const claudeOk =
    preflight?.claude?.supports_auto_approve === true
  const codexOk = preflight?.codex?.supports_auto_approve === true

  return (
    <section className="welcome">
      <p className="tagline">
        Give two model agents a goal and walk away. They design, build,
        review, and iterate until done.
      </p>

      <div className="card">
        <h2>Preflight</h2>
        <ul className="preflight-list">
          <PreflightRow
            label="Claude Code CLI"
            ok={!!preflight?.claude}
            detail={
              preflight?.claude
                ? preflight.claude.version_line
                : 'Not found on PATH. Install Claude Code CLI.'
            }
          />
          <PreflightRow
            label="Claude auto-approve flag"
            ok={claudeOk}
            detail={
              claudeOk
                ? `Using flag: ${preflight?.claude?.auto_approve_flag}`
                : 'CLI does not accept a known auto-approve flag; CCCPlayer needs this to run unattended. Upgrade the CLI.'
            }
          />
          <PreflightRow
            label="Codex CLI"
            ok={!!preflight?.codex}
            detail={
              preflight?.codex
                ? preflight.codex.version_line
                : 'Not found on PATH. Install Codex CLI.'
            }
          />
          <PreflightRow
            label="Codex auto-approve flag"
            ok={codexOk}
            detail={
              codexOk
                ? `Using flag: ${preflight?.codex?.auto_approve_flag}`
                : 'Same as above — upgrade Codex CLI.'
            }
          />
        </ul>
      </div>

      <div className="card">
        <h2>Workspace</h2>
        <p className="hint">
          Select a local folder. Existing code is fine — CCCPlayer will survey
          it first and improve on top of it. Do not choose your home directory
          or a system path (§16.14).
        </p>
        <div className="row">
          <input
            type="text"
            placeholder="/Users/you/dev/my-project"
            value={pathInput}
            onChange={(e) => setPathInput(e.target.value)}
          />
          <button
            type="button"
            onClick={() => onChooseWorkdir(pathInput.trim())}
            disabled={!pathInput.trim()}
          >
            Use this folder
          </button>
        </div>
        {safety && <SafetyNotice verdict={safety} />}
      </div>
    </section>
  )
}

function PreflightRow(props: { label: string; ok: boolean; detail: string }) {
  return (
    <li className={`preflight-row ${props.ok ? 'ok' : 'bad'}`}>
      <span className="dot">{props.ok ? '●' : '●'}</span>
      <span className="label">{props.label}</span>
      <span className="detail">{props.detail}</span>
    </li>
  )
}

function SafetyNotice(props: { verdict: SafetyVerdict }) {
  switch (props.verdict.level) {
    case 'ok':
      return <div className="safety ok">Workspace looks fine.</div>
    case 'soft_warn':
      return (
        <div className="safety soft-warn">
          <strong>Heads up:</strong>
          <ul>
            {props.verdict.reasons.map((r, i) => (
              <li key={i}>{r}</li>
            ))}
          </ul>
        </div>
      )
    case 'strong_warn':
      return (
        <div className="safety strong-warn">
          <strong>⚠ Confirm before continuing:</strong> {props.verdict.reason}
          <p className="hint">
            CCCPlayer will let two agents freely modify files inside this
            folder. Re-type the absolute path below to confirm.
          </p>
        </div>
      )
    case 'blocked':
      return (
        <div className="safety blocked">
          <strong>Refused:</strong> {props.verdict.reason}
        </div>
      )
  }
}
