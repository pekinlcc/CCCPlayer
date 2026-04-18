You are working in `{workdir}`. `GOAL.md` is the user's goal (read-only,
immutable). `PRD.md` is the current design — a LIVING document. If
implementation reveals that a design decision or milestone in `PRD.md`
needs to change to reach the goal, update `PRD.md` in the same turn
alongside your code change. The goal is what matters; the plan exists
to serve the goal.

## Step 1 — Goal-anchored milestone selection (do this FIRST)

Before writing any code:

1. Re-read `GOAL.md` end to end.
2. Look at `PRD.md`'s `## Milestones` section for the next unchecked
   milestone.
3. Ask yourself honestly: **given everything you know right now**
   (code already written, reviews received, constraints discovered in
   earlier turns), is this milestone still the shortest remaining path
   to `GOAL.md`?

   - **If yes**: identify the specific sub-goal(s) this milestone
     advances (cite them by number). Write down the `goal_anchor`:
     which sentence or sub-goal in `GOAL.md` will be closer to
     delivered after this turn. Proceed to Step 2.
   - **If no — the milestone is stale, off-target, or has a better
     alternative**: don't mechanically execute it. PIVOT:
       a. Update `PRD.md` first:
          - mark the superseded milestone in the Milestones section
            (`- [~] M3: <title>  superseded by M3b: <reason>`),
          - add the new milestone,
          - append a one-line entry to `## Changelog`:
            "round N: superseded M3 because <concrete evidence>;
             replaced with M3b because <why this is shorter path>".
       b. Then proceed with the NEW next milestone in Step 2.

   Guardrail: only pivot when you can articulate a concrete,
   evidence-based reason. "I want to try a different approach" alone
   is not enough. If in doubt, keep the current milestone.

## Step 2 — Implement the milestone

Guidelines:

- Favor small, compilable, testable increments over large refactors.
- Add tests when the milestone implies them.
- Run the project's build / test command if one exists; report its
  result.
- When you close out a milestone, check it off (`- [x]`) in `PRD.md`'s
  `## Milestones` section. Don't leave stale "todo" items once a thing
  is done.
- If during implementation you discover a separate PRD-level gap
  (missing module, wrong data model, infeasible dependency), UPDATE
  `PRD.md` in this same turn under the relevant section AND append a
  Changelog entry. Don't silently diverge from the written design, and
  don't defer it to a vague `Open Questions` bullet unless you
  genuinely cannot resolve it.
- Never touch `GOAL.md` or any `codex_review_v*.md` file.
- Every file write must be atomic (temp file + rename).

## Step 3 — Output

When done, print to stdout:

    IMPLEMENTING done: <milestone title>
    goal_anchor: <GOAL.md sub-goal or sentence this advances>
    files: <comma-separated paths>
    plan_revised: <yes: M3 superseded by M3b because ... | no>
    prd_updated: <yes | no>
    build: <ok | failed | n/a>   tests: <passed/total | n/a>
