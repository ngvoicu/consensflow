# What Node's CLI said: cf's recordings

These files were recorded from Node's CLI, which is gone, and are fixed. Nothing
records them again: they hold the Rust `cf` to what Node's CLI said and did, and a
change that moves one is made in the file by hand, on purpose, with the reason in
the commit. The recorders ran Node's CLI and went with it; they are in the flip
release `v3.0.0-alpha.82`, and in `21297242`, the commit before they were
deleted.

| File | What Node's CLI did | Read by |
|---|---|---|
| `cli.<platform>.json` | the standalone verbs (`--help`, the version, `setup`, `doctor`): every scenario played against Node's CLI, `darwin` for macOS (and any system but Windows), `win32` for Windows, 77 cases each (Node's had 374: the cases of `catalog` and `agent` went with the catalog and the saved agents, and the two of `setup` and `doctor` that stopped at a file of agents they could not read, harness-models): what it printed, said on the error output and exited with, and the files it left. `FORMAT.md` says what is in them | `../cli_goldens/` |
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

## The usage after the four roles of spec new-roles

The same prints have the entries of `--frontend`, `--architect` and `--research` after the
worker's and of `--integrate` after the tester's, in the order of `cf_ledger::roles`, and
the two sentences name the four in their parentheses (`or, with no tier, --frontend,
--architect, --research, --integrate or --design`). Changed by hand, in those words alone,
on 2026-11-11, with the daemon's traces of the same words (`cf-board-019`, `-021`, `-028`
and `-029`).

## Tell withdrawal guidance (T-1641)

The six usage prints say a pause and finish can withdraw and name told words;
the four prints with the chief's tell also say when it is read unless withdrawn.
The working-tell reply names the same possibility. Changed in those lines alone.

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
  cases) has a last line, `Sent: <n> characters, ending "<its last words>".` (B6; it begins `T-3: sent` since T-957, below);
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
the task's result for a paused task as well as a working one, since a pause with words (`cf task pause T-n "…"`) pauses a task and
the window's last complete answer, written after it, would otherwise reach nobody. The
usage's entry says it, `take what a working or paused task's worker said last as its
result`, in the six prints of `board.json` that hold it (`cf`, `cf help`, `cf --help`, `cf
-h --json`, `cf task --help`, `cf task help`): each is what the built `cf` prints from
`crates/cf/src/board/usage.txt` (`task_usage()` for the two task prints), written back by a
script from the binary's own output, and nothing else in them moved. The daemon's traces
hold no `cf task finish` line (`cf-board-028` and `-029` are departed for that, and are not
played), so none was changed.

T-1119 (3.0.7-alpha.2): a waiting task too, the one whose window asked a question and ended its
turn, or whose last words ConsensFlow raised as a question (`turn_end::asks` of the engine): the
usage's entry now reads `take what a working, waiting or paused task's worker said last as its
result`, and the same six prints of `board.json` hold it.

## `cf task accept` of a failed task

Board task T-1111 (3.0.7-alpha.2): the chief accepts a failed task whose work it checked,
with the words of what it checked, which are the task's result (a designer's session had
died after it saved both final files, and `cf task accept` was refused). The usage has a
line for it after `accept|cancel`, `cf task accept T-3 "…"  accept a failed task you
checked; your words are its result`, in the six prints of `board.json` that hold it (`cf`,
`cf help`, `cf --help`, `cf -h --json`, `cf task --help`, `cf task help`): each is what the
built `cf` prints from `crates/cf/src/board/usage.txt`, changed by hand in those words
alone, and nothing else in them moved.

## The usage after `cf task edit` took a tier, needs and before

The entry of `cf task edit` in the usage (the six prints of `board.json` that hold it, `cf
task --help` among them) is four lines since T-1435 (three before): `cf task edit T-3 "…"
[--tier <tier>] [--needs T-4,T-5|+T-5|-T-5] [--before …]`, and what it changes, the brief,
the tier, the tasks the task needs and the tasks that wait for it, of a task before any
window has its brief (T-1108; "no window has yet" before T-1435): a list
replaces, `+T-5` adds, `-T-5` removes. Changed by hand, in those words alone, as
`crates/cf/src/board/usage.txt` has it.

## The usage after a task took a new title

Board tasks T-1118 and T-1128 (3.0.7-alpha.2): `cf task edit`, `reopen` and `resume` take
`--title "…"`, and an edit takes it alone, in any state. The usage has two lines for it after
`cf task resume`, `cf task edit|reopen|resume T-3 … --title "…"` and `a new title, in any state
(else the brief's first line); edit takes it alone`, in the six prints of `board.json` that
hold the usage: each is what the built `cf` prints from `crates/cf/src/board/usage.txt`,
changed by hand in those words alone.

## The last line of a reply names its task

Board task T-957 (3.0.7, from T-689): the line that ends the reply of `cf task add`, `reopen`,
`resume`, `edit` and `cf tell` begins with the task, `T-3: sent 24 characters, ending "…".`,
where it began `Sent:`. A chief that read only that last line (a harness shows a command's
output cut, `crates/cf/src/board/cut.rs`) did not learn the number of the task it had just
added. The ten cases of `board.json` that carry the line were changed by a script from each
case's own first word (`T-3 is on the board …` gives `T-3`), and nothing else in them moved.
The daemon's traces that hold the line (`cf-board-004`, `-008`, `-009`, `-010`, `-011`, `-014`,
`-015`, `-016`, `-017`, `-020`, `-021`, `-022`) are all departed from Node's already, for other
words (`DEPARTED` in `crates/cf-daemon/tests/api/main.rs`), so they are not edited.

