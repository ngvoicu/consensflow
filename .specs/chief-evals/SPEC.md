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

### Phase C: every harness in every role [done]

Gabriel, after the first run: "test all the combinations of harnesses, cc,
codex, pi and opencode with cheap models, in every role from chief to
workers, advisors and reviewers; simpler and more complex cases too; keep
the tests, do not delete or replace them if they are good; free text as an
answer to questions; and devin."

- [x] [TEST-CE-04] `staffFor(harnesses)` gives each harness two workers, an advisor and a reviewer on its cheap model, all standard tier; `chiefEnvironment(chief, model)` sets the model through the environment for Claude Code (Opus unless given) and OpenCode (its cheap model unless given) and says "default" for the rest; `answerFor(scenario, question)` takes the scenario's first matching pattern (free text), else the first option, else the fallback. Unit tests, no spend.
- [x] [IMPL-CE-05] `--chief` and `--staff` on the runner, `evals/plan.mjs`, reports named by scenario, chief and staff and kept in git, `npm run eval:summary`; the `simple-fix` and `complex-launch` scenarios on the shared `site` fixture; `answers` on every scenario. Satisfies TEST-CE-04.

### Phase B: the first measurement [ ]

- [ ] [VERIFY-CE-03] One run with a Sonnet chief to prove the pipe (done 2026-09-25), one with a chief and a staff on other harnesses, then runs with the chief on Opus, before and after any change to the chief's text; the numbers in the TDD log.

## TDD log

- 2026-09-25, Phase A: the measurer's unit test builds a ledger with the real API and checks every number; the first live run (chief Sonnet, staff Haiku, 22 min) proved the pipe: the chief put four tasks on the board and workers did every edit (chief edits 3 in 44 turns), it asked the human once (free text, a plan with a question at its end), sent one note (the final summary), asked no advice, requested no review, and decided on its own that the old document is outdated. Parallel 1, because the staff had one worker: two now.
- 2026-09-25, Phase C: plan and measure tested without spend (staff roster, chief environment, free-text answers, the workspace-vs-fixture diff). The second live run (chief OpenCode on its cheap model, staff Pi, `simple-fix`, 2 min) exposed two runner faults, both fixed: the report named Opus for an OpenCode chief because `--model` defaulted to it for every harness, and the only edit count read Claude's tool text, so the chief's own fix of the page counted as no fix. Now `filesChanged` diffs the workspace against the fixture for any harness; the Claude-only count is null elsewhere. The run itself: the chief fixed the page by hand in one turn, put no task on the board, asked nothing, requested no advice or review.
- 2026-09-25, Phase B, run 3 (chief OpenCode on its cheap model, staff Pi, `simple-fix`, 144 s, 4/5): the chief found the planted contradiction (the docs say a scale of 1 to 5), asked the owner on the board which value is right, with options, although the prompt had said 0 to 10 (the one failed expectation, kept: the question blocked on what the owner had already said), fixed the page after the answer, touched nothing else, and put the finding about the docs in its last words in the terminal, not in a `cf note --human`. The pattern from the btb transcripts, reproduced on a cheap model in two minutes.
- 2026-09-25/26, Phase B, the `simple-fix` round, every harness as chief on a cheap model, each with another harness's staff: Claude Haiku 5/5 (one edit in three turns), OpenCode Muse Spark 4/5 (above), Pi Muse Spark 5/5, Devin SWE-1.6 Slow 5/5. Codex hit its usage limit on its first command ("try again at Sep 26th, 2026 2:49 PM", from its own session log); its cell reruns then. Two runner faults found on the way, both fixed and the faulty reports dropped: a Pi or Devin window reads idle the moment it opens, so the runner typed the prompt before the harness had drawn its box and the text was lost (now the runner waits for the window's output to hold still for three seconds first); Devin keeps a pasted prompt in its box and ignores the Enter that arrives in the same write (now the runner presses Enter once more when the chief is still idle five seconds after the prompt). Every report now keeps the chief's last screen lines, which is how the Devin fault was seen.
- 2026-09-26, Phase B, the `six-decisions` round, every harness as chief on its cheap model with the other three harnesses as staff (Codex out of quota). A first Haiku run was dropped: the chief opened with what the Sonnet chief had saved the day before in Claude Code's auto-memory for the fixed workspace ("built and accepted, three flags open"); the runner now clears that memory before every run. Then, clean: **Haiku 1/7** (224 s, 9 turns): it wrote a full plan in its terminal and ended with "what is the next step?" there, 0 questions, 0 tasks, 0 notes on the board, the btb pattern reproduced. **Pi Muse Spark 5/7** (29 turns): 6 tasks, 4 side by side, 1 advice, 1 review, 1 question with options and 3 notes on the board, 2 files changed, all in four minutes; then its model connection dropped (the socket sat in CLOSE_WAIT) and Pi never gave up on the request, so the window read "working" for the remaining 36 minutes and nothing more could be delivered to it. **OpenCode Muse Spark 1/7** (6 turns): one well-formed question with options on the board, answered by the script, then "working" for 38 minutes with no request open and no final words; its screen could not be read (OpenCode draws with sequences the capture does not strip). Two product findings from this: a window that stays "working" without output for a long time has no watchdog, and OpenCode's screen needs a different capture. One scenario fault fixed: the answer patterns put "publish" first, so the OpenCode chief's question about the HR document got the publishing answer; the specific subjects now come first. **Devin SWE-1.6 Slow 1/7** (202 s, 5 turns): a plan and a question in its terminal, like Haiku, nothing on the board; the daemon never showed the Devin chief "working" while it typed (its activity comes from its task record, and a chief has none). So the runner now answers a chief that stops in its terminal with nothing on the board (the scenario's `nudge`, twice at most, counted in the report), so a run shows what such a chief does next; the next round runs with it, Codex included once its quota returns.
- 2026-09-26 01:44, the nudge's rule tightened before the round with it had reached Pi: it fires only when the board is not busy, no task was ever added and no question ever asked, and the chief's screen has held still (the first rule looked only at the chief's idle state, so a chief waiting on its workers would have been nudged). The Haiku cell of that round ran under the first rule; read its `terminalAnswers` with that in mind.
- 2026-09-26 01:40 to 03:13, the `six-decisions` round with the terminal nudge, same chiefs and staffs. **Haiku 1/7**: nudged twice ("Da, cum recomanzi. Continuă."), it did the whole job itself, 9 edits, 7 files changed, nothing on the board. **Devin 1/7**: the same, 7 files changed in 25 turns after two nudges. **Pi 5/7**: 5 tasks, 2 side by side, 1 advice, 1 review, 1 question, 1 note, 3 files changed, no nudge needed; then the same hang (socket in `CLOSE_WAIT` after ten minutes). **OpenCode 1/7**: one question with options, then a dead turn (an assistant item that never completes, no connection open) for the rest of the cap, as in the first round. Reading: with cheap models the board is used fully by Pi and not at all by Claude Haiku and Devin, whatever the owner says in the terminal; OpenCode cannot be judged until its free model stops dying mid-run. The scripted owner now matches a question's first line (its subject) before its whole body, after two answers went to the wrong subject.

