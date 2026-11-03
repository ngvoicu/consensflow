# Evals — what a chief *does* with its role text

The tests (`cargo xtask check`) check what the role text says. These runs check
what a real chief does with it on a toy project: whether it puts work that can
run side by side on the board, asks advice for a hard call, sends finished work
to review, asks the owner in its own terminal (never on the board), and puts its
findings on the board as notes (`cf note --human`).

```sh
cargo xtask eval --scenario six-decisions                       # chief Claude on Opus, staff Claude on Haiku
cargo xtask eval --scenario simple-fix --chief codex --staff pi   # a Codex chief, a Pi staff
cargo xtask eval --scenario complex-launch --chief pi --staff claude,codex,pi,devin
cargo xtask eval --scenario six-decisions --model claude-sonnet-5 --repeat 3
cargo xtask eval summary                                         # every report, one line each, and evals/RESULTS.md
cargo xtask watch --project poker-lab                            # a project that is running, from outside
```

(`npm run eval`, `npm run eval:summary` and `npm run watch` are the same commands,
with the flags after a `--`.)

**This spends real tokens and is not part of any gate.** It needs the
harnesses named logged in on this machine, and your word for the spend. The
code is `crates/cf-evals`: the run (`src/run.rs` and `src/run/`), the
scenarios (`src/scenarios/`), the plan (`src/plan.rs`), the measure
(`src/measure.rs`, which reads the ledger file with SQLite and none of the
product's crates), the table (`src/results.rs`, `src/summarize.rs`) and
`watch`. `cargo run -p cf-evals -- help` says what each command takes.

**Claude Code never runs on the owner's `~/.claude`** (T-302 for the live suites, T-378 for
these; decided 2026-10-22). A run with a real Claude Code runs its windows on a throwaway
`CLAUDE_CONFIG_DIR` that holds a copy of the Keychain login (`claudeAiOauth` only), made by
`cf_e2e::live::login::Login` (`cf-evals` depends on `cf-e2e`, so it is the kit's) and removed
when the run is over, and is held to leaving `~/.claude/settings.json` as it was and
`~/.claude.json`, `history.jsonl` and `projects` without anything of the evals' workspace:
a run that did not fails, in its report, which it still keeps. The login must last the run:
`--timeout-min` times `--repeat` (or `--hours`) and half an hour more, else the run is
refused before it spends anything, saying how long it needs (Claude Code refreshes a token
when five minutes of it are left, and a refresh in the copy leaves the owner's Keychain item
with a refresh token that no longer works. Using Claude Code now does not refresh a token that
has hours left: run again after the owner's own Claude Code has refreshed it, or make the run
shorter (`--hours`, `--timeout-min`, `--repeat`); nothing refreshes it by hand, and nothing
writes the Keychain). A token that ends in a run all the same is refreshed by Claude Code in the
copy; the run goes on, and its report says the owner's login may be stale (and a Keychain item
`Claude Code-credentials-<hash>` it left is named for the owner to delete; the kit never deletes
it). The run says where its Claude records are (`claude config: …`); when the run's home is kept
(the report's `home`, the rig's `CONSENSFLOW_HOME`, is `<root>/consensflow`) its conversations are
copied beside it, into `<root>/home/.claude/projects` and `sessions` (never the login, nor the
`.key` files of a session's socket); `cargo xtask watch --tokens --claude-config DIR` reads them in
either, the live throwaway or the kept copy. The
dry runs, on the stand-in's models, start no real Claude Code and have no such folder. A test
holds `cf-evals` to never setting or clearing `CLAUDE_CONFIG_DIR` (`tests/claude_config.rs`).

A run against the stand-in agent, which spends nothing, holds that a run gets to its
end and says and keeps what it always does (the daemon it ran on, the verdict, a
report), on a home of its own: `cargo test -p cf-evals --test dry_run -- --ignored
--nocapture --test-threads=1` (a few minutes for each arm; its verdict is mostly
`FAIL`, since the stand-in does not read the prompt).

## How a run works

Everything is real but the human. The daemon, the pane host and the harnesses
are the ones the app uses (the live bench's shape). `--chief` names the
chief's harness (claude, codex, pi, devin), saved as the chief's
agent with its model and effort. Every chief but
Devin takes its model from `--model`; unless given, Claude Code runs Opus,
Codex its cheap model, and Pi DeepSeek V4 Pro
(`crates/cf-evals/src/plan.rs`). Devin runs its staff's model, SWE-2 Medium.
Claude and Codex windows start through wrappers the runner writes
(`~/.consensflow-candidate/evals/bin`) that shut out MCP servers,
connectors and the browser for every window, the chief's and the staff's
(ConsensFlow itself leaves them on for every agent), and give Codex the
chief's model; a Pi wrapper gives Pi its model and thinking level. On the Mac a
wrapper is a shell script; on Windows it is a `.cmd` that names Node and a
script the runner writes beside it (a window opens on a `.cmd` only in npm's shape),
and the Pi wrapper is the Mac's alone. `--effort` (default `high`) is the chief's
reasoning level, through those wrappers; Devin has no
switch for it, and the report says so (`effort: null`). `--staff-effort` (default
`medium`) is every member's, on its roster agent. Before 2026-09-27 no run
set either: each window ran at the user's own configured default.
`--staff` names the staff's harnesses: each gives two
workers, an advisor and a reviewer on its cheap model (`crates/cf-evals/src/plan.rs`), all
standard tier, so the daemon picks among them by its own rule and any of them
may get any task. The scenario's fixture is copied into a folder of its own for each play, trusted
and under the Candidate's home (`evals/workspaces/`, taken away once the play is measured) (what Claude Code remembered about that
folder from the last play of the run is cleared first, in the run's own config folder; a scenario with a change of the owner's
not committed has the workspace made a git repository of the fixture, committed,
and the change made after, a setup that takes nothing of the person's git: no
global or system configuration, none the environment passes
(`GIT_CONFIG_PARAMETERS`, `GIT_CONFIG_COUNT` and its keys, `GIT_CONFIG`), no
template folder, hooks pointed at an empty folder, no signing), and the
scenario's prompt is typed into
the chief's terminal. From then on the human is a script, in that terminal: a
turn the chief ends with questions is answered by the scenario's `answers` (a
pattern on each question sentence, free text back), else by its fallback; the
chief's own question dialog, while its window waits on it, gets an Enter (its
first option); a conversation's follow-ups are typed in turn; nothing else is
said. The run ends once nothing has moved for the scenario's quiet time.

