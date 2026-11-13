# What Node's engine said: the texts

`text.json` holds the texts of the engine as Node's modules answered them: how a
message reads in its recipient's pane (delivery text), the handoff a new chief
gets, and the role instructions each role is given. It was recorded from Node,
which is gone, and it is fixed: nothing records it again, and a change that
moves a text is made in the file by hand, on purpose, with the reason in the
commit. The recorder ran Node's modules and went with them; it is in the flip
release `v3.0.0-alpha.82`, and in `21297242`, the commit before it was deleted.

`../text/` plays it: `goldens_delivery.rs`, `goldens_handoff.rs` and
`goldens_roles.rs` each take their tables.

## Changed by hand after Node

The chief's text, in each of the twelve `roleInstructions` rows that hold it, has
three passages Node's did not: the line of its commands for `cf task add --test`, the
tester in the sentence that names what each role of a tier does, and the line of its
Reviews that says a tester checks work by running it. They were changed by hand, in
those words alone, when the tester was added (`skill/core/chief.md`), and no row of the
tester's own text was added: `../text/roles.rs` holds that.

The same twelve rows say "a crash" in the sentence about a window lost, where Node's
said "a restart or a crash" (`skill/core/chief.md`): a restart pauses no task any
more, for the project it suspends comes back by the human's Resume, which opens the
windows that had work. Changed by hand, in those words alone.

The same twelve rows say, after the heredoc example, "Write a long brief to a file with
your file tool and pass it with `--file brief.md` instead: a shell can cut a long heredoc"
(`skill/core/chief.md`): a chief's heredoc that a shell cut sent the board a brief cut at
about 8.1k characters (the field report of 2026-10-15, B6). Changed by hand, in those
words alone, on 2026-10-16.

