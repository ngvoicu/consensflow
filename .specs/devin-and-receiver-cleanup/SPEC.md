---
id: devin-and-receiver-cleanup
title: Devin roles and receiver integration cleanup
status: active
created: 2026-09-12
updated: 2026-09-12
priority: high
tags: [devin, harnesses, inbox, advisors, cleanup]
---

# Devin roles and receiver integration cleanup

Gabriel requests Devin as a worker, lead and PM advisor, plus cleanup of the
recent inbox/advisor development. Devin CLI 3000.6.14 is installed locally.
The planned integration uses the native harness and the existing durable inbox.
PM coordinator support follows the same coordinator interface where supported.

The installed ConsensFlow app and all running sessions remain untouched. Work
is source-only until a separate, private candidate passes verification. Runtime
integration files stay under the configured ConsensFlow home. No global harness
settings, project bookkeeping, paid model trials, release or commit is authorized.

## Acceptance

- Devin appears in Harnesses and agent selection with honest supported roles.
- Native workers/advisors start, complete multiple replies, and resume the exact
  owned conversation. Advisor instructions allow advice/read/research/tests;
  only the PM writes specifications. Role instructions load without a model task.
- A Devin coordinator collects complete inbox parts into the selected native
  conversation, preserving busy/draft input, new/resume ownership, and receipt
  evidence. API/hook success alone cannot mark a result received. If the native
  interface cannot satisfy this, record the limitation before choosing a fallback.
- Every result remains visible in the owning lead/PM group. No old automatic
  sender or retry state machine is reintroduced.
- Remove obsolete recent integration paths and duplicate installation logic;
  preserve immutable files already loaded by live processes and historical data.
- Existing harness, advisor/grid and inbox tests remain green. Verify Devin with
  stock native interfaces and private profiles; distinguish mocks from live proof.
- Closing or suspending a macOS pane also stops its owned detached children,
  including inherited children whose intermediate parent exited. Other panes and
  unrelated processes survive. Native conversation history remains available.

## Current UI preference

The user approved implementing **Devin's stock TUI** after the native probes.
Collect replies during native prompt/Stop callbacks. Replies arriving after full
idleness remain visibly pending until the next user prompt. Stop callbacks must
return promptly; an indefinite wait would delay native cancellation. No ACP chat
pane is selected. Keep the current installed app and running sessions untouched.

## Approach and verification

Use current native CLI help, official documentation and isolated probes before
choosing Devin's transport. Reuse the existing role, launch capability, completion
and receiver modules rather than building a second orchestration system. No new
package is planned. The installed CLI supports ACP and lifecycle hooks; idle
collection and native receipt persistence require verification.

Cleanup targets are bounded to this work: shared private integration installation,
obsolete global-skill/native-plugin detection, stale descriptions of the removed
sender, and dead paths found by a reference audit. Keep explicit user task messages
and legacy result import/read evidence: they are still used product behavior.

## Tasks

- [x] Research and probe Devin native session, completion and receiving interfaces.
- [x] [TEST-PROC-01] Reproduce detached and reparented children surviving close;
  verify isolation from another pane and cleanup on application teardown.
- [ ] [IMPL-PROC-02] Stop launch-owned children using native process identity;
  satisfy TEST-PROC-01 without global process-name matching. ← current
- [ ] Add failing role/launch/completion/inbox tests based on verified contracts.
- [ ] Implement Devin worker/advisor and coordinator integration and catalog entry.
- [x] Consolidate private integration installation and remove obsolete paths.
- [ ] Run focused regressions, all applicable existing gates and private native checks.
- [ ] Prepare a separate verified candidate and document remaining acceptance limits.

## Execution log

- 2026-09-12: inspected local CLI help/version, current source and official hook,
  configuration and command documentation. Research continues on idle receiving.
  Prior baseline is the completed receiver-pull candidate (standalone spec 272/272).
