You are working in `{workdir}`. Read, in order:

1. `GOAL.md` (read-only).
2. `PRD.md`.
3. `codex_review_v{N}.md` — the highest-numbered review file.
4. If present, a "## Goal-Check 待办" section appended to that same review
   file by the orchestrator — this lists items that BOTH agents agreed
   are still missing for the goal, even though the latest verdict was
   "approved" on code quality. Treat each such item with the same
   severity as a blocking review finding.

That review ends with a "## Verdict" section listing blocking and optional
non_blocking items. Combine that list with any "Goal-Check 待办" items as
your full work set for this turn.

Task for this turn:

A. Address every blocking item AND every Goal-Check 待办 item. For each,
   either fix it (in code and/or `PRD.md`) or reject it with a specific
   technical reason.

B. Consider non_blocking items; act on them only when clearly beneficial.

C. Append (do not overwrite) a "## Claude Code 回应" section to the SAME
   `codex_review_v{N}.md` file, with one subsection per blocking item:

       ### <verbatim blocking item title>
       - status: accepted | partial | rejected
       - action: <what changed, with file paths>   (omit if rejected)
       - reason: <why this resolves the item, or why rejected>

Rules:

- Never modify earlier `codex_review_v*.md` files.
- Never edit `GOAL.md`.
- All file writes must be atomic.

When done, print to stdout:

    REFINING done on v{N}: accepted <X>, partial <Y>, rejected <Z>
    files touched: <comma-separated paths>