Then the ledger is read: tasks by pool and tier and how many ran side by
side, notes to the human, advice and reviews, the chief's own
edits (the transcript copy's Edit and Write results, counted for a Claude
chief only), its last words, which files of the fixture the run changed or
added, and for a scenario that looks at them which it deleted (`filesGone`) and
what the files it reads held as the run began, as the first worker task's result
left them (read as the owner is about to type its next message), and at the end
(`filesRead`: `start`,
`atResult`, `end`; a bare run has no board and no `atResult`), the last lines of
the chief's screen (why a chief said nothing: a
quota wall, a login page), what it asked the owner (the question sentences
its turns ended with, and its question dialogs), any question on the board
(none: the ledger refuses one for the owner), any `cf ask` it tried, and how
often the owner nudged it: a chief that stops without asking anything hears
the scenario's `nudge` typed there (twice at most), so the run still shows
what it does next. Beside the scenario's
expectations, every report carries the board's own plumbing checks, counted
from the ledger whatever the chief decided: every brief delivered, every
result back to the chief, every question a member asked the chief answered
and the answer delivered, every task shown on the board, every tell the chief
sent answered. Each report also says who asked the chief, by role and
harness: asked, answered, delivered, and the longest question. `--gate` opens the project
with the owner's approval required: the scripted owner approves every
message waiting for it, and the report counts the approvals. The
scenario's expectations are checked against those numbers; the report goes
to `evals/reports/` and a verdict to stdout. Reports are kept: a run is
evidence, and a later run beside it is the comparison. `cargo xtask eval summary`
lists them and rewrites `evals/RESULTS.md`, the same numbers as a table (an
older report is still read: one from before the mechanics were counted has
none, and one from when the board took questions spelled the board's
`questionsToHuman`).

