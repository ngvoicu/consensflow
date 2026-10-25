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

Taken by hand on 2026-10-16 from a real transcript of an interactive Claude Code
session (`entrypoint: cli`) on the owner's Mac, read only, and scrubbed as the tool
scrubs (the session id is `$SESSION`, the folder `/work/app`, and the folder in the
paths of the jobs' output `-work-app`); not every record of the transcript, only the
ones about its background jobs, each as Claude wrote it (the attachment without the
two renderings of its text Claude keeps for its own display, `rendered` and
`renderedInHumanTurn`):

- `job-notifications.jsonl` — Claude Code 2.1.277 and 2.1.292. The start of job
  `bmh4uwmeq` (its tool result's `backgroundTaskId`), the notification of its end
  queued while a turn was under way, taken back from the queue (`remove`,
  `"reason":"absorbed_mid_turn"`) and brought into that turn as an attachment
  (`queued_command`, `"commandMode":"task-notification"`); the start of job
  `bey3qyuly`, and the notification of its end queued at rest, taken
  (`dequeue`) and written as the user's record of the turn it begins, from the
  system (`promptSource`, `turnOrigin`, `origin.kind`); and, after a session was
  resumed (2.1.292), one notification that names two jobs of the session before it,
  stopped, written with `origin.kind` and no `turnOrigin`.

Taken by hand on 2026-10-16 the same way, from real transcripts of `claude -p` sessions
(`entrypoint: sdk-cli`) on the owner's Mac, read only, scrubbed as above (the session id,
the folder, the branch `main`, and the attachment without its two renderings); and one
notification no log holds, written from Claude Code 2.1.301's own source:

- `job-ends.jsonl` — how a job ends, by its `<status>`, and a notification of one that
  runs. The end of job `b75v7kq3r` (2.1.296, `failed`), queued while a turn was under
  way, brought into it as an attachment and taken back from the queue; the end of job
  `bn1zf798r` (2.1.300, `killed`, with the `<note>` Claude Code adds of a job it stopped
  at its time limit), queued at rest, taken and written as the user's record of the turn
  it begins (`origin` with a `producer` and a `runId`, `turnOrigin`, `turnPosition`). And,
  in the same records as that last end, the notification 2.1.301 sends of a background
  command that has written nothing for 45 seconds after a line that looks like a question:
  it names job `bw7q3xk2m`, says it "appears to be waiting for interactive input", has
  **no `<status>`**, and goes on after its close with the command's last output and what
  to do. Its words are those of the function that writes it (the watchdog of 2.1.301's
  bundle: a notification of `taskId`, `toolUseId`, `outputFile` and `summary`, no
  `status`, `Background command "<description>" appears to be waiting for interactive
  input`, then `Last output:`, and `The command is likely blocked on an interactive
  prompt…`, queued as the end of a job is, `mode: "task-notification"`); the records it
  is in are 2.1.300's, with the ids made up (`c0ffee00-…`, `0mvderived-…`). The command
  runs on: no end of it is written. A real one, taken from a log, replaces it.

Written on 2026-10-17 from Claude Code 2.1.301's own source, as no log holds one:

- `job-orphans.jsonl` — the summary a resumed 2.1.301 process sends of the background
  shells the process before it left with no record of their end, when there are more than
  twenty: 21 orphans (`orph01` to `orph21`) and one shell it finds still running
  (`live22`). Its words are those of the functions that write it (`Vi`, which sorts the
  shells into orphans and live ones, and `Jt`, which writes the notification: the first
  `rt = 20` ids, then `__orphan_summary__:shell`, then `__orphan_summary_live__:<id>` of each
  live one, `<status>stopped</status>`, a summary that says `First 20 task ids: …`, and the
  note 2.1.292's summary has), queued, taken and written as the user's record of the
  system's, transcript-only (`queueTranscriptOnly`, as 2.1.292's summary is; 2.1.301 queues
  it with `shouldQuery: false`); the records are in 2.1.292's shape (`job-notifications.jsonl`),
  with the ids made up (`c0ffee00-…`, `0mvderived-…`). A real one, taken from a log,
  replaces it. The reader ends the twenty it names but the one it marks running, and
  nothing it does not name (turn-end's smallest rule, 2026-10-21).

Kept 2026-10-21 by `cargo xtask live monitor-wait --fixture <file>` (Claude Code
2.1.305, on Sonnet, with none of the owner's settings and no MCP server), which writes
the window's record as the reader reads it: the records of the user's, the assistant's,
the system's and the queue's, and the attachments that bring a notification into a turn;
the fields the reader reads and few others (no `cwd`, no `gitBranch`, a tool result's
fields that start and stop jobs and no output); the session id `$SESSION`, the window's
folder and Claude's folder of its temporary files `$WORK`, the owner's home `$HOME`. The
user's and the assistant's words stay as written:

- `monitor-wait.jsonl` — T-442's shape. The agent starts a command in the background
  (`backgroundTaskId`), loads Monitor (`ToolSearch`) and arms it on the command's output
  (`toolUseResult` `taskId`, `timeoutMs: 40000`, `persistent: false`), and ends its turn;
  the monitor's first event is queued before that turn ends. Each event is a notification
  with `<task-id>`, `<summary>` and `<event>`, and **no `<status>`**, which Claude Code
  begins a turn with (`promptSource: system`, `turnOrigin: task_notification`, `origin`
  with a `producer`); the command's end is a notification with a `<status>completed`;
  the monitor's expiry, 40 s after it was armed, is an `<event>` of Claude Code's own
  (`[Monitor expired after 40s with 3 events delivered. …]`), with no status. Then a
  second message: a command started in the background and stopped with TaskStop, whose
  result names it (`task_id`, `task_type: local_bash`, `message: Successfully stopped
  task: …`), and no notification follows.

A new Claude version that writes these otherwise is a new fixture, not an edit.