- Research finished: [native findings](research-devin.md). Startup, private role
  context and `session/load` are verified. `session/resume` returns method-not-found.
  No successful native assistant completion was produced without authentication.
  The native terminal has no verified idle receiver; an owner choice is pending
  between an ACP chat pane (automatic receiving) and native terminal next-turn
  collection. Devin has not been added to the supported harness list.
- Cleanup complete: shared immutable integration preparation, unused global-plugin
  detection and cmux compatibility functions removed, retired readiness export and
  channel branches removed, obsolete cmux test setup and doctor output removed.
  Compared with the preceding verified candidate: 11 production files changed,
  78 lines added, 185 removed (107 net removed). Historical receipt import and
  explicit task-message channels remain in use.
- Verification: focused installer 20/20, completion/pane/channel 212/212, affected
  CLI/roster/lifecycle 69/69; final full Node 1,076 passed, 6 gated skips, 0 failures;
  browser 122/122; packaged smoke 2/2; lint and diff checks pass. An initial full
  run exposed stale plugin assertions and a temp-path assertion that rejected the
  authorized private test root; both now assert the current contract. A subsequent
  run exposed one remaining removed cmux helper call, corrected before final green.
- Cleanup-only candidate is verified at
  `/Users/gabrielvoicu/.consensflow/candidates/cleanup-20260912/` with its own profile.
  All 61 bundled source files match and its deep/strict signature check passes.
  `Launch candidate.command` passes `zsh -n`. Build/test logs are in
  `/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/cleanup-*.log`.
  Installed alpha.61 remains untouched; no repository debug folder was recreated.
  Remaining tasks above concern Devin and subsequent integrated acceptance.

- Deeper research completed at the user's request: [transport decision and
  native evidence](research-native-transport.md). Tested installed Devin 3000.6.14
  and a checksum-verified private copy of latest stable 3000.10.21. Stock hooks
  reject FileChanged and ignore async; a second ACP owner is session-locked.
  No supported stock-TUI receiver or smaller complete generic terminal host was
  found. Recommendation: one ConsensFlow ACP connection and conversation pane
  per Devin pane, shared across roles and using the existing inbox.
- Local mock lifecycle proof: 36 assertions per version, 16 nonempty completed
  native turns and two cancellations across the final runs. Three complete result
  bodies persisted once, new/load kept exact identities, and a replacement owner
  retained history. Cancelled text remains in native history, so the adapter must
  retain explicit turn-completion evidence; final-assistant-row inference is unsafe.
  These are native transport probes, not production receiver/UI/account acceptance.
- This research turn changes only research/spec records and private probe files.
  Devin implementation, role enforcement, UI draft/permission guards, crash-window
  recovery and the integrated candidate remain unfinished. Installed ConsensFlow,
  installed Devin, real sessions and the existing cleanup candidate are untouched.

- Stock-TUI follow-up complete: [actual PTY probes and viable design](research-devin-tui.md).
  Eleven scenarios / 54 assertions verify repeated Stop-hook continuations,
  preserved unsent draft, exact new/resume identities after cancellation, and
  next-prompt collection of a result arriving after full idleness. Core repeat,
  late and cancel cases passed installed 3000.6.14 and private latest 3000.10.21.
- Waiting uses no inference by itself, but native cancellation does not complete
  until the Stop command returns. Indefinite waiting is therefore not a complete
  responsive solution. MCP/remote/loop research found no additional complete idle
  wake contract. Full automatic idle receiving remains unresolved; source feature
  tests/implementation and the integrated candidate remain outstanding.
- No runtime state, installed app, native installation, current conversation or
  existing candidate changed. Research/spec files and private probe evidence only.

## TDD log

| Task | Red | Green | Refactor |
| --- | --- | --- | --- |
| TEST-PROC-01 | cargo test pty::tests::close_stops: 3 tests, 3 failed; drop_stops_detached_children: 1 test, 1 failed. Detached children survived. | — | — |