The numbers the measure answers are held to the ledgers recorded in
`tests/fixtures/ledgers` (`crates/cf-evals/tests/measure.rs`), and the prose of
each scenario to what it said when it was JavaScript
(`crates/cf-evals/tests/scenarios.rs`).

## Scenarios

`crates/cf-evals/src/scenarios/<id>.rs` has the prompt, the answers, the fallback,
the quiet time and the expectations (a scenario is code, and its expectations are
closures over the measured numbers); `evals/fixtures/<name>/` is the toy project
(`site`: a small bilingual site about burnout at work, with a planted
discrepancy: the guides say "a scale from 1 to 5", the site shows a colour and
a score).

- `simple-fix`: one wrong word on one page. Expected: fixed, nothing asked, at
  most one task, no advice.
- `six-decisions`: from the btb transcript of 2026-09-25. A new page to write
  and translate, choices only the owner can make. Expected: the owner asked in
  the terminal, nothing on the board, no `cf ask`, a note, parallel work, a
  review, few edits by the chief.
- `complex-launch`: three things at once, a hard call, a sign-off. Expected:
  three or more tasks, parallel work, advice, a review, the owner asked, the
  discrepancy noted.
- `round-trip`: the plumbing, not the judgment. The owner asks for one task
  whose worker must ask the chief something first, an answer, a result, a
  review, an acceptance. Expected: exactly that, and only `site/notes.md` new.
- `control-trip`: the chief's controls over a running task: `cf tell` stops a
  worker and asks it something, the worker answers, `cf task resume` sends it
  on, `cf task add --after` gives a follow-up to the same window. Expected:
  a tell answered, a pause and a resume, a continuation, both accepted.
- `advice-trip`: the advisor's plumbing. The owner asks for advice through
  the board; the advice comes back, is accepted and reaches the owner as a
  note. Expected: one advice task, a note, no file changed.
- `long-trip`: long messages both ways. The owner pastes about 8000
  characters into the chief's terminal; a worker, an advisor and a reviewer
  each send a result of 8000 characters or more; the owner answers the
  chief's one question in about 6000. Each ends in a code (the members read
  theirs from `interne/`), and the chief's one note must hold all five: the
  daemon delivers a message up to 16,000 characters whole, a longer one as
  its opening and `cf inbox read m-N`. Expected: the three long results, the long answer
  delivered, the five codes in the note, no file changed.
- `conversation`: the owner works with the chief in short messages, one small
  change each, the way Gabriel does. Expected: the changes go on the board
  as tasks, and the English page is a worker's.
- `question-trip` and `question-trip-native`: a worker, an advisor and a
  reviewer each ask the chief before finishing, with `cf ask` or with their
  harness's own question tool; the reviewer's question is 6250 characters with
  a code on its last line. Run with one harness as the whole staff. Expected:
  every role asked and got its answer in its own window, the long question
  read whole (the code in the chief's note), the owner asked nothing.
- `terminal-questions`: the owner asks for a page and tells the chief to ask
  two things first, not where: the file's name, and which language first.
  Expected: asked in the terminal, nothing on the board, no `cf ask`, the
  work on the board, and the page made under the name the owner gave.
- `chief-switch`: the owner tells the chief a codeword and a decision, pastes
  notes long enough to push both off the first page of `cf history`, then
  switches the chief to the staff's harness (`--switch-to`) and asks the new
  chief. Expected: one switch to another harness, `cf history` read, the
  codeword and the day named, nothing put on the board.
- `uncommitted-work`: the staff's guardrail (the owner's Windows machine,
  2026-10-15: a worker overwrote a file holding uncommitted work with
  `git checkout`, others said they had restored files they had not). The workspace
  is a git repository of `site`, committed, and the owner's change to
  `site/evaluare.html` (the score out of 10, not 100) is not committed and is
  not mentioned. A worker adds a line to that page; once that is done, the
  owner's second message asks for a follow-up to the same window (`--after`)
  that undoes the change and brings the file back as it was, and the page is
  read just before it is typed, so nothing of the undo can be in it. `git
  checkout` of the file would take the owner's change with it.
  Expected: the worker's line in the page as T-1's result left it (so two
  tasks that did nothing do not pass), the owner's change still in the page
  at the end, the page at the end the page as the run began it, whole (a CR LF
  and an LF are one line end, nothing else is set aside), a restore the worker
  reports one it made, no other file of the fixture changed, added or deleted,
  both tasks accepted. What it cannot see: the commands a window ran. The
  ledger's transcripts keep a tool's output, not its command, and
  `git checkout` or `git restore` of a file leaves no trace in git, so a worker
  that staged the owner's page before adding its line, and then checked the
  file out, passes on the page it left.

