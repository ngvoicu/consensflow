---
id: reliable-delivery
title: Reliable delivery, markers and daemon load (board redesign phase 1)
status: active
created: 2026-09-19
updated: 2026-09-19
priority: critical
tags: [reliability, delivery, markers, performance, permissions]
---

# Reliable delivery, markers and daemon load

Phase 1 of the board redesign: make the next installable build reliable before
the ledger, board-driven dispatch and inboxes are rebuilt (phase 2). Gabriel
authorized development on 2026-09-19 ("go ahead with development"). Baseline:
branch `candidate/alpha-62`. Requirements, research, the 2026-09-19 reliability
diagnosis and every decision live in the `consensflow-sme` brain
(`~/Projects/ngvoicu/consensflow-sme`); this spec tracks implementation only.

## Decisions and scope

- The live app (`/Applications/ConsensFlow.app`, home `~/.consensflow`) is never
  touched. Everything is verified in ConsensFlow Candidate
  (`npm run candidate`, home `~/.consensflow-candidate`).
- Every unit starts with a failing test built from synthetic data (never a copy
  of a real transcript) and removes whatever it makes dead.
- Real transcripts may be read, never written, to replay a fix against live
  evidence.
- Live checks use the test models in the brain (`operations/test-models.md`):
  Claude Code Sonnet, OpenCode and Pi Muse Spark 1.3, Devin `swe-1-6-slow`,
  Codex `gpt-5.6-luna` when its quota allows. Kimi is paused (Gabriel, 2026-09-19).
- Every harness starts in full-permission mode (brain decision
  `2026-09-19-yolo-everywhere`).
- Out of scope, phase 2: the SQLite ledger, the inbox queue with one-by-one
  delivery (Gabriel, 2026-09-19), board-driven dispatch, removal of `cf run`,
  ConsensFlow's own copy of transcripts (Gabriel, 2026-09-19), Windows.

## Phase 1: Evidence the gate can trust [active]

- [x] [TEST-REL-01] tests/engine/completion.test.mjs: a cross-session message queued with an extra envelope attribute and removed without it leaves no queued turn.
- [x] [IMPL-REL-02] hosts/lib/completion.js: match queue removals and pops on content with the envelope tag's attributes set aside after an exact match fails; satisfies TEST-REL-01.
- [ ] [TEST-REL-03] Claude turns that finish without `turn_duration` (Calliope, Fable 5.1 at max effort) are recognised as finished from another native proof, after reading real records read-only.
- [ ] [IMPL-REL-04] hosts/lib/completion.js: that proof; satisfies TEST-REL-03.

## Phase 2: Daemon load [pending]

- [ ] [TEST-REL-05] tests for the watcher: deleted conversations are not parsed; each native session is parsed at most once per scan; an unchanged file is not parsed again; old unconfirmed receipts stop being re-checked after a bound.
- [ ] [IMPL-REL-06] src/delivery-watch.js (and the transcript reader if needed); satisfies TEST-REL-05.
- [ ] [TEST-REL-07] The state message the page loads stays bounded as deliveries grow.
- [ ] [IMPL-REL-08] Page the delivery list; satisfies TEST-REL-07.

## Phase 3: Launch [pending]

- [ ] [TEST-REL-09] Launch arguments put Claude Code, Codex (fresh and resume), OpenCode, Pi and Devin in full-permission mode for every role.
- [ ] [IMPL-REL-10] hosts/lib/runners.js and the per-harness settings; satisfies TEST-REL-09.
- [ ] [TEST-REL-11] Panes and child processes never inherit `CLAUDE_CODE_*` session variables (configuration such as `CLAUDE_CONFIG_DIR` stays).
- [ ] [IMPL-REL-12] Strip them at launch; satisfies TEST-REL-11.

## Phase 4: Visibility and restart [pending]

- [ ] [TEST-REL-13] A pane waiting on a permission prompt or a question shows "waiting", from each harness's native signal.
- [ ] [IMPL-REL-14] Wire those signals; remove the never-sent `pane.idle` subscription; satisfies TEST-REL-13.
- [ ] [TEST-REL-15] After an app restart, sessions that were open come back without a manual Resume.
- [ ] [IMPL-REL-16] Restore on start; satisfies TEST-REL-15.
- [ ] [IMPL-REL-17] The app's and the daemon's error output goes to a log file inside the home.

## Phase 5: Acceptance [pending]

- [ ] [VERIFY-REL-18] Live bench in the Candidate with the test models: dispatch, delivery to the lead, markers, permission mode and restart, per harness.
- [ ] [VERIFY-REL-19] Full suites, candidate rebuild with the packaged smoke, brain status and progress updated.

## Resume context

Unit 1 (TEST-REL-01/IMPL-REL-02) is done. The next unit is TEST-REL-03: read
a Calliope transcript (read-only) to find what marks the end of a turn when
`turn_duration` is missing.

## TDD log

- TEST-REL-01 RED: `queuedTurns` was `['queue:0']`. GREEN after IMPL-REL-02:
  completion suite 70/70.
- Real-data replay (read-only) of the live lead transcript
  `5f7ecad7…` (135.7 MB): installed alpha.61 code says in-flight, 10 queued,
  1,355 settled answers, last 2026-09-18 07:49:40Z; the fixed source says
  settled, 0 queued, 1,505 settled answers, last 2026-09-19 05:55:57Z.
