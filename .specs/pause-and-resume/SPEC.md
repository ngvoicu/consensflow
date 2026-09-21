---
id: pause-and-resume
title: Pause and resume — the lead stops a worker's task and brings it back, and a lost window waits for the lead
status: completed
created: 2026-09-21
updated: 2026-09-21
priority: high
tags: [board, tasks, daemon, sessions]
---

# Pause and resume

Gabriel, 2026-09-21: "Leads should be able to stop work of one of the
workers, for example if I say something to him or some other worker finds
something bad; and maybe resume later." On the first design (close the
window on pause): "why close? it shouldn't be paused because it can resume?"
And: "if something happens, app restarts or stuff like that, the lead should
be able to trigger resume of the work." On naming workers: still not wanted;
a pause names the task, never the member.

## Decisions

- **A task can be paused** (`cf task pause T-5`; Pause on the card) from the
  board or from a window, by the lead or the human, never a review and never
  the lead's own work. The agent is interrupted (the Escape key, once) and
  its window stays open; whatever was on its way to the task is withdrawn;
  what the agent still writes is not collected. The task keeps its member,
  its session and its conversation.
- **A task resumes with words** (`cf task resume T-5 "…"`; Resume on the
  card): into the same window when its session is there, a brief never
  delivered going in first; back onto the board for its tier when the
  session has ended (two idle hours). A task paused on the board goes back
  on the board with the words appended.
- **A lost window pauses its task instead of giving it up.** A restart, a
  crash, a window the human closed, or a closed project: the task is paused
  with its session and conversation, and the lead is told how to resume it.
  The window then comes back on its own conversation, with its memory. A
  review lost this way is still withdrawn for another reviewer, and a member
  out of quota still loses its task to another member.
- **The board says it.** A paused card sits in the Queued column, dimmed
  and labelled Paused; the drawer offers Pause or Resume; `cf task list`
  reads `[paused]`.
- **Rejected:** closing the window on pause (Gabriel: resume should be
  instant); a pause reason on the task (the resume words carry what
  matters); naming a worker to stop (the task number is enough).

## Phases

### Phase A: Pause and resume end to end [done]

- [x] [TEST-PR-01] Ledger: a ninth migration adds the `paused` state;
  `pauseTask` and `resumeTask` with their refusals and every resume path
  (same window, brief first, back on the board, session ended); a paused
  task holds its window and keeps its session on acceptance of earlier
  work; `pausedTask`. Dispatcher: Escape once, the window kept, output not
  collected, resume delivered into the same window; a closed window and a
  restart pause with a note and resume on the conversation; a closed
  project pauses its work. API and `cf`: `pause`, `resume`, the sentences,
  the usage line, only the lead or the requester. Page: `task.pause`,
  `task.resume`. Board: the paused card, Pause and Resume in the drawer.
- [x] [IMPL-PR-02] Satisfies TEST-PR-01; the lead's text and the README.

## TDD log

- 2026-09-21, one gated commit: ledger 96 (4 new), dispatcher 45 (1 new, 3
  rewritten from giving up to pausing), API 22 (1 new), page 15 (1 new),
  browser 86 (1 new); the full Node and integration suites green in the
  isolated gate.
