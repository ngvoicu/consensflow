---
id: sessions-are-the-humans
title: Sessions are the human's — no expiry, no cap, a window opened, closed or deleted from its lane
status: completed
created: 2026-09-21
updated: 2026-09-21
priority: high
tags: [board, sessions, daemon]
---

# Sessions are the human's

Gabriel, 2026-09-21, on learning that a finished session expired after two
idle hours and that a member ran at most two: "It should not expire and we
should not cap to 2 sessions; unlimited; only a human can close a session.
We should make a button on that lane, under 'Window closed with its task; a
follow-up reopens it' (these messages should also go away); so the windows
should be closable and resumable and deletable; and when resume happens it
should open the original session."

## Decisions

- **No expiry, no cap.** A session keeps its conversation until the human
  deletes it. A member runs as many sessions as it has tasks; only quota
  keeps a member from new work. Accepting, cancelling, a review's verdict or
  the daemon taking work back no longer ends a session.
- **Three buttons on a session's lane.** *Open window* brings the window
  back on its own conversation, with its history, and it stays open until
  the human closes it, whatever work comes and goes. *Close window* closes
  it (work in it pauses, as any lost window's does). *Delete session* ends
  it: its lane folds into its member's, its conversation closes; refused
  while it holds work (queued, working, waiting or in review).
- **The lane's status is short.** "Window closed" for a session without a
  window; the old sentence is gone.
- **One word on screen: terminal** (Gabriel, 2026-09-21: "then we can have
  open terminal, close terminal and remove the window stuff"). A session's
  row offers *Open terminal* (reopens a closed one on its conversation, or
  brings an open one into view, unfolding the dock), *Close terminal* and
  *Delete session*, plus *Transcript* while it is closed. The lead's row has
  Open terminal; a member's row is a heading with no buttons. The dock is
  titled Terminals.
- **The transcript copy stays** (asked: "do we still need to keep a copy?"):
  reopening shows the harness's own history, but that history is the
  harness's to prune, it is gone once the session is deleted, and the card
  needs no window. Gabriel may still drop it.
- **Rejected:** ending sessions on a clock or on acceptance; a cap on
  sessions; reclaiming idle sessions when a member needs a slot.

## Phases

### Phase A: Sessions are the human's [done]

- [x] [TEST-SH-01] Ledger: no `SESSION_SLOTS`, no `SESSION_IDLE_MS`, no
  `expireSessions`, no `busy`; `endSession` (the human's, refused while the
  session holds work); accepted, cancelled, judged and released work keeps
  its session; a follow-up finds an accepted task's session. Dispatcher: a
  window the human opened is not retired; close and end at the human's
  hand; nobody is ever "busy", only out or low on quota. Page: `session.open`,
  `session.close`, `session.end`. Board: the three buttons, "Window closed".
- [x] [IMPL-SH-02] Satisfies TEST-SH-01; the lead's text and the README.

## TDD log

- 2026-09-21, one gated commit: ledger, dispatcher, API and page suites
  (186), browser 87, Rust 108 + 16; seven tests that pinned the cap, the
  expiry or auto-ended sessions rewritten to the new rule.
