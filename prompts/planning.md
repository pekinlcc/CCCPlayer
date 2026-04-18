You are working in `{workdir}`. First read `GOAL.md` — that is the user's
immutable goal for this session; never modify it.

IMPORTANT: the working directory may already contain the user's existing
code — possibly a real in-progress project. Before writing any design,
SURVEY the current state:

- List top-level files and directories.
- Identify language, build system, entry points, and key modules.
- Note what is already implemented vs. what the goal still requires.
- Never assume an empty workspace; never plan to scrap existing code
  unless `GOAL.md` explicitly requires it.

Task for this turn: produce or update `PRD.md` so it fully specifies how
to reach `GOAL.md` FROM THE CURRENT STATE. `PRD.md` is the single design
document; later turns will implement code against it.

Required top-level headings (in order):

1. Goal            — verbatim restatement of `GOAL.md`, one sentence.
2. Current state   — your survey findings. Write "empty workspace" if the
                     directory is empty; otherwise 3–10 bullets covering
                     stack, entry points, what is already done, and any
                     obvious gaps or risks inherited from existing code.
3. Scope           — what is in, phrased as *delta* over Current state.
4. Non-goals       — what is explicitly out.
5. Design          — architecture, key modules, file layout, data model,
                     external dependencies. Where existing code already
                     fits the design, say "reuse as-is" rather than
                     redesigning.
6. Milestones      — ordered checklist of shippable increments, starting
                     from Current state and ending at `GOAL.md`.
7. Shelved disagreements — items Claude and Codex have agreed to
                     disagree on (see session rules in common prompt).
                     Preserve existing entries verbatim. Empty list is
                     fine when no disagreement has been shelved yet.
8. Open Questions  — anything you could not decide; empty list is fine.

Guidelines for keeping `PRD.md` a living document:

- If `PRD.md` already exists, revise it in place; preserve prior
  decisions unless newly contradicted by reality (review feedback,
  implementation discoveries, or user clarifications).
- When a milestone is done, check it off in the Milestones section.
  When one becomes irrelevant or reveals a better path, rewrite it.
- Keep Current state factual and current — update it when code has
  moved on; stale "Current state" misleads the next review.
- When a `codex_review_v*.md` exists, read the highest-numbered one
  (including its `## Claude Code 回应` section) and fold PRD-level
  feedback into this revision.
- Keep the document concise. If the design has stabilized, trim prose
  in favor of bullets. Aim for a reader to grasp the design in under 5
  minutes.

Write `PRD.md` atomically (temp file + rename). Do not modify any other
file in this turn.

When done, print one line to stdout:

    PLANNING done: <N> sections, +<added>/-<removed> lines
