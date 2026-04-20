You are performing a READ-ONLY evaluation. Do not edit any file. Do not run
any command that mutates state beyond reads.

Read:

1. `GOAL.md`.
2. `PRD.md`, if present. Note TWO sections in particular:
   - `## Hard deliverables` — non-shelvable required outputs (files,
     commands, end-to-end behaviors) the user's goal treats as
     ship-criteria. These items **cannot be satisfied by shelving**;
     if any of them is not produced, `done` MUST be `false`.
   - `## Shelved disagreements` — genuine design disagreements both
     agents agreed to ship around. These are out of scope for
     "missing" *unless* one of them substantially refers to a hard
     deliverable (protocol violation; see v1.7 rule below).
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

IMPORTANT rules for the `done` field (v1.7):

- Before answering `done: true`, **walk through `## Hard deliverables`
  item by item**. For each hard deliverable, verify:
    (a) the required file / command / behavior is actually present
        and working in the current tree, OR
    (b) it is concretely recorded as accepted / partial in the latest
        review's response section with the delivered partial visible
        in the tree.
  If neither (a) nor (b) holds for any hard deliverable, `done` MUST
  be `false` — regardless of what the latest review says, regardless
  of shelved items, regardless of how "productive" the loop has been.
- You MAY NOT justify `done: true` with language like "the user can
  produce X themselves by running Y" for a hard deliverable. If X is
  listed as hard, it has to actually exist in the tree OR be an
  accepted partial with the remaining gap owned in PRD.
- The orchestrator runs an independent hard-deliverable gate after
  your answer: if any item in `missing[]` or `shelved[]` shares any
  distinguishing token (content word, stop-words filtered) with a
  hard deliverable, `done=true` is overridden to `done=false`
  automatically and a `Note` event explains why. Claiming done while
  the gate can still fire wastes a round — tell the truth here.

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