The same twelve rows end the chief's opening with "you never type into another window or launch
agents to do the work" (`skill/core/chief.md`), where Node's said "or launch agents": the chief's
own rule about its harness's subagents allows them for searching and reading, which the opening
forbade (the review T-1447 of T-1435, finding 3); the sentence before it lost "here" ("the work
the human gives you is yours to plan"), which "here in your terminal" below it says, to pay for the
words. Changed by hand, in those words alone.

The members' text (worker, advisor, reviewer and image designer), in each of the 27
`roleInstructions` rows that hold it, has one sentence Node's did not, the last but one of
its paragraph about a window that is for one task: "Your answer ends your turn's work:
anything you left running is stopped then, so wait for what your result needs"
(`skill/core/staff.md`). It was changed by hand, in those words alone, on 2026-10-13, when
a worker's answer became its task's result at once and what its turn left running began
to be ended with it: an agent was seen leaving a check running and answering, which that
rule now ends.

The same 27 rows have one paragraph Node's did not, after the one about a removal's paths:
"Others work in this folder too, and not all of their work is committed. Never throw away
a change you did not make: no `git checkout` or `git restore` that overwrites a file
(`git restore --staged` of your own change is fine), and no `git reset --hard`,
`git stash` or `git clean` over work that is not committed; take a change of yours out by
hand. Say a file is restored only once you have checked it: its diff, or what it holds."
(`skill/core/staff.md`). It was changed by hand, in those words alone, on
2026-10-15, when a worker on the owner's Windows machine overwrote a file that held
uncommitted work with `git checkout`, and other workers said they had restored files they
had not.

The chief's text, in each of the 12 `roleInstructions` rows that hold it, and the text of
the worker, the advisor and the reviewer, in each of the 7 rows each has, moved on
2026-10-31 for spec `smaller-contexts` (T-1009, `skill/core/chief.md` and
`skill/core/staff.md`), in the words of those two files alone. The image designer's 6
rows did not move, and the tester has none.
- **The chief's** Reviews: a finding blocks only when it is evidenced; the rest does not
  hold the work; a second review checks the earlier findings by number; after two reviews
  that both found blockers the chief changes the work or asks the human; no pass-or-fail
  first line is asked of a reviewer. Also the paragraph on a fresh window after about
  three rounds (in the section on continuing a window; the old task is accepted, after the
  new one is added `--before` the tasks that wait on it), the targeted checks at the end of
  item 4 of "What you do", the facts a brief states and how they are known (item 3), and
  the line on commands that go together in one shell call (the command card).
- **The members'** result begins with the outcome and what the chief has to decide, and
  puts a long log in a file outside the tracked files (worker, advisor, reviewer and
  tester). The reviewer's begins with whether the work is ready, numbers its findings,
  marks each blocking or not, and on a second review checks the earlier findings.
The rows were moved by a script that put the new text where the old stood and left the
staff table, the tiers and the note of this window's cf as they were; the test of this
folder's rows holds that each equals what the engine renders.

The chief's text, in the same 12 rows, moved again on 2026-11-01 for the same spec
(T-1012, SCX-15): item 3 of "What you do" ends with one sentence more, that every member
already has the project's rules for the staff, so a brief doesn't repeat them and says only
what its task needs. Nothing else moved. The rules themselves are in no row: a row's
project has none, and a text with none is what it was, to the byte (`with_staff_rules`).
The rows were moved by a script that put the three lines after the last line of item 3.

The chief's text, in the same 12 rows, and the reviewer's, in its 7, moved again on
2026-11-02 for the same spec after its critical review (T-1014 notes 3 and 9, T-1074), in
the words of `skill/core/chief.md` and `skill/core/staff.md` alone:
- **The chief's** paragraph on a fresh window ends "then accept the old one: its open
  findings go on in the new task", so accepting it does not read against "accepting work
  with an open blocker needs the human's yes". Two cuts of T-1009 are back in "What you
  never do": the exception "unless the human told you to do it yourself" (bullet 1) and the
  reason "the board is the only channel, and" (bullet 3). Item 6 says its brief comes back
  "once the task it needs is accepted", naming no T-3 that only the command card defines.
- **The reviewer's** "A review changes no project file", which the line on a long log in a
  file outside the tracked files no longer contradicts.
The rows were moved by a script that replaced those passages, wrapped as the files wrap
them, in each row that holds them (12, 12, 12, 12 and 7 times). The role-change budgets
moved with the longer chief text: whole to 24 rows without specs (27 before), read whole to
240 rows with specs on and 250 without (241 and 251), and to 85 and 95 with the most rules
for the staff (86 and 96).

The members' text, in the same 27 `roleInstructions` rows, moved again on 2026-11-01
(T-1061, `skill/core/staff.md`): the sentence of 2026-10-13 said "Your answer ends your
turn's work: anything you left running is stopped then, so wait for what your result
needs", and the engine had not done that since `0eec91d20` (2026-10-21) — a turn's end
ends no process: a turn whose harness record counts pending work holds its task working,
and otherwise the answer is the result and the free worker's window then closes, which
ends only what the host can tell is the window's, so a detached job can run on. The
sentence now says "Your answer ends your turn: wait for what your result needs, and
leave nothing running", in those words alone. The rows were moved by a script that
put the new text where the old stood.

The 39 rows of the role texts, the 27 members' and the twelve of the chief's that hold the role's own text
(the other chief rows read an eval's card, and hold the card), name `cf` in a different way
(`skill/core/chief.md` and `skill/core/staff.md`, T-1049, 2026-11-01). A window's shell can
find another `cf` first on its PATH (Devin's login zsh on the Mac, Claude Code's login Git
Bash on Windows, the Cloud Foundry CLI: T-972's live kit, 2026-10-31), so each command a text
teaches starts with the window's own `cf` by its full path, where Node's said `cf`: the
`cf` of the row's input, quoted as one word where a shell would read it otherwise (the rows
whose path has a space, quotes, a backtick or a `$`), and `cf` where the row gives none (the
undefined `cf` and the empty one). Two more changes follow from it. The sentence "`cf` is on
this window's PATH and its output says what happened" is now "Its output says what
happened", for it is no longer the PATH that finds the `cf`. And the section at the end,
"## This window's cf" ("Here `cf` is …; if `cf` says a command is unknown, or answers as
another program, … run … instead"), is gone from the rows that had it (those with a `cf`):
the commands say it. Changed by hand, in those words alone, as a regular edit of each text
(a command's `cf` put through the row's path, the sentence and the section), on 2026-11-01.

The members' 27 rows have one paragraph Node's did not, after the one that ends "it arrives
only once your turn has ended": "If you are a sub-agent that a member started (your prompt
came from the member's own agent, not from the chief), you are not the member: you have no
board identity, so never run `cf`; answer your parent in your final message."
(`skill/core/staff.md`). The chief's twelve rows have one Node's did not either, after the
paragraph that opens the text: "If you are a sub-agent that the chief started (your prompt
came from the chief's own agent, not from the human), you are not the chief: you have no
board identity, so never run `cf`; answer your parent in your final message."
(`skill/core/chief.md`). The `cf` in both is the row's, as in every command of the text
(T-1049, above). Both were changed by hand, in those words alone, on 2026-11-01
(T-1053): a sub-agent a window starts has the window's board token in its shell, on every
harness, and one that keeps the window's role (a Codex or Claude Code sub-agent forked with
the window's history, or one of an agent type the owner defined for Codex) acts as the
window. T-766 saw it: four of four Codex sub-agents that held a member's role and could not
answer something asked the board as the member (`cf ask`), and none of three did with the
member's sentence in that role.

The chief's twelve rows say, in item 4 of "What you do" after "any task that needs it stays
blocked", "A failed task whose work you checked is accepted too, with what you checked:
`cf task accept T-3 "what I checked"`" (`skill/core/chief.md`), where Node's said nothing of
a failed task, its `cf` the row's, as in every command of the text (T-1049, above): a
designer's session died after it had saved both final files, `cf task accept` was refused,
and the task that needed it stayed stuck until the chief cancelled it and added it again
(T-1111, the field report of 2026-11-02). Changed by hand, in those words alone, on
2026-11-02, and recorded from the engine on 2026-11-03 when it was merged onto the texts
above (T-1176). The role-change budgets moved with it: a chief's replacement is read whole
to 208 rows with specs on and 236 without (211 and 240), and to 42 and 71 with the most
rules for the staff (46 and 75).

The chief's text, in each of the twelve `roleInstructions` rows that hold it, says two
things more (T-1108, `skill/core/chief.md`): after "goes to the nearest one somebody does,
the next one up first" it says "you are told whose" (the note and the reply that say which
member's tier took a task); and its line of `cf task edit` says what the command changes
now, the brief, the tier, the tasks it needs and the tasks that wait for it: `cf task edit
T-3 "…" [--tier <tier>] [--needs …] [--before …]`, with how a list is written, its `cf` the
row's (T-1049, above). Changed by hand, in those words alone, on 2026-11-02, and recorded
from the engine on 2026-11-03 when it was merged (T-1176). The role-change budgets moved
with it: read whole to 205 rows with specs on and 234 without (208 and 236), and to 40 and
69 with the most rules for the staff (42 and 71).

The chief's twelve rows say two things more (T-1118 and T-1128, `skill/core/chief.md`): the
command card has `cf task edit|reopen|resume T-3 … --title "…"   a new title; edit takes it
alone, in any state`, its `cf` the row's, after the line of `cf task resume`; and a follow-up
given with `--after` "waits for T-3 and for the window's other done tasks to be accepted, and
goes in then", where it said a window still at work on T-3 takes it once T-3 is accepted.
Changed by hand on 2026-11-02, and recorded from the engine on 2026-11-03 when it was merged
(T-1176). The role-change budgets moved with it: read whole to 202 rows with specs on and 231
without (205 and 234), and to 37 and 65 with the most rules for the staff (40 and 69).

The members' 27 rows end the sentence "Never wait for an answer in your shell (no `sleep`, no
loop over `cf inbox`): it arrives only once your turn has ended." with "To wait for the
chief's answer, ask with `cf ask`, never in your last words." (`skill/core/staff.md`), and
the chief's twelve say, in item 5 of "What you do", that a question ending "(ConsensFlow: …
If they are its result: `cf task finish T-5`.)" is a turn's last words: answer it, or finish
the task (`skill/core/chief.md`). Each `cf` is the row's (T-1049, above). A worker's question
written in its last words arrived as its result (T-1119, the field report of 2026-11-02).
Changed by hand on 2026-11-02, and recorded from the engine on 2026-11-03 when it was merged
(T-1176). The role-change budgets moved with it: read whole to 199 rows with specs on and 227
without (202 and 231), and to 33 and 62 with the most rules for the staff (37 and 65).

The chief's twelve rows say that each task gets a window of its own, and that a member runs
any number of tasks at once (T-1134, `skill/core/chief.md`): the command card's `--after`
line says a follow-up is "only when it needs what that window already knows" and that
"reviews, parallel work and work for another role stay fresh, and start at once beside that
window's work"; "Each task gets a new window of a member of that role and tier on this
project's staff", where it said "ConsensFlow gives the task to a free member"; and the staff
section ends "Each member is a model, not a person: it runs any number of tasks at once, each
in its own window." Changed by hand on 2026-11-03, and recorded from the engine when it was
merged (T-1176). The role-change budgets moved with it: read whole to 195 rows with specs on
and 223 without (199 and 227), and to 29 and 58 with the most rules for the staff (33 and 62).

The chief's text, in the 12 `roleInstructions` rows that hold it with a staff, and the
image designer's text, in its 6 rows, moved in 3.0.7 (T-732, `skill/core/chief.md` and
`skill/core/staff.md`), in the words below alone; the 5 `teamTable` rows that hold a table
moved with the table's shape. They were written by the engine itself, as the tests now
play it, not by hand.
- **The chief's** two paragraphs: the image designer is given a precise drawing order (the
  subject and composition, the style, the exact colours and size, transparency when needed,
  each reference by path and what it is for, the output path), draws exactly that and does
  not research or choose, and a revision goes back to its window with `--after`
  (Gabriel, 2026-10-21; T-160); and the chief's model is the human's to change, with their
  harness's own switch in the chief's window, which a reopen keeps, while Switch the chief
  is for another harness or a fresh conversation, on a model from that harness's list
  (CHF-22).