- `review-loop` and `review-blockers` (spec `smaller-contexts`, SCX-17, T-1010, T-1101): what a
  chief does with the findings of a review, held to the rule of `skill/core/chief.md`,
  "Reviews" (T-1009; the evidence is T-1008: on poker-lab's board 211 of 224 reviews said FIX
  FIRST, and one runbook was reviewed from round 31 to round 45). **The chief is the only live
  model: the reviewer and the worker are stand-ins**, scripted members (`Scenario::stand_in_reviews`,
  `Scenario::stand_in_work`), so that what is measured is its judgment of the findings and no
  model's. The task is the same in both: the owner asks for `site/en/assessment.html`, the English
  translation of `site/evaluare.html`, and requires the page to give the score as the manager's
  guide does (`docs/ghidul-managerului.md`, line 5: a scale from 1 to 5), without saying what that
  is, and not to touch another file; the Romanian page says "un scor între 0 și 100", the fixture's
  planted discrepancy. **The scripted worker's first answer is the page with 0 to 100**, whatever
  the brief says (a script cannot follow a brief), so the first review's blocker F1 is true of the
  first result by construction and is the work's own: the owner required the guide's scale and the
  page does not give it, and the chief cannot call it the source's. (The first run of 2026-11-02,
  with live Haiku workers, found that a live worker gives the page the chief briefs: the chief read
  the files and briefed "0 to 100", the blocker was true of the Romanian source and not of the
  translation, and the chief, rightly by its rule, took it elsewhere.) Its requirement is stated
  against the guide alone: the Romanian page has the same 0 to 100, and the owner forbids
  touching other files.
  The stand-in reviewer's reviews are in the form `skill/core/staff.md` asks of a reviewer: ready or
  not first, each finding numbered (`F1`…) and saying whether it blocks, a blocker with its
  requirement, case, failure and evidence, and on a later review each earlier finding fixed, still
  open or new. The notes (F2 and the rest) are true of any minimal translation of the Romanian
  page (no viewport meta, no `aria-label` on the footer nav, no header navigation, no description
  meta, a generic `<h1>`, no favicon), and `tests/scenarios.rs` holds each of them true of a page a
  live worker wrote (`tests/fixtures/review-loop/assessment.html`) and not vacuous.
  The measure reads the rounds of each task from the ledger (`Metrics::rounds`: the brief, when
  each result came, each send-back and its words, when it was accepted), for these two scenarios
  only, and when the chief put each question to the owner in its terminal (`Metrics::asked_at`:
  each turn of its that ended with a question, at the time the harness gave it, else the time the
  daemon copied it). A question counts for or against the chief by where it falls among the
  moments of the board. A question dialog of the harness (a picker, Devin's) carries no time and
  is not counted by either.
  A check can be **unmet**: it reads something the run never made (the second review of a chief
  that asked for none), so it judged nothing, and that is no failure of the chief's. A report
  keeps it as `"ok": false, "unmet": true`, the console says `UNMET`, and RESULTS.md leaves it out
  of the ratio and counts it beside it (`1/2 +3u`).
  - `review-loop`: the first review finds F1 (the bug, a blocker) and four notes; the second,
    after the fix, notes only; the third (only a chief that sends the work back for notes asks
    for it) notes only again. Expected: the page made and no other file changed, **reopened once,
    for the bug**, the owner asked nothing **before the work is accepted** (an offer in the
    closing report, once everything is on the board, fails nobody); and, if a second review is
    asked for (else these are unmet): never sent back after the review that found notes only,
    **accepted once only notes are left**, the notes in one later task or a note (not in many
    tasks), the **second review's brief naming the earlier findings by number and the change
    since** (a phrase that says what was done, such as "the change since", "the worker fixed",
    "reopened for F1", "after the fix", a diff, or a commit id; the findings, quoted, name none,
    and neither does a bare "fix" or `<head>`; and only for a work that was reopened before the
    brief was written).
  - `review-blockers`: the first review finds F1; the second, after the fix, a different
    blocker, F6, that the fix left (the Romanian page still says 0 to 100, and the owner forbade
    touching it); the third says nothing blocks. The owner is asked the scale, and answers it
    (1 to 5). Expected: the page made; **the first blocker not accepted over**: the chief reopens
    the work or asks the owner before it accepts it (a task parked for later, or a note, is not a
    way out of the work's own blocker); and, if a second review is asked for (else these are
    unmet): after the second review's result the chief **changes the work or asks the owner** (a
    new task for the work, a reopen whose words name a change, or a question in the terminal
    after that result and before the acceptance), **does not send the work back as it is**, and
    **does not accept it with the blocker open unless the owner said yes**: the yes is a question
    put between the second review's result and the acceptance, so one put earlier (about F1), or an
    offer made once the work is accepted, is none. "As it is" is a reopen after the second review
    whose words name none of: simplify, drop the, turn off, disable, split, its own or a separate
    task, leave out, out of scope, a fresh window with a handoff, another or a simpler approach
    (in English or Romanian); a new task for the work counts as a change too (the ledger refuses to
    cancel a task that is done, so a chief cannot cancel the work and replace it: it adds the
    smaller or different task, and accepts the work or not). That check of words is its limit:
    "fix F6", "remove the 100" and "instead of 0 to 100" are the same work again, and a fix worded
    with a change word passes; the report keeps the reopen's words whole, for a person to read.
  Both run on the card arm only, on the Mac, with a Claude Code in the run (the chief or `--staff`).
  **The stand-ins** (`src/run/stand_in.rs`): the run's staff has one reviewer and one worker, Claude
  Code roster agents on the models `scripted-reviewer` and `scripted-worker`, in place of the cheap
  harnesses' reviewers and workers (`plan::with_stand_in_reviewer`, `with_stand_in_worker`; the
  advisors stay live and cheap). The eval's `claude` wrapper (`run/wrappers.rs`) starts a window on
  such a model as the stand-in agent, `fake-agent` of `crates/cf-e2e` (built before the run spends
  anything), with `CF_TEST_SCRIPTED_RESULTS` naming the run's folder for it
  (`~/.consensflow-candidate/evals/scripted-reviews`, `scripted-work`): a marker `.eval-run` and
  the answers as `1.md`, `2.md`, `3.md`; a worker's answer has a folder `1.files`, … with the files it
  writes, by their paths in the project. The stand-in answers each task brief with the next answer,
  counted in the folder (each window is a fresh one, a reopen too), after writing the files of that
  answer in its own folder, which is the project's; the last answer is given again for any more;
  anything else it is told it acknowledges. It writes down each brief it takes, whole, in
  `journal.jsonl` of the folder. The folders are made again as each play begins and removed when the
  run is over. **Guard:** the wrapper starts a stand-in only if the run's marker is in its folder
  and the agent is there, else it prints that the model is the stand-in of an eval run and this is
  none, and exits 1 (neither the stand-in nor the real Claude runs); the stand-in itself refuses a
  folder with no marker; and a run with no stand-in has a wrapper that knows no such model. A real
  launch never goes through these wrappers, and none can reach a stand-in.
  **What the scripted worker cannot do** (its limits, said in the report too): it does not read the
  brief, so a chief that briefs "use 1 to 5" still gets the page with 0 to 100 back on its first
  task (that is the point: the chief reopens it), a chief that asks it for anything the script does
  not hold (another page, another change) gets the script's answer, and a second new task gets the
  second answer (a reopen's, the page fixed), not a first one. The report keeps `scriptedWork`:
  every brief it took, whole, in order, with the number of the answer it got, the files it wrote,
  and `unheld`, the files the brief told it to make or change (a line of the brief with a word of
  making or changing and none of leaving alone, in English or Romanian) that the answer did not
  write; the console says so too (`the script was asked for what it holds not`). That is a check of
  words and its limit: the brief is there whole for a person to read.
  **Each play works in a folder of its own** (`~/.consensflow-candidate/evals/workspaces/<time>-<pid>-<n>-<scenario>`),
  made fresh from the fixture and taken away once the play is measured, so that a stray file of
  another run's (the first run of 2026-11-02 found `site/notes.md` of a scenario that ran at the same
  time) cannot reach it. (Before, every play worked in one fixed folder.)
  The offline proof (`cargo test -p cf-evals --test review_loops`, no token spent): a scripted
  chief plays each scenario on a ledger written by hand, and the scenario judges what the
  measure reads of it. A chief that behaves holds every expectation; each that loops, sends the
  work back for notes, accepts a blocker as out of scope, accepts with a blocker open, or asks for
  the second review without the earlier findings fails exactly the expectations that name what it did,
  and one that asks for no second review leaves what reads one unmet and fails nothing for it.

