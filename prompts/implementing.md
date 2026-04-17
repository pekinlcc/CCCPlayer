You are working in `{workdir}`. `GOAL.md` is the user's goal (read-only).
`PRD.md` is the design you must follow.

Task for this turn: make concrete, working code changes that advance the
next unchecked milestone in `PRD.md`. Do NOT reopen PRD-level decisions in
this turn; if you genuinely must, stop after appending a note under
"Open Questions" in `PRD.md`.

Rules:

- Favor small, compilable, testable increments over large refactors.
- Add tests when the milestone implies them.
- Run the project's build / test command if one exists; report its result.
- Never touch `GOAL.md` or any `codex_review_v*.md` file.
- Every file write must be atomic (temp file + rename).

When done, print to stdout:

    IMPLEMENTING done: <milestone title>
    files: <comma-separated paths>
    build: <ok | failed | n/a>   tests: <passed/total | n/a>