- **The designer's** own text, in the six rows that hold it: its description, its
  introduction and the end of its result. It draws exactly what the order says, at the
  size and path it names, with only the references it names; it does not research the
  project or redesign what was asked; and where something it needs is missing or
  contradicts itself it asks with `cf ask` and does not guess. Its result is as it was.
- **The staff table** has no column of models (T-1234). A member is named by its model
  (T-1263), so the name says it; a member whose name holds no model (one on its harness's
  default, one that needs an edit) has the model beside its name, `zeus (Opus · high)`.
  The `role_change` sizes of the chief's text moved with it, and are measured in the
  engine's tests and docs.
- **The image designer's tier** is none (a design task has none): in the 2 `staffOf` rows
  that hold one, `osiris` reads without the tier its participant holds, and a chief's table
  says `none` for it, from a staff row that has no tier and the one role of a designer.

The role texts moved on 2026-11-11 for the read-through of them (T-1435, after T-1418 and
T-1419), in the words of `skill/core/chief.md`, `skill/core/staff.md` and the specs
section, by exact replacement in the 12 chief rows and the 27 member rows (7 advisor rows),
and the shell-dependent paragraphs by shell below. By hand, in those words alone:
- **The chief's** command card: `cf task add --advice` and `--review` say "no project file
  changed" (an advisor and a reviewer may put a long log in a file outside the project);
  `cf task edit` is for "a task before any window has its brief (open, queued, or paused)",
  the words the code uses; `cf tell` says "(queued or open: kept for its window; no answer,
  nothing to resume)"; "(never to pick one)" and "; ConsensFlow picks the member" are out.
  Item 3 of "What you do" says "Every window opens with the project's rules for the staff",
  not that every member already has them (a window already open keeps its own); the
  sentence "here in your terminal and end your turn" of "Choose the tier" and "What goes out
  goes on the board." of "What you never do" are out.
