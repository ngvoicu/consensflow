---
id: board-daemon-core
title: Board, daemon and inboxes — the Stage 2 rewrite of the daemon core
status: active
created: 2026-09-19
updated: 2026-09-19
priority: critical
tags: [rewrite, board, daemon, inbox, ledger, adapters, windows]
---

# Board, daemon and inboxes (Stage 2)

The rewrite of ConsensFlow's daemon core that Gabriel authorized on 2026-09-19
("if you need to rewrite old consensflow please do so, we need clean code and
coherence"). Started on 2026-09-19 on branch `stage-2/board-daemon-core`, while
the Claude and Codex runs of `.specs/reliable-delivery`'s live bench wait for
their model quotas. Requirements,
research and decisions live in the `consensflow-sme` brain: requirements
(core model, platform and quality, engineering principles), decisions (Stage 2
direction, inbox queue and transcript copy, yolo everywhere), research (Omnigent
and Munder Difflin lessons, harness launch audit, reliability diagnosis).

## What the product becomes

A board is the main screen. Every participant — lead, PM, each advisor, each
worker, and the human — has a lane and an inbox. A coordinator runs one command
to create and dispatch a task; the daemon opens or resumes the right pane,
delivers the task, watches the agent through its harness's own signals, collects
the answer, attaches it to the task and queues it in the requester's inbox, from
which the daemon delivers it one item at a time when the requester is idle.
Questions travel the same way, to a coordinator or to the human. Nothing else
opens panes or types into them.

## Decisions and scope

- **Keep the shell, rebuild the core.** The Tauri app (window, Rust PTY host,
  input arbiter, updater) and the bridge protocol stay; the Node daemon core is
  rebuilt.
- **Built beside the old core, switched once.** The new core is new modules
  with its own daemon entry. From Phase B on, the integration harness and the
  live bench start it with the real Rust pane host, so it is proven end to end
  before the app uses it. The app changes over in one phase (the switch), which
  deletes the old core: `store.js`, `tabs.js`, `panes.js`, `delivery-watch.js`,
  the inbox, delivery and receiver modules it replaces, `page.js`, `tasks.js`,
  `launch.js`, `requester.js`, the JSON state files and the `O_EXLOCK` lock. The
  branch merges only when the new core is the only core, so no shipped build
  ever has two sources of truth (ENG-2).
- **One ledger:** SQLite through the built-in `node:sqlite` at
  `<home>/consensflow.db`, WAL mode, one writer (the daemon). Schema versions and
  migrations are code. The board and every inbox are views of it.
- **The ledger's lock is the instance lock.** The daemon opens the file with
  `locking_mode=EXCLUSIVE` and takes the lock at once; a second process gets
  "database is locked", and the operating system releases the lock when the
  holder dies (probed on 2026-09-19). It works on Windows too. Nothing but the
  daemon opens the file: `cf`, the page and the tests go through the daemon.
- **The model.** A *session* is one board: a lead with its workers and,
  optionally, a PM with its advisors, plus the human. A *participant* is one
  member with a lane and an inbox (human, lead, PM, advisor, worker) and at most
  one pane at a time; parallel work goes to different participants. A *task*
  has a requester, an assignee and a state. A *message* is one inbox item
  (task, result, question, answer, note) with a delivery state and a receipt.
  A *conversation* is a participant's native harness session, kept as history.
  An *event* is an append-only line of the session's log.
- **Adapters own the harnesses:** one module per harness (Claude Code, Codex,
  OpenCode, Pi, Devin; Kimi kept but paused) with one interface: launch
  (full-permission mode, settings, extensions), deliver (native input first),
  signals (working, idle, waiting with a reason, turn ended), collect (the answer
  written after a delivery), transcript source.
- **Signals before transcripts:** status comes from each harness's own signals
  (Claude's sessions status file and hooks, Codex app-server events, OpenCode
  server events, Pi extension events, Devin hooks), one publisher per pane.
  Transcript parsing is only for collecting answers, from ConsensFlow's own copy.
- **Inbox queue, one by one:** every result, question, answer and follow-up is a
  message queued for its recipient; the daemon delivers the head of each queue
  when the recipient is idle, proves arrival from a native signal, and keeps a
  receipt. A long result is delivered as a short notice plus `cf inbox read`.
- **Daemon-only dispatch:** `cf task add --to @agent` creates and dispatches;
  `cf run`, `cf say`, `cf attach`, `cf read`, `cf results`, `cf lead send/read`
  and every way for an agent to open a pane are removed.
- **Session team:** the human picks the agents a session may use (Agent library
  → Your agents → Session team, with uses: worker, advisor, reviewer); the
  daemon refuses assignments outside the team; the previous team is reused.
- **Transcript copy:** the daemon copies every transcript it launched into
  `<home>/transcripts/` incrementally, with a retention rule.
- **Windows:** everything above runs on Windows; the automated suites run on a
  Windows runner.
- Out of scope: cloud or cross-machine sessions; a separate background daemon
  process (revisit only if real crashes appear).

## Phases (each: failing tests first, live bench green, dead code deleted)

### Phase A: Ledger [completed]

- [x] [TEST-BDC-01] Ledger schema, migrations and the domain operations (sessions, participants, team, conversations, tasks, messages, events), the instance lock, crash safety and the invariants each operation keeps. `tests/ledger.test.mjs`, 26 tests.
- [x] [IMPL-BDC-02] `src/ledger/` on `node:sqlite` (`index.js`, `schema.js`); satisfies TEST-BDC-01.

### Phase B: Harness adapters [planned]

- [ ] [TEST-BDC-05] Adapter contract (launch, deliver, signals, collect, transcript source) with a fake harness per adapter.
- [ ] [IMPL-BDC-06] Claude Code adapter (status file, hooks, peer delivery, Stop-hook turn end).
- [ ] [IMPL-BDC-07] OpenCode, Pi, Devin and Codex adapters, including their waiting signals.
- [ ] [VERIFY-BDC-08] Live bench per adapter, as lead and as worker, with the test models.

### Phase C: Dispatcher and inbox delivery [planned]

- [ ] [TEST-BDC-09] Task state machine; per-participant queue delivering one item at a time when idle; receipts; retries; the launch-liveness reaper; restore on start.
- [ ] [IMPL-BDC-10] Dispatcher; satisfies TEST-BDC-09.

### Phase D: CLI, API and role skills [planned]

- [ ] [TEST-BDC-11] `cf task add/list/get/accept/reopen`, `cf inbox`, `cf ask`, `cf answer`; team enforcement; the removed commands are gone.
- [ ] [IMPL-BDC-12] CLI, daemon API and regenerated lead/PM/advisor/worker instructions.

### Phase E: Board-first UI [planned]

- [ ] [TEST-BDC-13] Playwright: lanes per participant, cards and markers, card detail, the human inbox with question answering, the team picker, session views kept.
- [ ] [IMPL-BDC-14] The board page on the new core's page protocol.

### Phase F: The switch [planned]

- [ ] [TEST-BDC-03] Import from alpha.62 state (tabs, threads, tasks, inbox results) is complete, idempotent and read-only on the source; replayed against a copy of the live home.
- [ ] [IMPL-BDC-04] Importer; satisfies TEST-BDC-03.
- [ ] [IMPL-BDC-21] `cf ui` runs the new core; the old core, its page code, the JSON state and `O_EXLOCK` are deleted; every suite and the live bench are green.

### Phase G: Transcript copy [planned]

- [ ] [TEST-BDC-15] Incremental copy per harness, retention, answers collected from the copy.
- [ ] [IMPL-BDC-16] `src/mirror/`.

### Phase H: Windows [planned]

- [ ] [TEST-BDC-17] Windows runner for Node and Rust suites; process-tree termination and single-instance guarantees on Windows.
- [ ] [IMPL-BDC-18] Job Objects in the PTY host, Windows updater path, path and shim handling.

### Phase I: Acceptance [planned]

- [ ] [VERIFY-BDC-19] Full live bench matrix, Playwright, a computer-use pass over the Candidate, an upgrade from a copy of the live home, hours-long CPU and memory profile.
- [ ] [VERIFY-BDC-20] Hand the Candidate to Gabriel for the live test.

## Resume context

Phase A is done (the ledger, unwired until the switch by design). Next:
Phase B, the harness adapters, starting with the contract test TEST-BDC-05.

## TDD log

- TEST-BDC-01 RED: the module did not exist (suite failed to load). GREEN after
  IMPL-BDC-02: 24/25, the one failure a wrong test (it expected a native
  session id to be unique across harnesses; it is unique within one harness).
  Review added three cases: a file that is not a database is refused as
  `ledger-unreadable`, a question needs its asker, and WAL is asserted. 26/26,
  three runs. The crash test kills a child writing tasks in a loop after
  150 ms; every task it left has exactly its one task message, and
  `integrity_check` is ok.
