---
id: quota-hold
title: Quota mid-work — hold the task with its window when the reset is near or nobody else can take it
status: completed
created: 2026-09-24
updated: 2026-09-24
priority: high
tags: [quota, dispatcher, ledger, board]
---

# Quota mid-work: hold or release

Gabriel, 2026-09-24, after gefjon's window lost T-1 to a misread retry:
"we should do something to check if quota is reached and handle it
somehow because the quota might be reached mid work as well; how do we
want to handle it?" → the rule below → "implement".

## Decisions

- **Detection stays per harness.** Codex says ahead (usage %, `low` at
  95 %, exhausted with a reset); Claude Code, Pi and Devin only on a
  refusal in their record; OpenCode in its live status, and only for a
  spent quota (`free_tier_limit`, or a limit with the retry a minute or
  more away). Claude Code's session file carries no usage field (looked
  on 2026-09-24), so there is nothing to look ahead with there.
- **Hold when the reset is near or nobody else can take it.** A member
  out of quota mid-task keeps the task with its session when the reset
  is within 30 minutes, or when no other free member of the task's role
  and tier exists: the task is *paused* with `held_until`, its agent is
  stopped and its window waits as any paused task's does, and the
  requester is told "T-3 waits
  with @zeus-amber-pine: out of quota until …; it goes on by itself
  then." Otherwise the task goes back to the board for another member,
  as before, with the partial-work note.
- **It goes on by itself.** Each pass, a held task whose time has come
  (and whose member is no longer out) is resumed in its own window with
  "Go on where you stopped." — the same words and path as the human's
  Resume. The board shows "out of quota until HH:MM" on the card until
  then; the lead's `cf task resume` and the human's Resume or Reassign
  work at any time.
- **Rejected:** always releasing (loses the window's context, and with
  no teammate the task just sits); always holding (a long reset with an
  idle teammate wastes the team).

## Phases

### Phase A: the ledger

- [x] [TEST-QH-01] `holdTask` pauses with `heldUntil`; `heldTasksDue`
  lists the ones whose time has come; any move clears `heldUntil`;
  `resumeTask` works without `by` (the daemon) and keeps the window.
- [x] [IMPL-QH-02] Migration 3 (`task.held_until`), the methods, the view.

### Phase B: the dispatcher

- [x] [TEST-QH-03] Reset in 20 min → held, note, window closed; the clock
  past the reset → resumed in the same window with the resume words.
  Reset in 3 h with a free teammate → released as before. Reset in 3 h
  with no teammate → held.
- [x] [IMPL-QH-04] `#outOfQuota` decides per task; `pass()` resumes what
  is due.

### Phase C: the board

- [x] [TEST-QH-05] A held task's card reads "out of quota until HH:MM".
- [x] [IMPL-QH-06] `route()`.

## TDD log

- 2026-09-24, all phases in one commit (`77e0664`, README `d89200c`): migration 3 `task.held_until`, `holdTask`/`heldTasksDue`, `resumeTask` without `by`; `#outOfQuota` holds or releases per task and `#resumeHeld` resumes what is due in the same window; the card reads "out of quota until HH:MM". Ledger, dispatcher and page suites green in the gate. The record here was ticked on 2026-09-25, a day late.