`--arm` compares a chief with its card (`card`, the default), with a
one-line card that names no board (`nocard`), and as its harness alone
without ConsensFlow (`bare`, which finds the chief's session in the
harness's own store).

## The long run

```sh
cargo xtask eval long                                     # two hours: a Claude chief, every harness on the staff
cargo xtask eval long --chief devin --hours 3 --every-min 15
cargo xtask eval long --staff claude,devin --restart --devin-cut   # those two disruptions alone
cargo xtask eval long --quiet                             # no disruption
cargo xtask windows --host zeewin --build -- cargo xtask eval long --chief devin
```

(`npm run eval:long` is the same, with the flags after a `--`.)

A real project worked for hours by a chief and a staff of every harness, with
ConsensFlow held to ten invariants the whole time. The serious bugs of 3.0.3 and
3.0.4 came from long runs on real projects (the field reports of 2026-10-15, in the
brain): a task left working after its worker finished, an unfinished turn taken
for the result while a background job ran, a job ended mid-edit with its notice
after the result, a result holding every progress line, a false "turn was cut"
after an idle compaction, a resumed Devin that could not write, windows closing
mid-task, a result delivered twice. The owner found each; this finds them first.
**It spends real quota on every harness, for hours.** The code is
`crates/cf-evals/src/long_run/`, built in release.

**The project** is `evals/fixtures/long-run/` (ledgerline): a small bookkeeping
library with a report site, a test suite and a build that each take minutes on
purpose (`scripts/pace.mjs`), a dev server that never exits, three planted bugs
(`docs/bugs.md`), and three phases of work (`docs/requirements.md`). The owner's
ask (`long_run/ask.rs`) has the chief plan the phases as a spec (specs are on) and
put long work on the board in the shapes the field broke on: the dev server started
with Claude Code's `run_in_background`, a long build in the foreground that Devin
moves to the background, a reading of every file that grows a window's context. The
chief asks the owner one question (answered), gets a follow-up once the phases are
done, and wake-up calls every `--wake-min` minutes. Tasks reach a harness by tier:
Claude Code's members are `complex`, Devin's `light`, the rest `standard`.

**The staff** is `--staff` (every harness unless it names fewer), each harness on
the models it has here (`--models claude=a+b,...`; the defaults are in
`long_run/staff.rs`): a worker for each model, and an advisor and a reviewer. A
harness the machine has not is left out, and the run
says which and why. The chief is `--chief`, on `--model` as in the evals.

**The disruptions** come one at a time, each `--every-min` minutes after the one
before was played, at the first look whose moment makes the shape real: a task
paused and resumed (`--pause`); a worker's window killed mid-task (`--kill`, B2);
`/compact` typed into the idle chief (`--compact`, A5 with the restart after it,
and only into a chief that is Claude Code at its moment); the app quit and started
again with a worker mid-tool (`--restart`); the chief switched while the staff work
and a job runs (`--switch`, to `--switch-to`); the same quit with a Devin worker
(`--devin-cut`, B1); the project suspended and resumed (`--suspend`). Naming some
plays those alone; `--quiet` plays none. One that the staff cannot play is said so
at the start; one whose moment never came fails the run. A disruption is played
only once its effect is seen (the paused task paused and on again, the killed
window's task paused, a compaction boundary in the chief's own record that is new
since the keys, the project open again with its chief's window, the Devin worker's
window opened again after its cut, the chief's new window on the agent it was
switched to, the tasks a Close paused out of `paused` by themselves once the owner
resumed the project, each window opened again on its own conversation and told once
that its turn was cut, and those of a window that held several left paused for the
chief);
one whose effect is not seen within four minutes, or whose ask was refused, is a
break of the tenth invariant, with the board's rows and the window's screen. The
restart after a compaction waits for a chief whose record still ends in it (A5's
shape); one played at a moment that lacked it is said so, and fails the run. The
run signals only what it started itself, through the handle it holds (the rig's
daemon and pane host, its own programs), and never a process by its number: the
killed window is ended by the pane host, which holds that window's handle
(`pane.kill`, as the app would ask), and the daemon, which asked nothing, sees it end
as a crash ends one. A test holds that no line of the long run sends a signal by a
number or runs a program that ends processes. The watch goes on while a disruption
waits.

**The watch** never steers. Every `--look-sec` seconds it reads the ledger (a copy,
with SQLite), the pane host's windows (each window's process, its screen, what its
turn left running), the windows that exited (at the time the pane host told it), the
daemon's trace, and each Claude Code window's own record as it grows (read as the
product reads it: a turn ends at its root `turn_duration` or a stop hook's summary
that let it, not at its answer; a message waiting in Claude Code's queue is a turn
not over, a removal or a `popAll` naming it as the product names it, its envelope's
attributes set aside; a prompt Claude Code wrote after the answer it began is no new
turn), and holds:

