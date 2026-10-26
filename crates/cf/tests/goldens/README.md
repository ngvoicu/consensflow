# What Node's CLI said: cf's recordings

These files were recorded from Node's CLI, which is gone, and are fixed. Nothing
records them again: they hold the Rust `cf` to what Node's CLI said and did, and a
change that moves one is made in the file by hand, on purpose, with the reason in
the commit. The recorders ran Node's CLI and went with it; they are in the flip
release `v3.0.0-alpha.82`, and in `21297242`, the commit before they were
deleted.

| File | What Node's CLI did | Read by |
|---|---|---|
| `cli.<platform>.json` | the standalone verbs (`--help`, the version, `catalog`, `agent`, `setup`, `doctor`): every scenario played against Node's CLI, `darwin` for macOS (and any system but Windows), `win32` for Windows, 374 cases each: what it printed, said on the error output and exited with, and the files it left. `FORMAT.md` says what is in them | `../cli_goldens/` |
| `board.json` | the board commands, which a window's token makes `cf`: each case served by a scripted API, with the requests Node's board made, what it printed, its errors and its exit code. Recorded at `b54361c` from Node's `runCoreCli`, a commit that holds that code and the scripted API | `../board_goldens.rs` |

The words after each verb were recorded too, through Node's `util.parseArgs`:
`crates/cf-base/tests/goldens/args.json`.

## The usage after the tester

