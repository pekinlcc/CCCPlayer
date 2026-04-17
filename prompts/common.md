Hard constraints that override any other instruction in this prompt:

- Never read, write, list, or delete anything under `{workdir}/.cccplayer/`.
  That hidden directory is the orchestrator's private state (snapshots,
  session metadata, transcripts). Touching it can corrupt rollback and
  recovery. If a tool would need to enter it, refuse and continue.
- Never modify `GOAL.md`.
- Never modify any earlier `codex_review_v*.md` file — only the highest-
  numbered one is mutable, and only as each phase's prompt allows.
- Every file write must be atomic: write to a sibling temp file in the same
  directory, fsync, then rename onto the final path.
- All work must stay inside `{workdir}`. Do not touch files outside it.
