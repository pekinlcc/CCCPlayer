You are working in `{workdir}`. Read, in order:

1. `GOAL.md` (read-only, immutable).
2. `PRD.md` — especially the `## Shelved disagreements` section if present.
3. `codex_review_v{N}.md` — the highest-numbered review file.
4. Any earlier `codex_review_v{N-1}.md`, `codex_review_v{N-2}.md` to
   understand how long a given blocking item has been contested.
5. If present, a `## Goal-Check 待办` section appended to the highest
   review file by the orchestrator — this lists items BOTH agents agreed
   are still missing for the goal even though code review was approved.
   Treat each with blocking-level severity.

That review ends with `## Verdict` listing blocking + non_blocking.
Combine that list with any Goal-Check 待办 items as your full work set
for this turn.

Task for this turn:

A. For every blocking item AND every Goal-Check 待办 item, choose ONE:
   - **accept**  : implement the fix in code and/or update `PRD.md`.
   - **partial** : implement the part you agree with; explain what you
                   did not do and why.
   - **reject**  : do NOT implement. Provide a specific technical reason
                   (performance, simplicity, goal fit, API constraint,
                   etc). "I disagree" is not a reason; "this would add a
                   round trip per call and the goal calls for 100 r/s"
                   is a reason.
   - **shelve**  : the item has been contested for 2+ consecutive rounds
                   with both sides maintaining position and no new
                   argument emerging; add it to `PRD.md`'s
                   `## Shelved disagreements` section and note it here.
                   Shelving is mutual — you are declaring the item as
                   "we agree to disagree"; Codex will confirm or refuse
                   in the next review.

B. Consider non_blocking items; act on them only when clearly beneficial.

C. Append (do NOT overwrite) a `## Claude Code 回应` section to the
   SAME `codex_review_v{N}.md` file, with one subsection per blocking
   item:

       ### <verbatim blocking item title>
       - status: accepted | partial | rejected | shelved
       - action: <what changed, with file paths>  (omit if rejected/shelved)
       - reason: <why this resolves the item; or why you reject/shelve>
       - contested_rounds: <N> (only if status in {rejected, shelved};
         count of prior rounds where this exact item appeared and you
         maintained the same position).

D. If this refinement changes a design decision in `PRD.md`, update the
   PRD in the same turn. Preserve older decisions as history when
   useful; append a brief note under the relevant section. The goal is
   to keep `PRD.md` as an accurate living description of current design.

E. If any rejected/shelved item implies a design trade-off the user
   should know about, add a one-line note to `PRD.md` under a section
   named `## Shelved disagreements` (create if missing):

       ### <item title>
       - first raised in: codex_review_v<N>.md
       - contested rounds: <count>
       - Claude position: <one sentence>
       - Codex position (as written): <one sentence>
       - goal impact: <why shelving is safe for the goal>

Rules:

- Never modify earlier `codex_review_v*.md` files.
- Never edit `GOAL.md`.
- Every file write must be atomic (temp file + rename).
- Rejecting a whole review without any technical reasoning is not
  permitted. Either engage or accept.

When done, print to stdout:

    REFINING done on v{N}: accepted <X>, partial <Y>, rejected <Z>, shelved <S>
    files touched: <comma-separated paths>
