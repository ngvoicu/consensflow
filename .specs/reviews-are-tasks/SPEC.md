---
id: reviews-are-tasks
title: Reviews are tasks — no automatic reviews, no review buttons; the lead adds a review for a reviewer
status: completed
created: 2026-09-21
updated: 2026-09-21
priority: high
tags: [board, reviews, daemon, ledger]
---

# Reviews are tasks

Gabriel, 2026-09-21, after an automatic review failed on the Candidate and a
reviewer's row said "Reviewing T-1" with nothing on the board: "I feel like
these flows with tasks and then reviews... we should not have automated
reviews and we should not have tasks with buttons to ask for reviews; the
lead can add tasks and can add review tasks for reviewers; we don't do magic
items; so let's simplify." The same hour: "let's clean all sessions,
projects and all work; let's clean everything from .consensflow-candidate
and start from scratch."

## Decisions

- **Nothing is reviewed on its own.** A worker's result goes straight to
  whoever asked; there is no project review policy, no held result, no
  rounds, no verdict line and no automatic send-back.
- **A review is a task.** The lead puts it on the board for a reviewer of a
  tier, like any other task: `cf task add --review --tier complex "Review
  T-3: …"`. The daemon gives it to a free reviewer of that tier in a session
  of its own; the reviewer can read the reviewed task with `cf task get
  T-3` and changes no file. Its findings come back to the lead as the
  review's result, and the lead decides the work (accept, reopen, cancel)
  and the review.
- **No review buttons.** The page has no Ask for a review, no review policy
  in the New project or Team dialog, no In review column, no review lines
  under cards and no Reviews panel in the drawer. A review card sits on its
  reviewer's row like any card.
- **The schema starts afresh.** The Candidate home was moved aside (kept in
  the session scratchpad with its ten saved agents) and the migrations
  squashed into one: go-live starts clean too, so no older ledger needs a
  way up. A ledger written by the old schema is refused as newer.
- **Cleaned with it:** the human's decline carries no reason (the page sent
  none since the drawer stopped writing to agents); the dispatcher's
  delivery takes no project.
- **Rejected:** keeping the policy but dropping the button (still magic: work
  moves without the lead); a reviewer on another model enforced by the
  daemon (the lead names only the tier, never the member).

## Phases

### Phase A: Reviews are tasks [done]

- [x] [TEST-RT-01] Ledger: no review policy, kind, round or verdict; a
  reviewer task is opened for a tier, assigned to a reviewer session, and
  its result goes to the lead; the schema refuses a review state, a held
  message and an unknown pool. Dispatcher: a review task launches with the
  reviewer text and its findings come back as the result. API and cf:
  `--review` opens a reviewer task, `cf task review` is gone. Page: no
  `task.review`, no `project.review`. Board: no review column, lines, panel
  or policy selects. Integration: the lead adds a review through `cf` and
  gets the findings, through the real pane host.
- [x] [IMPL-RT-02] Satisfies TEST-RT-01; the role texts (lead, coordinating,
  reviewer, worker, advisor) and the README.

## TDD log

- 2026-09-21, one gated commit (`fdadf05`): Node 657 passed, 3 skipped
  (660); integration 9/9; browser 79/79; Rust 108 + 16, clippy clean. The
  review-gate suites were replaced by tests of a review as a task.
