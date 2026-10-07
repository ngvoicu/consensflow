# Claude transcripts of live runs — provenance

Kept 2026-10-07 by `npm run live:stops -- --case <case> --keep <folder>` (Claude
Code 2.1.292, the native daemon of the receipt branch, the cheap eval model),
then scrubbed with `node tests/live/scrub-transcript.mjs <transcript> <fixture>`.
Every record stays, in its order; the session id becomes `$SESSION` (a test puts
its own there), and what is personal or opaque is replaced: folders, the
branch, account and organization ids, what an attachment carried, request ids,
thinking signatures. The user's and the assistant's words stay as written.

- `stopped-before-a-word.jsonl` — `--case early`. The daemon paused the task a
  moment after its brief and pressed Escape; Claude, in the hooks of the
  prompt, stopped, put the brief back in its input box and wrote no record of
  it. The transcript ends on the brief, with what Claude keeps beside it.
- `stopped-in-the-stop-hook.jsonl` — `--case slowhook`. Escape while a Stop hook
  that ignores signals ran, after the answer `DONE` was written. Claude's
  interrupt record is the last: one text block, `[Request interrupted by
  user]`, and no `interruptedMessageId`.

A new Claude version that writes these otherwise is a new fixture, not an edit.
