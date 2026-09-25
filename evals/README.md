# Evals — what a chief *does* with its role text

`npm test` checks what the role text says. These runs check what a real chief
does with it on a toy project: whether it puts work that can run side by side
on the board, asks advice for a hard call, sends finished work to review, and
puts the owner's decisions and its findings on the board (`cf ask --human`,
`cf note --human`) instead of a report in its terminal.

```sh
npm run eval -- --scenario six-decisions                       # chief Claude on Opus, staff Claude on Haiku
npm run eval -- --scenario simple-fix --chief codex --staff pi   # a Codex chief, a Pi staff
npm run eval -- --scenario complex-launch --chief opencode --staff claude,codex,pi,opencode,devin
npm run eval -- --scenario six-decisions --model claude-sonnet-5 --repeat 3
npm run eval:summary                                            # every report, one line each
```

**This spends real tokens and is not part of any gate.** It needs the
harnesses named logged in on this machine, and your word for the spend.

## How a run works

Everything is real but the human. The daemon, the pane host and the harnesses
are the ones the app uses (the live bench's shape). `--chief` names the
chief's harness (claude, codex, pi, opencode, devin); Claude Code and
OpenCode take the chief's model from `--model` (Opus and OpenCode's cheap
model unless given), the others run their own configured default. `--staff` names the staff's harnesses: each gives two
workers, an advisor and a reviewer on its cheap model (`evals/plan.mjs`), all
standard tier, so the daemon picks among them by its own rule and any of them
may get any task. The scenario's fixture is copied into a fixed, trusted
workspace under the Candidate's home (what Claude Code remembered about that
folder from the last run is cleared first), and the scenario's prompt is typed into
the chief's terminal. From then on the human is a script: every question the
chief puts on the board is answered by the scenario's `answers` (a pattern on
the question, free text back), else by the first option, else by the
scenario's fallback; nothing else is said. The run ends once nothing has
moved for the scenario's quiet time.

Then the ledger is read: tasks by pool and tier and how many ran side by
side, questions and notes to the human, advice and reviews, the chief's own
edits (the transcript copy's Edit and Write results, counted for a Claude
chief only), its last words, which files of the fixture the run changed or
added, the last lines of the chief's screen (why a chief said nothing: a
quota wall, a login page), and how many times the owner had to answer in the
chief's terminal: a chief that stops there, asking or proposing, instead of
asking on the board, hears the scenario's `nudge` typed there (twice at
most), so the run still shows what it does next. The
scenario's expectations are checked against those numbers; the report goes
to `evals/reports/` and a verdict to stdout. Reports are kept in git: a run is
evidence, and a later run beside it is the comparison. `npm run eval:summary`
lists them.

## Scenarios

`evals/scenarios/<id>.mjs` exports the prompt, the answers, the fallback, the
quiet time and the expectations; `evals/fixtures/<name>/` is the toy project
(`site`: a small bilingual site about burnout at work, with a planted
discrepancy: the guides say "a scale from 1 to 5", the site shows a colour and
a score).

- `simple-fix`: one wrong word on one page. Expected: fixed, nothing asked, at
  most one task, no advice.
- `six-decisions`: from the btb transcript of 2026-09-25. A new page to write
  and translate, choices only the owner can make. Expected: the owner asked on
  the board, a note, parallel work, a review, few edits by the chief.
- `complex-launch`: three things at once, a hard call, a sign-off. Expected:
  three or more tasks, parallel work, advice, a review, the owner asked, the
  discrepancy noted.
