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
