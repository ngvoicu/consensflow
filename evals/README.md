# Evals — what a chief *does* with its role text

`npm test` checks what the role text says. These runs check what a real chief
does with it on a toy project: whether it puts work that can run side by side
on the board, asks advice for a hard call, sends finished work to review, and
puts the owner's decisions and its findings on the board (`cf ask --human`,
`cf note --human`) instead of a report in its terminal.

```sh
npm run eval -- --scenario six-decisions              # once, chief on Opus
npm run eval -- --scenario six-decisions --repeat 3
npm run eval -- --scenario six-decisions --model claude-sonnet-5
```

**This spends real tokens and is not part of any gate.** It needs the real
Claude Code logged in on this machine, and your word for the spend.

## How a run works

Everything is real but the human. The daemon, the pane host and Claude Code
are the ones the app uses (the live bench's shape). The chief runs on
`--model` (default `claude-opus-5`); the staff (two workers, an advisor and
a reviewer) runs on a cheap model, so tasks really run and results really come
back. The scenario's fixture is copied into a fixed, trusted workspace under
the Candidate's home, and the scenario's prompt is typed into the chief's
terminal. From then on the human is a script: every question the chief puts
on the board is answered by the scenario's policy, and nothing else is said.
The run ends once nothing has moved for the scenario's quiet time.

Then the ledger is read: tasks by pool and tier and how many ran side by
side, questions and notes to the human, advice and reviews, the chief's own
edits (the transcript copy's Edit and Write results), its last words. The
scenario's expectations are checked against those numbers; the report goes
to `evals/reports/` (ignored by git) and a verdict to stdout.

## Scenarios

`evals/scenarios/<id>.mjs` exports the prompt, the answer policy, the quiet
time and the expectations; `evals/fixtures/<id>/` is the toy project.

- `six-decisions`: from the btb transcript of 2026-09-25. A bilingual site, a
  new page to write and translate, choices only the owner can make, and a
  planted discrepancy (the guides say "a scale from 1 to 5"; the site shows a
  colour and a score).
