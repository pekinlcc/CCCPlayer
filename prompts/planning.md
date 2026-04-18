You are working in `{workdir}`. First read `GOAL.md` — that is the user's
immutable goal for this session; never modify it.

IMPORTANT: the working directory may already contain the user's existing
code — possibly a real in-progress project. Before writing any design,
SURVEY the current state:

- List top-level files and directories.
- Identify language, build system, entry points, and key modules.
- Note what is already implemented vs. what the goal still requires.
- Never assume an empty workspace; never plan to scrap existing code
  unless `GOAL.md` explicitly requires it.

Task for this turn: produce or update `PRD.md` so it fully specifies how
to reach `GOAL.md` FROM THE CURRENT STATE. Think of `PRD.md` as **your
current best hypothesis for the path to GOAL.md** — not a contract, but
a scaffold that later turns will extend, revise, or replace.

## Step 1 — Goal decomposition (do this BEFORE writing the PRD body)

Break `GOAL.md` into the smallest set of independently-verifiable
sub-goals. Each sub-goal should be:

- a concrete deliverable (something that can be inspected and judged
  "done" or "not done");
- directly traceable to a sentence or clause in `GOAL.md`;
- independent of implementation choices (describe WHAT, not HOW).

Keep this decomposition internal — it informs the PRD sections below.
You'll also use it to rubber-duck your PRD before saving (Step 3).

## Step 2 — Write or revise PRD.md

Required top-level headings (in order):

1. **Goal**            — verbatim restatement of `GOAL.md`, one sentence.
2. **Sub-goals**       — your Step-1 decomposition, numbered. Each item
                         is a one-line deliverable you can later check
                         off against implementation. These are
                         disposable scaffolding: if a later turn
                         discovers a sub-goal is wrong or has been
                         superseded, that turn is expected to update
                         this list (see Step 4 below).
3. **Current state**   — your survey findings. Write "empty workspace"
                         if the directory is empty; otherwise 3–10
                         bullets covering stack, entry points, what is
                         already done, and any obvious gaps or risks
                         inherited from existing code.
4. **Scope**           — what is in, phrased as *delta* over Current
                         state. Tie each scope item back to a sub-goal
                         by reference (e.g. "advances sub-goal 2").
5. **Non-goals**       — what is explicitly out.
6. **Design**          — architecture, key modules, file layout, data
                         model, external dependencies, and a brief
                         **implementation path** (which technologies
                         you'd use and why — think about feasibility,
                         performance, platform constraints, dependency
                         risk). Where existing code already fits the
                         design, say "reuse as-is" rather than
                         redesigning.
7. **Milestones**      — ordered checklist of shippable increments,
                         each tagged with the sub-goals it advances.
                         Format:
                             - [ ] M1: <title> — advances sub-goals <1,2>
                             - [ ] M2: …
                         Checkmarks are updated by the agent that
                         completes a milestone. Superseded milestones
                         stay visible as:
                             - [~] M3: <original title>
                                   superseded by M3b: <reason, 1 line>
8. **Shelved disagreements** — items Claude and Codex have agreed to
                         disagree on (see session rules in common
                         prompt). Preserve existing entries verbatim.
                         Empty list is fine when no disagreement has
                         been shelved yet.
9. **Changelog**       — PRD evolution history. Append a one-line
                         entry every time you revise the document in a
                         later turn:
                             - round N: <what changed, why, sub-goal link>
                         This is the audit trail; it lets a later
                         reviewer see *why* the PRD drifted, not just
                         the final state. Empty on first write.
10. **Open Questions** — anything you could not decide; empty list is
                         fine.

## Step 3 — Self-adversarial check (MANDATORY before saving)

Before atomically writing `PRD.md`, re-read `GOAL.md` end to end and
verify for each sentence/clause:

- Which sub-goal in your decomposition covers it?
- Which section of the PRD addresses how that sub-goal will be
  delivered?
- Would an implementer following this PRD exactly actually satisfy
  that part of GOAL.md? If you'd hesitate — revise.
- Is the implementation path realistic? Any technology choice, external
  dependency, or platform assumption that could break? Any performance
  or scaling concern that needs to be anticipated now?

Only save when every sentence of `GOAL.md` has a clear coverage chain:
GOAL → sub-goal → scope → design → milestones. Gaps = revise.

## Step 4 — Rules for later revisions

If this is not the first PRD write (previous versions already exist):

- Preserve prior decisions UNLESS newly contradicted by reality (code
  written, reviews received, user clarifications, feasibility
  discoveries).
- When a prior decision is contradicted, don't silently rewrite it —
  add a line to `## Changelog` explaining the shift and the
  superseding decision.
- When a milestone is done, check it off in the Milestones section.
  When a milestone becomes irrelevant or reveals a better path,
  supersede it (see Step 2 item 7 format). Never delete milestones
  silently.
- Keep Current state factual and current — update it when code has
  moved on; stale "Current state" misleads the next review.
- When a `codex_review_v*.md` exists, read the highest-numbered one
  (including its `## Claude Code 回应` section) and fold PRD-level
  feedback into this revision.
- Keep the document concise. If the design has stabilized, trim prose
  in favor of bullets. Aim for a reader to grasp the design in under
  5 minutes.

Write `PRD.md` atomically (temp file + rename). Do not modify any other
file in this turn.

When done, print one line to stdout:

    PLANNING done: <N> sections, +<added>/-<removed> lines, sub_goals <count>
