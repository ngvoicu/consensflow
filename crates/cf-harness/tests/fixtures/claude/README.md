# Claude transcripts of live runs — provenance

Kept 2026-10-07 by `npm run live:stops -- --case <case> --keep <folder>` (Claude
Code 2.1.292, the native daemon of the receipt branch, the cheap eval model; the
flag that keeps the transcripts is `--transcripts <folder>` now, and `--keep`
leaves a run's home where it is), then scrubbed with
`cargo xtask scrub-transcript <transcript> <fixture>` (a Node script made these
files; the command that replaced it writes the same bytes from the same
transcript).
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

Kept 2026-10-11 by `npm run live:resume-cut -- --keep <folder>` (Claude Code
2.1.296, on Sonnet; `--transcripts <folder>` now), scrubbed the same way. Each
case kills the window's process with its turn at work, as the app's quit does,
and opens a window on `--resume` of the conversation with the app's own
arguments:

- `quit-before-a-word.jsonl` — `--case early`. Killed as soon as Claude's status
  read busy, before a word of its answer. The transcript ends on the user's
  message, with what Claude keeps beside it (and its own bookkeeping, which a
  quit writes).
- `quit-in-a-command.jsonl` — `--case command`. Killed inside a long command:
  the kill ended the command, and Claude wrote the error result of the call
  (`Exit code 144`) before it went. The transcript ends on that result.
- `quit-before-a-word-resumed.jsonl`, `quit-in-a-command-resumed.jsonl` — the
  same conversations at the end of the run. Resumed, Claude waited at its
  prompt, status idle, for the 45 s it was watched, and wrote nothing of the
  turn but a record of its own bookkeeping. A second message, pasted as the
  daemon pastes, reached it alone (nothing was put back in its input box) and
  was answered in a second or two; the turn that was cut was closed then, by a
  message of Claude's own with no model behind it (`"model": "<synthetic>"`,
  `No response requested.`), written just before the second message.

Made 2026-10-18 from a field record, not a live run, and not by the scrub tool:

- `restart-left-a-queued-notification.jsonl` — T-442 (poker-lab, ConsensFlow
  3.0.4, Claude Code 2.1.302, the worker `@artemis-silver-thistle`). Made from
  a content-free copy of the record (each record's type, subtype, origin,
  timestamps, uuids, parent links, queue operations and the ConsensFlow headers
  it held) by a script that gave every record made-up words in the place and
  form Claude writes them; the keys and values of the restart's three records
  (the close of the cut turn, the `SessionStart:resume` hook's run, the two
  prompts after it) were read off the real record. It keeps three runs of the
  record's 1,444 lines, in order: T-442's brief and its first calls (lines
  0–35), the end of a turn and m-2078 (504–544), and 18:29 to the end
  (1100–1443), so a parent before a cut is not in the file. The window was closed
  at 18:30 with the notification of a job ConsensFlow had ended still queued
  (`bqehk4ozo`, failed) and a notification's turn cut; the chief's reopen
  resumed it at 18:30:59, and Claude closed the cut turn (`No response
  requested.`), re-notified the job (stopped) and took m-2081, whose turn ended
  at 19:25:10.

A new Claude version that writes these otherwise is a new fixture, not an edit.