`board.json` holds the usage the board's commands print (`cf` with no command, `cf
help`, `cf --help`, `cf -h --json`, `cf task --help` and `cf task help`) and the
sentence `cf task add` says when it was given too little. Each names the tester,
`--test`, where Node's did not: the six prints have its entry in the list, as
`crates/cf/src/board/usage.txt` has it, and the two sentences have it in their
parentheses. Changed by hand, in those words alone, when the tester was added; the
daemon's traces of the same words (`crates/cf-daemon/tests/goldens`) were changed with
them.

## The usage after a tell could wait

`cf tell` to a task still queued, its window not up yet, keeps its words for the window
(the field report of 2026-10-15). Its entry in the usage has a third line that says so,
`(queued or open: kept for its window, to go in with or after its brief)`: added by hand
to the six prints of `board.json` that hold the usage (`cf task --help` among them, which
prints the lines that go on from the commands), as `crates/cf/src/board/usage.txt` has
it, and to the daemon's traces of the same words. It said `(queued: kept for its window, to
go in after its brief)` until T-534 (2026-10-25), when a task still open, waiting on what
it needs, was told too: its words are added to its brief.

## The usage and the replies of 3.0.6's field reports

The owner's decisions of 2026-10-16 on the field report of 2026-10-15 moved `board.json`
in three ways, each made by hand:

- the transcript's entry in the usage has a line for `--page` and `--find` (B7), and its
  tail says that `--file` reads a long text from a file, which no shell cuts, and that the
  four commands that take one say how it ends (B6): the six prints that hold the usage,
  `cf task --help` among them, where the `--file` example follows the heredoc's;
- the reply of each case that gives a window words (`cf task add`, `resume`, `tell`: ten
  cases) has a last line, `Sent: <n> characters, ending "<its last words>".` (B6);
- the daemon's traces of the same commands were changed with them.
## The usage after `cf task edit`

`board.json` holds the usage again (the six prints named above) and the sentence `cf
task` says of a verb it does not know (`unknown task command "frobnicate": use add,
list, get, done, accept, reopen, edit, cancel, pause or resume`). Each names the new
verb, `cf task edit T-3 "…"`, which replaces the brief of a task no window has been
given yet (board task T-110, 3.0.6), and the `--after` entry says one more line: while
T-3 is at work the follow-up waits for T-3 to be accepted, where Node refused it. The
six prints have the entry and the line as `crates/cf/src/board/usage.txt` has them, and
the sentence has the verb: changed by hand, in those words alone. The daemon's traces
of the same words (`crates/cf-daemon/tests/goldens`) were changed with them.
Round 2 of the same task (T-158) changed one phrase of that entry, by hand, in the same six
prints: `(open, blocked or queued)` became `(open, queued, or paused)`, since a task paused
before any window had it is editable too (the same number of characters, so the role text's
documented sizes did not move).

## The task usage without the lines of other commands

`board.json` holds the task usage twice (`cf task --help` and `cf task help`, the cases `task
usage` and `task usage on help`). Node's, and the Rust `cf`'s after it, printed under `cf task
resume` four lines that go on from `cf tell`, `cf ask` and `cf history`, whose own lines it
had left out (board task T-355, 3.0.6). Both cases lost those four lines, by hand and in
nothing else; the four prints of the whole usage keep them, where they stand under their
commands. Merged for 3.0.6-alpha.3 with `cf tell`'s third line (`(queued: kept for its window,
to go in after its brief)`, above), which the old filter printed there too: both cases are
`task_usage()` of the merged `usage.txt`, five lines fewer than before.

## `cf task list` and its idle sessions

Two things in `board.json` were changed by hand for board task T-261 (3.0.6). After two
days a board listed 141 sessions of finished tasks and buried the few that held work, so
`cf task list` now lists only the lanes with a task to look at and says "and N idle
sessions (`cf task list --all`)" for the rest; `--all` lists every lane, as Node did.

- The usage line of the command, in the six prints named above, reads `cf task list
  [--all]` and says what the flag adds, as `usage.txt` has it.
- The case `the board as JSON, keys as JavaScript orders them` (`cf task list --json`)
  carries the idle sessions under their own key, `idle`, which stands right after `lanes`.
  Its board has none, so the one change is `"idle": []` in its output.

The cases `an empty board` and `the board, waiting work and lanes` are as Node printed
them: the second one's only lane holds a working task. What the flag and the idle lanes
do is held by the tests of `crates/cf/src/board/task.rs`, not by a recording.

## Two cases changed by hand after Node

In `cli.darwin.json` and `cli.win32.json`, the cases `unknown command: -h`
(`cf -h`) and `catalog: a short option` (`cf catalog -h`) keep what Node's CLI
said (an unknown command, an unknown option; exit code 1) and carry a `kept`
record, `{why, rust}`, which the player holds the binary to in its place: the
usage of what was asked, exit code 0, nothing on the error output. They were
added by hand, in both files alike, after Node, and not recorded.

Why: every verb and sub-verb of `cf` answers `--help` and `-h` with its usage,
and reads or posts nothing. The chief's report of 2026-10-08 found `cf note
--help` posting a note that said "--help", and `cf task get --help` refused as
no task; Node's CLI, which knew `--help` after a verb for no option, refused it
too. The board's commands have no recorded case of this (Node's board answered
the word only as `cf --help` and `cf task --help`, which `board.json` holds):
`crates/cf/tests/help.rs` and `help_standalone.rs` hold the rest. The `why` of
each case says the same in its file.

## `cf task edit` takes its brief from a file

The merge of main's `cf task edit` with 3.0.6's field work (board task T-340): a new
brief is words that give a window work, so `edit` takes them as the others do, from a
file with `--file` (B6), and its reply says how they end. The usage's line of the
commands that take a file names it, `(task add, reopen, resume, edit and tell)`, and the
next says `Those five say how many characters went`: changed by hand, in those words
alone, in the four prints of `board.json` that hold that tail and in the daemon's trace
of the same words (`crates/cf-daemon/tests/goldens/cf-board-028.json.gz`).

## `cf task finish` takes a paused task too

Board task T-442 (3.0.6): the chief's `cf task finish T-3` takes what a worker said last as
the task's result for a paused task as well as a working one, since a tell pauses a task and
the window's last complete answer, written after it, would otherwise reach nobody. The
usage's entry says it, `take what a working or paused task's worker said last as its
result`, in the six prints of `board.json` that hold it (`cf`, `cf help`, `cf --help`, `cf
-h --json`, `cf task --help`, `cf task help`): each is what the built `cf` prints from
`crates/cf/src/board/usage.txt` (`task_usage()` for the two task prints), written back by a
script from the binary's own output, and nothing else in them moved. The daemon's traces
hold no `cf task finish` line (`cf-board-028` and `-029` are departed for that, and are not
played), so none was changed.
