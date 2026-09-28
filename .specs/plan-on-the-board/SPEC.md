---
id: plan-on-the-board
title: A plan on the board — task needs, blocked tasks, and a task put before others
status: completed
created: 2026-09-20
updated: 2026-09-20
priority: high
tags: [board, tasks, dependencies, daemon]
---

# A plan on the board

Gabriel, 2026-09-20 evening: "The lead has a big plan, hundreds of tasks, it
puts them in the backlog or something; the daemon takes them one by one and
assigns them, but they should be in a particular order and also maybe they
can't be done in parallel; and sometimes one task discovers some issue and an
intermediate task is needed before others can start. What do we do then?"
Then: "you decide and build … just ship."

## Decisions

- **One concept: a need.** A task on the board may need other tasks of the
  project (`cf task add --tier <tier> --needs T-3,T-4 "…"`). It stays open
  and *blocked* until every need is **accepted**: done is not enough, since
  the lead may send a finished task back. The daemon gives out only
  unblocked open tasks, in number order, so parallel work happens where the
  plan allows it and nowhere else.
- **A task put before others.** `--before T-9,T-10` adds the new task as a
  need of tasks still on the board. A task already in a window is not pulled
  back: the lead finishes it, or cancels it and adds it again with the need.
  A `--before` naming a task that is not open refuses the whole add.
- **What the board says.** An open card reads "blocked by T-3, T-4 · for a
  standard worker"; the drawer lists each need with its state; `cf task
  list` says the same; `cf task add` says what the new task waits for and
  what waits for it. A blocked task gets no "waits for a free worker" note.
- **A cancelled or failed need keeps its dependents blocked**, and the card
  says so: the lead re-adds the need or cancels the chain.
- **Needs go with tasks on the board only.** `--self`, `--after` and a task
  for a member by name refuse them. (The human's New task form had an
  "Only after" field until 2026-09-21, when New task went: the human asks
  the lead in its terminal, and the lead orders the board.)
- **A plan has no circles.** A `--before` target that the new task waits
  for, directly or through its needs, refuses the add (`circular-needs`):
  otherwise both would wait forever with nothing to say but "blocked by".
- **Rejected:** phases (a coarser DAG, expressible with needs); a project
  "one task at a time" switch (a chain of needs); a lock or resource concept
  for tasks that touch the same files (chain them with needs until a real
  plan needs more); a cap on tasks in flight (easy later, not now).

## Phases

### Phase A: Needs end to end [done]

- [x] [TEST-POB-01] Ledger: a seventh migration adds `task_need`; a task's
  view carries `needs` (number and state) and `blockedBy`; `createTask`
  takes `needs` and `before` with their refusals (unknown, cancelled, not
  open, not on the board, not numbers); a need is met at accepted; a
  cancelled need blocks on. Dispatcher: a blocked task is not assigned and
  gets no waiting note; it goes out once its needs are accepted. API and
  `cf`: `--needs`, `--before`, the sentences, the list line, usage errors.
  Board: the card's route and the drawer's needs.
- [x] [IMPL-POB-02] Satisfies TEST-POB-01; the lead's text and the README
  say how a plan goes on the board and what to do when a result uncovers
  work that must come first.

## TDD log

- 2026-09-20 evening, in one gated commit: ledger 92 (4 new), dispatcher
  and API 66 (2 new), browser 83 (1 new), the full Node suite and the
  integration suite green in the isolated gate.
