import type { ReactNode } from 'react'

export type ShellStatus = 'idle' | 'running' | 'done' | 'stopped' | 'errored'

// Winamp-style outer chrome. Every screen wears this shell; the inner
// composition of display / transport / progress panel / footer comes from
// the caller. Approved mockup: ui/mockups/cyberpunk-preview.html.
export function Shell(props: { status: ShellStatus; statusLabel: string; children: ReactNode }) {
  return (
    <div className="shell">
      <div className="shell-titlebar">
        <span className="dot r" />
        <span className="dot y" />
        <span className="dot g" />
        <span className="brand">CCCPlayer</span>
        <span className="tag">CLAUDE·CODE × CODEX</span>
        <span className="spacer" />
        <span className={`status ${props.status}`}>
          {props.status === 'running' && '▶ '}
          {props.status === 'idle' && '◇ '}
          {props.statusLabel.toUpperCase()}
        </span>
      </div>
      {props.children}
    </div>
  )
}

// SVG glyphs for the transport bar.
export const PlayIcon = (p: { size?: number }) => (
  <svg width={p.size ?? 16} height={p.size ?? 16} viewBox="0 0 16 16" aria-hidden="true">
    <polygon points="3,2 14,8 3,14" fill="currentColor" />
  </svg>
)

export const PauseIcon = (p: { size?: number }) => (
  <svg width={p.size ?? 16} height={p.size ?? 16} viewBox="0 0 16 16" aria-hidden="true">
    <rect x="3" y="2" width="4" height="12" fill="currentColor" />
    <rect x="9" y="2" width="4" height="12" fill="currentColor" />
  </svg>
)

export const StopIcon = (p: { size?: number }) => (
  <svg width={p.size ?? 16} height={p.size ?? 16} viewBox="0 0 16 16" aria-hidden="true">
    <rect x="3" y="3" width="10" height="10" fill="currentColor" />
  </svg>
)

// Decorative mini meters shown in the transport bar.
export function Meter(props: { label: string; active: boolean }) {
  const cls = (i: number) =>
    props.active
      ? `b h${i} on ${['c1', 'c1', 'c2', 'c3'][i - 1] ?? 'c1'}`
      : `b h${i}`
  return (
    <div className="meter">
      <span>{props.label}</span>
      <div className="bars">
        <span className={cls(1)} />
        <span className={cls(2)} />
        <span className={cls(3)} />
        <span className={cls(4)} />
      </div>
    </div>
  )
}
