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

## Phase 2: Daemon load [completed]

- [x] [TEST-REL-05] tests/inbox-scan.test.mjs, tests/engine/completion.test.mjs: a deleted conversation is never read; an unchanged transcript returns the previous result and a grown one is read again.
- [x] [IMPL-REL-06] hosts/lib/completion.js (`cachedAnswers`, `locateTranscript`), src/delivery-watch.js; satisfies TEST-REL-05. Old unconfirmed receipts are still checked, but each check is now a file stat, so no bound was added.
- [x] [TEST-REL-07] tests/inbox-page.test.mjs, app/tests/page.spec.mjs, commands.rs forwarding table: the state carries unconfirmed results and per-conversation counts only (under 64 KB with 2,000 received results); history is paged newest first for a session or one conversation, with "Load older results" in the dialog.
- [x] [IMPL-REL-08] src/page.js (bounded state, `answersList` paging), app/src-tauri/src/commands.rs (`answers_list(tab, conversation?, offset?)`), app/ui/panes.js (dialog pages its history), app/ui/tasks.js (counts); satisfies TEST-REL-07. The unused `answers` state field was removed.

## Phase 3: Launch [completed]

- [x] [TEST-REL-09] tests/engine/interactive.test.mjs, tests/channels.test.mjs: every window, fresh and resumed, opens in full-permission mode (Claude `--permission-mode bypassPermissions`, Codex bypass flag, OpenCode `--auto`, Pi `--approve`, Devin `--permission-mode dangerous --respect-workspace-trust false`, Kimi `--auto`); Claude's settings file carries `permissions.defaultMode`, `skipDangerousModePermissionPrompt` and `crossSessionInbound: accept`.
- [x] [IMPL-REL-10] hosts/lib/runners.js (`YOLO`), src/claude-install.js; satisfies TEST-REL-09. Live-proven for Claude; OpenCode, Pi and Devin are proven by the live bench (VERIFY-REL-18).
- [x] [TEST-REL-11] app/src-tauri/src/lib.rs: a parent Claude session's identity variables are never inherited; configuration stays.
- [x] [IMPL-REL-12] lib.rs removes the identity list at startup, before the daemon and panes exist (`2becdcc`); satisfies TEST-REL-11.

## Phase 4: Visibility and restart [active]

- [x] [TEST-REL-13] tests/inbox-scan.test.mjs, app/tests/page.spec.mjs: a Claude pane shows the status Claude records itself (waiting with its reason, busy, idle); the page shows "Waiting" with the reason.
- [x] [IMPL-REL-14] src/delivery-watch.js (`claudeStatuses`), app/ui/sidebar.js, app/ui/index.html; the never-sent `pane.idle` subscription is removed; satisfies TEST-REL-13. Codex, OpenCode, Pi and Devin waiting signals move to the Stage 2 harness adapters (each adapter owns its native signals); Rust's `idle_ms` stays as Stage 2's silence fallback.
- [ ] [TEST-REL-15] After an app restart, sessions that were open come back without a manual Resume.
- [ ] [IMPL-REL-16] Restore on start; satisfies TEST-REL-15.
- [ ] [IMPL-REL-17] The app's and the daemon's error output goes to a log file inside the home.

## Phase 5: Acceptance [pending]

- [ ] [VERIFY-REL-18] Live bench in the Candidate with the test models: dispatch, delivery to the lead, markers, permission mode and restart, per harness.
- [ ] [VERIFY-REL-19] Full suites, candidate rebuild with the packaged smoke, brain status and progress updated.

## Resume context

Phases 1-3 and the Claude waiting marker are done. Next: TEST-REL-15 (restore
open sessions after an app restart), then error logs, then the live bench.

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
- TEST-REL-05 RED: the deleted conversation's reply was indexed; `cachedAnswers`
  did not exist. GREEN after IMPL-REL-06: completion 71/71, watcher, Devin and
  lifecycle suites 28/28.
- Measured on copies of the live home (read-only source), same machine: the
  previous commit's watcher took 7.3 s, 23.1 s and 17.7 s for three scans; this
  change takes 4.0 s, 1.9 s and 0.56 s (about 30-40x faster in steady state).
  Memory stays about 1.1 GB in both; Stage 2's ledger addresses that.
- TEST-REL-07 measured first: on a copy of the live home the committed code's
  state message was 489 KB, 485 KB of it the 1,154-result history (alpha.61:
  216 KB). RED: 3 Node tests; the Rust forwarding test hung waiting for the new
  body (RED); GREEN after IMPL-REL-08: inbox-page 6/6, Rust forwarding 1/1, UI
  137/137 including the new paging test. `answers.list` had no caller; it now
  serves the dialog.
- TEST-REL-09 facts, checked on the installed binaries and docs before coding:
  OpenCode's resolved rules keep `doom_loop` and `external_directory` as "ask"
  under `"permission": "allow"` (the last matching rule wins, and the built-in
  specific rules come after `*`), so the TUI's documented `--auto` is used.
  Claude's docs: a bypass-mode session holds messages from other sessions and
  drops them after 5 minutes unless `crossSessionInbound` is `accept`.
- Live proof (Claude Code 2.1.278, Sonnet, real TUI in a PTY): launched with the
  flag and settings file, no bypass-acceptance dialog appeared, a message in
  ConsensFlow's peer format was delivered to the idle session and answered
  (`PEER_OK`), and `stop_hook_summary` was recorded. A probe that stopped
  reading its PTY hung the child's exit (the known macOS rule); the probe now
  drains until the child is gone.
- TEST-REL-13: Claude Code 2.1.278 writes `status` and `statusUpdatedAt` into
  `~/.claude/sessions/<pid>.json` (read on the live machine, read-only). RED:
  the pane read `unknown`, and the page had no Waiting label; GREEN: inbox-scan
  12/12, the Playwright waiting test passes.
