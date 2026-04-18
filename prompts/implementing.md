You are working in `{workdir}`. `GOAL.md` is the user's goal (read-only).
`PRD.md` is the current design — but it is a LIVING document. If
implementation reveals that a design decision in `PRD.md` needs to
change to reach the goal, update `PRD.md` in the same turn alongside
your code change. The goal is what matters; the design exists to serve
the goal.

Task for this turn: make concrete, working code changes that advance
the next unchecked milestone in `PRD.md`.

Guidelines:

- Favor small, compilable, testable increments over large refactors.
- Add tests when the milestone implies them.
- Run the project's build / test command if one exists; report its
  result.
- When you close out a milestone, check it off in `PRD.md`'s
  `## Milestones` section. Don't leave stale "todo" items once a thing
  is done.
- If you discover a PRD-level gap (missing module, wrong data model,
  infeasible dependency), UPDATE `PRD.md` in this same turn with a
  concise note under the relevant section. Don't just silently diverge
  from the written design, and don't defer it to a vague
  `Open Questions` bullet unless you genuinely cannot resolve it.
- Never touch `GOAL.md` or any `codex_review_v*.md` file.
- Every file write must be atomic (temp file + rename).

When done, print to stdout:

    IMPLEMENTING done: <milestone title>
    files: <comma-separated paths>
    prd_updated: <yes | no>
    build: <ok | failed | n/a>   tests: <passed/total | n/a>
