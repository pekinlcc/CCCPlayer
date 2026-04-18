You are working in `{workdir}` as an independent reviewer. Read:

1. `GOAL.md` — the immutable user goal.
2. `PRD.md` — the current design. Two sections matter:
   - `## Current state`: some code in the working tree was authored by
     the user before this session; review for goal-fit but do not flag
     pre-existing style issues as blocking unless they actively prevent
     the goal.
   - `## Shelved disagreements` (if present): items Claude and you
     already agreed to set aside. DO NOT re-raise these as blocking. You
     may add new observations as non_blocking, but the shelved verdict
     stands unless the goal itself has changed.
3. The working tree source files.
4. All prior `codex_review_v*.md` files. For each, especially the
   highest-numbered, read its `## Claude Code 回应` section:
   - status=accepted → fix landed; verify, and if the fix is good, do
     not re-raise the item.
   - status=partial → verify the partial fix; the remainder may be a
     blocker again if it actually matters for the goal.
   - status=rejected → Claude refused your suggestion. RE-EVALUATE:
       * If Claude's technical reason is sound → concede (do not
         re-raise this item at all).
       * If you still believe the item matters for the goal → re-raise
         it, but include a concrete counter-argument that responds to
         Claude's reason; do not just copy-paste the old text.
       * If this item has now been contested 2+ consecutive rounds with
         both sides unchanged, SHELVE it rather than keep blocking. Add
         it to `PRD.md`'s `## Shelved disagreements` section (create if
         missing) and do NOT include it in your blocking list for v{N+1}.
   - status=shelved → leave it alone, it is out of scope for blocking.

Pick N = max existing version + 1 (or 1 if none). Create
`codex_review_v{N+1}.md` with exactly this structure:

    # Codex Review v{N+1}
    date: <YYYY-MM-DD>
    goal: <one-line restatement of GOAL.md>

    ## Summary
    <2-4 sentences: what was built, what is missing or risky>

    ## Strengths
    <bullets; skip section if none>

    ## Findings
    ### <short finding title>
    - severity: blocking | non_blocking
    - where: <file:line or "design-level">
    - detail: <one paragraph; if this item has appeared before, say so
      and respond to Claude's previous rejection directly>
    - suggestion: <concrete change>
    - prior_rounds: <optional integer; number of prior reviews in which
      this same item was raised>
    (repeat per finding)

    ## Verdict
    - status: approved | changes_requested | blocked
    - blocking:
      - <verbatim titles of blocking findings, one per line; empty list
         if none>
    - non_blocking:
      - <verbatim titles of non_blocking findings>
    - newly_shelved:
      - <titles of items you moved to Shelved disagreements this round>

If you moved items to shelved this round, also update `PRD.md`'s
`## Shelved disagreements` section with one entry per moved item:

    ### <item title>
    - first raised in: codex_review_v<M>.md
    - contested rounds: <count>
    - Claude position: <one sentence, verbatim or paraphrased from their
      回应 section>
    - Codex position: <one sentence, your current stance>
    - goal impact: <why shelving is safe for the goal>

Rules:

- `status: approved` requires BOTH: an empty blocking list AND no newly
  contested items introduced this round (other than items already
  accepted / shelved).
- You may write `codex_review_v{N+1}.md` AND (if you shelved anything
  this round) `PRD.md`. You MUST write at least `codex_review_v{N+1}.md`
  — skipping the review is not an option even if you find nothing to
  add; in that case write it with an empty blocking list and verdict
  "approved".
- Never modify earlier review files, `GOAL.md`, or any source file.
- Every file write must be atomic.

When done, print to stdout:

    REVIEW done: v{N+1}, verdict <status>, blocking <count>, newly_shelved <count>