The line of a task given to a window says what it waits for (`T-7 [open] blocked by T-1, T-2 ·
@chief ← @chief: …`), as the line of a task for a tier did (T-957, from T-798). No case of
`board.json` prints such a line, so none changed; the daemon's trace that does (`cf-board-021`) departs
from Node's already (`crates/cf-daemon/tests/api/main.rs`), and is not edited.

## `cf task add --after` says the task may wait for the chief's word (T-957, the field issue of 2026-10-29)

The reply of `cf task add --after T-3` ends, before the `T-6: sent …` line, with ` If that window
cannot go on in its conversation when T-6 starts, T-6 waits paused for your word (cf task edit T-6,
then cf task resume T-6).`: a follow-up whose window closed with no proof its program was gone is
held for the chief's word (`crates/cf-engine/src/ownership.rs`) instead of going to a window of its
own, which would not remember the task it follows. The board cannot say at the add whether that
window will be proven gone (the host's word on its exit comes to the engine, after the accept), so
the sentence is the same for every follow-up. The one case of `board.json` that adds one was changed
by hand, in those words alone; the daemon's traces hold no `--after` add.
## Without the catalog and the saved agents

`cf catalog` and `cf agent add|list|edit|remove` went with the bundled list of models and the
human's saved agents (spec `harness-models`, T-1187): a member is a harness, a model it lists,
an effort and a tier. `tools/move_harness_models_goldens.py cli` took their 293 cases out of
each `cli.<platform>.json`, their lines out of every usage printed (and the words of `ui` and
`doctor` for them), and the count of saved agents out of what `setup` and `doctor` print; the two cases of
`setup` and `doctor` that stopped at a file of agents they could not read went, as neither
reads one.

## The usage after the read-through of the role texts

Changed by hand on 2026-11-11 (T-1435), in the 6 prints of `board.json` that hold them and
in `crates/cf/src/board/usage.txt`, in these words alone: `cf task add --advice` and `--review`
end "no project file changed" (they were "no file changed"), `cf task edit` is "for a
task before any window has its brief (open, queued, or paused)", on two lines as before, where
it was "a task no window has yet", and `cf task add --tier` ends "work for a worker of that
tier (--purpose for critical work)", without "; ConsensFlow picks the member" (the chief's
command card lost the same words).

The first line of the footer that teaches a text from standard input reads, since T-1435, "Any
"…" can be - to read the text from standard input, as written (not PowerShell: use
--file):", where it ended "as written:" (the heredoc is not PowerShell's; the
footer line about a file, and the commands it names, did not change). Changed by hand in the
4 prints of `board.json` that hold the footer.

On 2026-11-13 (T-1609), the four prints of board usage gained the member's
`cf tell "…"`, the note line names the human, and the footer's character count
is the chief's tell. The stdin note case is now a member tell; it, the chief's
numbered tell and the two empty commands first read `/api/whoami` for the calling
role. Their existing posts, output and refusals are unchanged.
