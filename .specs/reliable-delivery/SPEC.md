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

## Phase 1: Evidence the gate can trust [completed]

- [x] [TEST-REL-01] tests/engine/completion.test.mjs: a cross-session message queued with an extra envelope attribute and removed without it leaves no queued turn.
- [x] [IMPL-REL-02] hosts/lib/completion.js: match queue removals and pops on content with the envelope tag's attributes set aside after an exact match fails; satisfies TEST-REL-01.
- [x] [TEST-REL-03] tests/channels.test.mjs, tests/claude-receiver.test.mjs: every Claude launch carries one `--settings` file under `<home>/integrations/claude/<launch>/` whose Stop hooks end with a no-op turn-end hook, merged with a coordinator's receiver hooks; a launch without a home is refused.
- [x] [IMPL-REL-04] src/claude-install.js (`prepareClaudeSettings`, receiver returns hooks), src/channels.js, src/panes.js; satisfies TEST-REL-03.

## Phase 2: Daemon load [active]

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

Phase 1 is done. Next: TEST-REL-05 (watcher load). The committed watcher
already coalesces scans; it still parses every bound conversation each second
and re-reads the lead's history once per old unconfirmed legacy receipt.

## TDD log

- TEST-REL-01 RED: `queuedTurns` was `['queue:0']`. GREEN after IMPL-REL-02:
  completion suite 70/70.
- Real-data replay (read-only) of the live lead transcript
  `5f7ecad7…` (135.7 MB): installed alpha.61 code says in-flight, 10 queued,
  1,355 settled answers, last 2026-09-18 07:49:40Z; the fixed source says
  settled, 0 queued, 1,505 settled answers, last 2026-09-19 05:55:57Z.
- TEST-REL-03 research (read-only): across all tracked Claude sessions,
  `turn_duration` appears on only some turns in every version, and
  `stop_hook_summary` never appears (no worker pane had a Stop hook). Six
  Calliope sessions (2.1.274/2.1.276) have neither, so their answers never
  settled. Live probes (Sonnet): a Stop hook, from a file or inline JSON with
  `exit 0`, makes Claude 2.1.277 record `stop_hook_summary` after the final
  answer, which the completion model already accepts. TEST-REL-03 RED: args
  length 0; GREEN after IMPL-REL-04: channels and receiver suites 10/10.
  First version passed the settings inline; the full gate showed the lead's
  launch frame grow past a test's deliberately small frame cap (3,069 bytes),
  so settings moved to a per-launch file under the home, like Devin and Pi.
- The full gate also exposed a third race in the Pi duplicate test: the
  extension's own inbox watcher can start the scan first, so the test now waits
  for the send (30/30 solo runs pass).
