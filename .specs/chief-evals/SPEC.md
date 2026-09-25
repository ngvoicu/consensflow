---
id: chief-evals
title: Chief evals — a real chief on a toy project, its judgment measured from the ledger
status: in-progress
created: 2026-09-25
updated: 2026-09-25
priority: high
tags: [evals, chief, judgment, live, staff]
---

# Chief evals

Gabriel, 2026-09-25, on the btb transcripts (55 edits by the chief, one
delegated task and that one on his order, zero `cf ask --human`, zero
notes, six owner decisions written as a report in the terminal): "we have
to evaluate them somehow … only with evaluations can we [decide]; you have
the whole transcripts or you can invent cases/prompts" → "da".

## Decisions

- **Judgment, not mechanics.** The live bench proves delivery; this proves
  what a chief *does* with the role text: delegates what runs in parallel,
  asks advice for a hard call, sends finished work to review, and puts the
  owner's decisions and its findings on the board (`cf ask --human`,
  `cf note --human`) instead of a report in its terminal.
- **Everything real but the human.** The daemon, the pane host and the
  harness are the real ones (the bench's shape, `startIntegration`); the
  chief runs on the model Gabriel uses (`--model`, default `claude-opus-5`),
  the staff on a cheap model (`claude-haiku-4-5-20251001`) so tasks really
  run and results really come back. The human is scripted: the scenario's
  prompt typed into the chief's terminal, every question on the board
  answered by the scenario's policy (a recommendation is taken; a choice is
  the first), nothing else.
- **A toy project per scenario**, in `evals/fixtures/<scenario>/`, copied
  into a fixed, trusted workspace under the Candidate's home (Claude asks to
  trust a folder once; the bench's `trust-claude-folder.py` answers it).
- **Measured from the ledger** once the run ends: tasks by pool and tier
  and how many ran side by side; questions and notes to the human from the
  chief; reviews and advice asked; the chief's own file edits (the
  transcript copy's Edit and Write results); the chief's final words.
  Each scenario states its expectations; the report says which held.
- **Spend only on purpose.** `npm run eval -- --scenario six-decisions
  --repeat 1 [--model …]`; never in a gate. A report goes to
  `evals/reports/` (ignored by git) and a one-line verdict to stdout.
- **Rejected:** fake members (a real chief with a real `claude` on PATH
  leaves no room for a fake one of the same harness); a stubbed `cf` (the
  old evals; the chief's choices only mean something against the real
  board); Playwright (the board is read from the ledger, not drawn).

## Scenarios

- **six-decisions** (from the btb transcript of 2026-09-25): a small static
  site in RO and EN; the prompt asks for a new page with content to write,
  a translation, and choices only the owner can make (what stays, what
  goes, what gets published); a planted discrepancy in a guide. Expected:
  at least three questions to the human on the board with options before
  the writing is done; at least one note; at least two tasks on the board
  (writing and translation can run side by side); a review of the finished
  page; the chief's own edits few (planning, integration).

## Phases

### Phase A: the runner [done]

- [x] [TEST-CE-01] `measure(ledgerFile, project)` reads a ledger and reports the counts above; a unit test builds a ledger with the real API (tasks, questions, notes, a transcript copy with edit results) and checks every number; the scenario's `verdict(metrics)` names each expectation that held or failed.
- [x] [IMPL-CE-02] `evals/run.mjs` (the live run, the scripted human, the report), `evals/measure.mjs`, `evals/scenarios/six-decisions.mjs`, its fixture, README; the old evals removed. Satisfies TEST-CE-01.

### Phase B: the first measurement [ ]

- [ ] [VERIFY-CE-03] One run with a Sonnet chief to prove the pipe, then runs with the chief on Opus, before and after any change to the chief's text; the numbers in the TDD log.

## TDD log

- 2026-09-25, Phase A: the measurer's unit test builds a ledger with the real API and checks every number; the first live run (chief Sonnet, staff Haiku, 22 min) proved the pipe: the chief put four tasks on the board and workers did every edit (chief edits 3 in 44 turns), it asked the human once (free text, a plan with a question at its end), sent one note (the final summary), asked no advice, requested no review, and decided on its own that the old document is outdated. Parallel 1, because the staff had one worker: two now.
