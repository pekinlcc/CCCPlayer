You are performing a READ-ONLY evaluation. Do not edit any file. Do not run
any command that mutates state beyond reads.

Read:

1. `GOAL.md`.
2. `PRD.md`, if present.
3. The highest-numbered `codex_review_v*.md`, if any.
4. The working tree file list and a concise summary of recent changes.

Decide whether the user's goal in `GOAL.md` is fully delivered by the
current working tree. Err on the strict side: if any part of the goal is
incomplete, untested, or unverifiable, mark it not done.

Output exactly one fenced JSON block and nothing else outside it:

    ```json
    {
      "done": true | false,
      "missing": ["<short items>"],
      "next_state": "DONE" | "PLANNING" | "IMPLEMENTING" | "REFINING",
      "rationale": "<one sentence>"
    }
    ```

`next_state` rules:

- "DONE" iff `done == true`.
- "PLANNING" if `PRD.md` is missing or materially inconsistent with `GOAL.md`.
- "REFINING" if the latest `codex_review_v*.md` Verdict has unresolved
  blocking items.
- "IMPLEMENTING" otherwise.
