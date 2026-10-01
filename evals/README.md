# Evals — what a chief *does* with its role text

`npm test` checks what the role text says. These runs check what a real chief
does with it on a toy project: whether it puts work that can run side by side
on the board, asks advice for a hard call, sends finished work to review, asks
the owner in its own terminal (never on the board), and puts its findings on
the board as notes (`cf note --human`).

```sh
npm run eval -- --scenario six-decisions                       # chief Claude on Opus, staff Claude on Haiku
npm run eval -- --scenario simple-fix --chief codex --staff pi   # a Codex chief, a Pi staff
npm run eval -- --scenario complex-launch --chief opencode --staff claude,codex,pi,opencode,devin
npm run eval -- --scenario six-decisions --model claude-sonnet-5 --repeat 3
npm run eval:summary                                            # every report, one line each, and evals/RESULTS.md
```

**This spends real tokens and is not part of any gate.** It needs the
harnesses named logged in on this machine, and your word for the spend.

## How a run works

Everything is real but the human. The daemon, the pane host and the harnesses
are the ones the app uses (the live bench's shape). `--chief` names the
chief's harness (claude, codex, pi, opencode, devin). Every chief but
Devin takes its model from `--model`; unless given, Claude Code runs Opus,
Codex its cheap model, and Pi and OpenCode DeepSeek V4 Pro
(`evals/plan.mjs`). Devin runs the model its own configuration names.
Claude and Codex windows start through wrappers the runner writes
(`~/.consensflow-candidate/evals/bin`) that shut out MCP servers,
connectors and the browser for the chief too (ConsensFlow already does it
for members) and give Codex the chief's model; a Pi wrapper gives Pi its
model and thinking level. `--effort` (default `high`) is the chief's
reasoning level, through those wrappers; OpenCode's window and Devin have no
switch for it, and the report says so (`effort: null`). `--staff-effort` (default
`medium`) is every member's, on its roster agent. Before 2026-09-27 no run
set either: each window ran at the user's own configured default.
`--staff` names the staff's harnesses: each gives two
workers, an advisor and a reviewer on its cheap model (`evals/plan.mjs`), all
standard tier, so the daemon picks among them by its own rule and any of them
may get any task. The scenario's fixture is copied into a fixed, trusted
workspace under the Candidate's home (what Claude Code remembered about that
folder from the last run is cleared first), and the scenario's prompt is typed into
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
added, the last lines of the chief's screen (why a chief said nothing: a
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
to `evals/reports/` and a verdict to stdout. Reports are kept in git: a run is
evidence, and a later run beside it is the comparison. `npm run eval:summary`
lists them and rewrites `evals/RESULTS.md`, the same numbers as a table.

## Scenarios

`evals/scenarios/<id>.mjs` exports the prompt, the answers, the fallback, the
quiet time and the expectations; `evals/fixtures/<name>/` is the toy project
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
  chief's one question in about 6000. The daemon delivers a message over
  4000 characters as its opening and `cf inbox read m-N`, so each ends in a
  code (the members read theirs from `interne/`), and the chief's one note
  must hold all five. Expected: the three long results, the long answer
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
- `lead-switch`: the owner tells the lead a codeword and a decision, pastes
  notes long enough to push both off the first page of `cf history`, then
  switches the lead to the staff's harness (`--switch-to`) and asks the new
  lead. Expected: one switch to another harness, `cf history` read, the
  codeword and the day named, nothing put on the board.

`--arm` compares a chief with its card (`card`, the default), with a
one-line card that names no board (`nocard`), and as its harness alone
without ConsensFlow (`bare`, which finds the chief's session in the
harness's own store).
