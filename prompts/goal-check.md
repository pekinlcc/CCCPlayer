You are performing a READ-ONLY evaluation. Do not edit any file. Do not run
any command that mutates state beyond reads.

Read:

1. `GOAL.md`.
2. `PRD.md`, if present. Note the `## Shelved disagreements` section if
   one exists — those items are out of scope for "missing" (both agents
   have already agreed to disagree on them and ship around them).
3. The highest-numbered `codex_review_v*.md`, if any, including its
   `## Claude Code 回应` section.
4. The working tree file list and a concise summary of recent changes.

Decide whether the user's goal in `GOAL.md` is substantively delivered
by the current working tree, accounting for any shelved items as agreed
non-blockers. Err on the strict side: if any core part of the goal is
incomplete, untested, or unverifiable, mark it not done. "Substantively
delivered" means: a reasonable user reading `GOAL.md` would look at the
tree and call the goal met, even if minor shelved disagreements remain.

Output exactly one fenced JSON block and nothing else outside it:

    ```json
    {
      "done": true | false,
      "missing": ["<concrete items still required for the goal>"],
      "shelved": ["<titles copied verbatim from PRD.md's
                   `## Shelved disagreements` section>"],
      "next_state": "DONE" | "PLANNING" | "IMPLEMENTING" | "REFINING",
      "rationale": "<2-3 sentences: what's truly missing, or why the
                   goal is substantively met; call out shelved items
                   explicitly so the user can see the trade-offs>"
    }
    ```

IMPORTANT rules for the `shelved` field (v1.4.1):

- The `shelved` array must ONLY contain items that already exist in
  `PRD.md`'s `## Shelved disagreements` section. This phase is
  read-only; you cannot introduce new shelved items here.
- If `PRD.md` has no `## Shelved disagreements` section, `shelved` MUST
  be an empty array `[]`.
- If you believe an item *should* be shelved but isn't yet recorded in
  PRD: list it in `missing` and explicitly note in `rationale` that
  "the next REFINING turn should formalize this into
  `PRD.md`'s `## Shelved disagreements`". Don't pre-shelve here.
- This constraint exists because the other agent's goal-check reads
  the same PRD — if you self-shelve items that aren't actually in
  PRD, the two agents' views diverge, the stagnation detector sees a
  persistent disagreement, and the session errors out. Shelved
  disagreements live in PRD or they don't exist.

`next_state` rules:

- "DONE" iff `done == true`. Done is allowed even if `shelved` is non-
  empty, provided each shelved item has a recorded agreement in
  `PRD.md`'s `## Shelved disagreements` section.
- "PLANNING" if `PRD.md` is missing or materially inconsistent with
  `GOAL.md`.
- "REFINING" if the latest review's Verdict has unresolved blocking
  items, or if `missing` is non-empty.
- "IMPLEMENTING" otherwise (rare — usually means "review says approved
  but goal check disagrees").

Be honest about shelved items. Listing them transparently is more
valuable than pretending consensus existed where it didn't.