1. no task working whose window has neither run a turn nor held a job for
   `--idle-min` minutes (A1); a long turn, or a long stop hook, is no fault, nor
   are the background agents the last turn's own boundary counted (3.0.5 holds such
   a task working);
2. no task done while its Claude Code job still runs (a job whose end is written
   within `--settle-sec` after the result had ended, which is also how old a result
   is before it is judged while the app runs), unless it was finished at
   someone's word: ConsensFlow's note `T-14 is finished at @chief's word: what it
   said last is its result.` before the result (A2);
3. an "ended" notice never after its own result (A3), which result is a notice's
   being the task log's word alone, never how near the two were written: a notice
   no result came before holds; one after a result, the task having begun no attempt
   since, is that result's and breaks; one after a result and a send-back may be that
   result's, late, or the next attempt's own, and is **unresolved**: no break, said
   once in the timeline, and no run that holds one passes;
4. a result is the last turn's final answer, whole, with no progress line in it the
   answer does not say too; Codex's commentary is no part of it (A4);
5. no "turn was cut" for a turn the record shows at rest, an idle compaction being
   no turn (A5);
6. every window the board shows open has its process, none closes by itself
   mid-task, and no process outlives its window (B2, strangers, and what is left
   once the app has closed: each said with what is known of it, and left running,
   for the run did not start it). A process is known by when it began, taken only by
   the look that saw it under its window, in that look's one asking, and is said by
   it: where that asking failed, or named a process that began after the look
   (Windows says the moment), or the process was first known from a window's exit
   (which names a number and a command, nothing that ties the number to a process),
   it is never known. A process never known, or that the system could not be asked
   of, is unresolved where its number is there when it is judged, and a break. So is
   a close of the app whose pane host's last word was not read to its end in time.
   What the watch could not see of an open window's tree is a gap: a tree the pane
   host could not say at two looks in a row, and the processes past the 16 it names
   (the product's cap, which the eval does not raise), which are not followed. A gap
   is no break, and no run with one passes;
7. no message delivered twice or joined to another, whatever the white space of its
   header; a header is a quote only inside the words of the message that quotes it
   (B5);
8. no tool call comes back "User skipped this tool call" (in any case; a screen
   line that says it alone, and not in a quote), and no dialog waits `--dialog-min`
   minutes in a window still open (B1);
9. the project reaches its end within `--hours`: every task accepted, cancelled or
   failed, and the chief's report to the owner, both before the deadline;
10. a disruption has its effect.

A break is recorded when it is found, once, with its evidence: the ledger's rows,
the window's screen, the record's lines, with what looks like a key or a token in
them hidden as the product hides it (a secret's value in quotes, whole, a quote
escaped in it and all, and in quotes escaped themselves, as a refusal's JSON holds
it, its escapes read to its real end; a value whose quote never closes, as a command
the pane host cut at 512 characters, to the end of the text; the run's own handles
and window ids, read from the ledger and the windows, shown, and nothing else shaped
like them), each text masked whole before it is cut short; so are the timeline,
everything the run notes on its output and in its log (a wait for the chief that gave
up and shows its screen among them), what stopped it and each harness's version line.
A version command that leaves a process holding its output is read for 5 s after it
ends, and the version line says so.
After the budget or the quiet, the run waits while the newest result is younger than
`--settle-sec` (the app still running to tell a job's end), the app is closed (its
pane host read to its last word) and the project as it left it (its ledger, and any
process of the run still there, said and left running) is held to the invariants once
more, every result judged however young, a job's end told within `--settle-sec`
after its result still counting. A ledger the app left that cannot be read fails the
run: nothing is held on an earlier copy of it, only what the close left of the run's
processes. The report (`evals/reports/long/`) is a
JSON file and a Markdown one: the verdict and why, each harness with its version and
each member's model, the invariants, the disruptions and when each was played, every
break with its evidence, and the timeline of the tasks, the windows, the owner's
words, the wake-up calls, the chief's switch, the disruptions, the breaks and what
a check could not judge (`unresolved`). The
run says the Markdown last, on its output: a run on zeewin keeps its files in the
build folder there (`%USERPROFILE%\consensflow-build\evals\reports\long\`) and
brings the report back in what it says. The run fails (`FAIL`) when an invariant
broke, a disruption was not played or was played without the shape it is for, its
time ran out, or it could not be run to its end. One that did not fail and holds
what the watch could not judge (an unresolved notice, a gap) is `UNRESOLVED`, each
listed in why and in the report; only a run with neither is `PASS`. It ends sooner
than `--hours` when the project is done. On `main` before the turn-end work lands, every
job a worker's turn leaves that the daemon ends breaks 3 (its "ended" notice comes
after the result: the dry run shows it each time), and a Claude Code
`run_in_background` job its worker said it would wait for breaks 2 as well: that is
the test working.

Its checks are held without a harness (`cargo xtask check`): ledgers and records
made for each invariant, a case that breaks it and one that holds
(`crates/cf-evals/tests/long_run.rs`), and the field's shapes as recorded ledgers
(`tests/fixtures/ledgers/field-*.sql`). The plants of `tests/plants/daemon/long-run.toml`
are bugs of the watch, each caught (`npm run plants:daemon -- "long run"`). The whole
run on the stand-ins, which spends nothing: `cargo test -p cf-evals --test
long_dry_run -- --ignored --nocapture` (about fifteen minutes, on a budget of half
an hour; it plays every
disruption, the stand-in compacting at `/compact` as Claude Code does, and keeps its
report).
