You are working in `{workdir}`. Read, in order:

1. `GOAL.md` (read-only, immutable).
2. `PRD.md` — especially `## Sub-goals`, `## Milestones` (including any
   `- [~]` superseded ones and the Changelog), and `## Shelved
   disagreements` if present.
3. `codex_review_v{N}.md` — the highest-numbered review file.
4. Any earlier `codex_review_v{N-1}.md`, `codex_review_v{N-2}.md` to
   understand how long a given blocking item has been contested.
5. If present, a `## Goal-Check 待办` section appended to the highest
   review file by the orchestrator — items BOTH agents agreed are
   still missing for the goal. Treat each as blocking-severity.

The review ends with `## Verdict` listing blocking, path_drift,
non_blocking, and newly_shelved items. Combine the blocking list with
any Goal-Check 待办 items and the path_drift list as your full work
set for this turn.

## For each blocking / path_drift item, choose ONE:

- **accept**  : implement the fix in code and/or update `PRD.md`.
                For path_drift items, the "fix" is usually a PRD
                revision — mark the affected milestone as superseded
                and add a Changelog entry per the common session
                rules.
- **partial** : implement the part you agree with; explain what you
                did not do and why.
- **reject**  : do NOT implement. Provide a specific technical reason
                (performance, simplicity, goal fit, API constraint).
                "I disagree" is not a reason; "this would add a round
                trip per call and sub-goal 3 requires 100 r/s" is.
- **shelve**  : the item has been contested for 2+ consecutive rounds
                with no new argument; move it to `PRD.md`'s
                `## Shelved disagreements` section and note it here.
                Shelving declares "we agree to disagree"; Codex will
                confirm or refuse in the next review.
- **stale**   : the item refers to a milestone or design decision that
                has since been superseded in `PRD.md`'s Changelog.
                Cite the Changelog entry and skip.

## For every blocking / path_drift item, you MUST also state goal_impact

Every response gets a one-line `goal_impact` field:

- For accept / partial: which `GOAL.md` sub-goal this fix advances or
  unblocks. If the fix doesn't clearly advance the goal, reconsider —
  it may be a style nit that should be deferred, not accepted.
- For reject / shelve: why ignoring this item does NOT harm `GOAL.md`
  delivery. If you can't defend that cleanly, you probably shouldn't
  be rejecting.
- For stale: cite the PRD Changelog entry (`round N changelog`) that
  makes this item irrelevant.

No `goal_impact` = response considered incomplete.

## Response file format

Append (do NOT overwrite) a `## Claude Code 回应` section to the SAME
`codex_review_v{N}.md` file, with one subsection per blocking AND
path_drift item:

    ### <verbatim item title>
    - status: accepted | partial | rejected | shelved | stale
    - action: <what changed, with file paths>  (omit for reject / shelve / stale)
    - reason: <why this resolves the item; or why you reject / shelve / consider stale>
    - goal_impact: <one sentence per the rules above>
    - contested_rounds: <N> (only if status in {rejected, shelved};
      count of prior rounds this same item appeared and you
      maintained the same position).

## PRD updates in this turn

If this refinement implies a design or milestone change, update
`PRD.md` in the same turn. Preserve older decisions as Changelog
history; supersede rather than silently delete. Goal is to keep
`PRD.md` an accurate living description of current design.

If you move items to shelved this round, update `PRD.md`'s `## Shelved
disagreements` section (create if missing):

    ### <item title>
    - first raised in: codex_review_v<N>.md
    - contested rounds: <count>
    - Claude position: <one sentence>
    - Codex position (as written): <one sentence>
    - goal impact: <why shelving is safe for the goal>

## Consider non_blocking

Act on non_blocking items only when clearly beneficial and you have
bandwidth — never at the cost of blocking work.

## Rules

- Never modify earlier `codex_review_v*.md` files.
- Never edit `GOAL.md`.
- Every file write must be atomic (temp file + rename).
- Rejecting a whole review without technical reasoning is not permitted.
- Mechanically addressing items without thinking about GOAL.md impact
  wastes tokens and drifts the session. Every item gets a genuine
  goal_impact justification.

When done, print to stdout:

    REFINING done on v{N}: accepted <X>, partial <Y>, rejected <Z>, shelved <S>, stale <T>
    files touched: <comma-separated paths>
    prd_updated: <yes | no>