- **The members'** shared text: "A new window starts from nothing; one the chief continues
  has its earlier conversation. Either way, finish with a result that stands on its own."
  in place of "Nothing from an earlier task is here, and nothing from this one carries
  over"; `cf ask` is "a question to the chief" alone. The advisor's description and intro
  say "change(s) no project file" and its intro ends there.
- **The specs section** (the chief's rows that hold it are not in this folder; its words are
  pinned in `roles/specs/tests.rs`): "Plan specs with the Specmint skill", and what Specmint
  calls "research beyond searching and reading, or implementation" goes on the board as
  tasks for the staff.
- **The chief's section on the project's rules** says "Every window of your staff opens with
  these rules", and the two notes to a running chief of a change of them say "Windows that
  open from now on are given them (none); one already open keeps its rules".
The role-change budgets moved with the shorter text: read whole to 151 rows with specs on
(174 without), and to 15 and 38 with the most rules for the staff, 41 bytes and 57 lines to
spare for the 15 (T-1480 moved them again: the last paragraph).

The paragraphs of a text that depend on the shell of the window's commands (how a text is passed
to `cf`, and how a removal whose path a variable makes is written: the chief's input, a member's
input and its removal) are in `skill/core/shells.md`, a part for each shell, and `chief.md` and
`staff.md` name them as slots. The goldens here are played for `Shell::Posix`, the shell of
bash, zsh and Git Bash (every harness off Windows, and Claude Code, Pi and Devin on it), whose
parts are the words these paragraphs always had, so no row moved. The PowerShell parts (Codex's
and Muse's on Windows) are pinned in `tests/text/shells.rs`: their words, that a PowerShell
text holds no heredoc and no bash removal, that it is no longer than the bash text with the same
word for `cf`, and which lines differ.

The staff table (the `table` row of `teamTable` and the two chief's texts whose staff holds
an image designer) says "image designer" where Node's said `designer`: since spec new-roles
(FD-04) a frontend designer can be on the staff too, and the table says each role by its
name (`cf_ledger::roles`), so the two are never one word. Changed by hand, in that cell
alone, on 2026-11-11.

The chief's text was taught the four roles of spec new-roles (FD-06, FD-07; T-1480), in the twelve
chief's rows that hold `chief.md` and in no other row, changed by hand on 2026-11-11 with the same
exact replacements as the file: the card has a line for `--frontend`, `--architect`, `--research`
and `--integrate`, and `--design`'s line points to its paragraph; "## Choose the tier, not the
member" is "## Choose the role, not the member", which names the role and its tier where its
command takes one, and has a paragraph for the architect, the frontend designer and the
integrator, and the image designer's with the flow of a member's drawing orders; what the card
says (the commands of a note, an answer and a review's brief) and what the text said twice (the
opening's list of who gets what, What you never do's first two bullets, `--after`'s when and when
not, the session files) is said once. A window left for hours is a new task too. The role-change
budgets moved: read whole to 143 rows with specs on (168 without), and to 7 and 32 with the most
rules for the staff, 12 bytes and 53 lines to spare for the 7.

The same twelve rows say again, in What you never do, "Give a task to a member by name, or write
one with a member in mind." (T-1513, changed by hand on 2026-11-12 with the file): T-1480 had
taken the ban out to pay for the four roles' words, and what was left of it no longer banned a
brief written for one member (review T-1510). The same task has `cf inbox read` print a role
past the bound a role is read whole at in parts (`cf_base::role_read::role_parts`): it prints in
one piece to 141 rows with specs on (166 without), and to 5 and 30 with the most rules for the
staff, and in parts past that, so no staff is too large to be read.

The same twelve chief texts say a tier can be served by a backup, that a member on Vacation
takes no tasks, and that the chief still names the tier (BKV-05, T-1627). With those words,
the one-piece bounds are 139 rows with specs on and 163 without, or 2 and 27 with the most
staff rules; longer texts are read in parts as before.
