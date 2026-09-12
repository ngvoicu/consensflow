# Receiver-owned pull feasibility: Pi and OpenCode

Date: 2026-09-12, Europe/Bucharest (UTC+3).
Scope: experimental research only. No production source, installed ConsensFlow, running app, real session records, real projects, global harness settings or harness binaries were changed. Both probes launched separate installed native TUIs with isolated HOME/config/data/cache/temp/session directories below `~/.consensflow/tmp/receiver-pull/pi-opencode`. Every model response came from an in-process or localhost mock provider; no remote model calls.

## Verdict

Receiver-owned retrieval is feasible on the installed Pi 0.85.1 and OpenCode 1.18.30 native TUIs. Five independent results from the same worker reached the intended native conversation and complete model request in each harness. Their source labels and every body line were also emitted by the native TUI. This is evidence for a common durable-inbox architecture with harness-specific receiver integrations, not evidence that changing polling direction alone solves delivery reliability.

The first Pi prototype reproduced a duplicate after native `/new`: the custom-message event arrived before its native tree entry could be observed. Retrying because immediate receipt evidence was absent delivered the same body twice. Retaining the in-flight result until native persistence was observable fixed the probe. Durable claims, precise receipts and uncertainty handling remain mandatory.

## Prototype contract

- Producer appends immutable `{id, owner, worker, body}` result entries to an atomically replaced local inbox. Five separate IDs belong to `probe-lead`; a sixth belongs to another owner.
- A receiver integration inside the actual TUI polls this inbox every 100 ms. There is no application-side native-session resolver and no controller telling it where to inject a result.
- The receiver selects only results for its logical owner, reads its own current native identity, and waits for native idle state. It inserts a source/result envelope with the complete body and end marker.
- Each result gets its own confirmation containing the exact native session, message/tree entry ID and full body. Confirmation requires a matching native record, not merely a successful submission API response.
- A separate test observer captures the provider request and verifies the complete body there. This stronger observation is not falsely attributed to the receiver's native-persistence receipt.
- Confirmed result IDs are excluded on every subsequent poll and after native session switches. Cross-owner results remain unclaimed.

Pi uses `pi.sendMessage({customType, content, display:true, details:{resultId,owner}}, {triggerTurn:true,deliverAs:'followUp'})`. Native `session_start`/`session_shutdown` refresh and dispose receiver context. `ctx.isIdle()`, `ctx.hasPendingMessages()` and `ctx.ui.getEditorText()` protect native readiness. The receiver checks `sessionManager.getBranch()` before confirming the custom message.

OpenCode uses the TUI's `api.route.current`, synced `api.state.session.status(sessionID)` and `api.client.session.promptAsync()`. It confirms by reading native messages and finding the exact full text. `/new` first displays the home route, so no result is claimed until a concrete native session is selected. An absent status is native idle semantics after the TUI state is ready; the version-pinned native status service explicitly uses this convention. Submission attempts are recorded before calling the native API and are not automatically replayed.

## Executed acceptance cases

| Case | Pi | OpenCode |
|---|---|---|
| r1, first result while idle | Passed | Passed, including empty native session |
| r2, second result arrives during a user turn | Held while busy, then passed | Held while busy, then passed |
| r3, third result from the same worker | Passed independently | Passed independently |
| r4, native new conversation | Correct new native session; absent from old history | Home route held; correct new session; absent from old history |
| r5, native resume of original conversation | Correct original native session | Correct original native session |
| Another owner's result | Never claimed or confirmed | Never claimed or confirmed |
| Repeated polling | Exactly one native body per result | Exactly one native body per result |
| Complete body in provider request | All 5 | All 5 |
| Native stored body and receipt identity | All 5 | All 5 |
| TUI source label, body lines and end marker emitted | All 5 | All 5 |

Native resume was exercised through Pi's actual `/resume` picker and OpenCode's native `tui.selectSession` API, which changed the TUI route. OpenCode's new-session command was executed through the native `tui.executeCommand('session_new')` path, not by rewriting app state.

Successful native persistence confirmation occurred 40–102 ms after the Pi receiver selected a result and 103–208 ms after the OpenCode receiver selected one. These figures come from local probes with 100 ms polling and mocked responses; they are not product latency benchmarks.

## Evidence and reproducibility

All runnable scripts and raw evidence remain under:

`/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/pi-opencode`

Run each independently from any working directory:

```sh
python3 /Users/gabrielvoicu/.consensflow/tmp/receiver-pull/pi-opencode/probe-pi.py
python3 /Users/gabrielvoicu/.consensflow/tmp/receiver-pull/pi-opencode/probe-opencode.py
```

Each creates a new isolated run directory, launches its own installed native TUI, runs assertions, prints JSON and terminates only the process group it created. Pi accepts trust for this isolated session only. The OpenCode local mock server listens on loopback, and the native test API uses a test-only credential.

Successful evidence directories:

- `pi-run-20260912-175635`: sessions `01a0961e-cbb3-70e5-9513-826e5e4dcbe9` and `01a0961e-efab-70e5-9513-827012cdceea`.
- `opencode-run-20260912-175654`: sessions `ses_f69e0e0c6ffeXKoJrhAd3xQk11` and `ses_f69e0c45fffekA2mUJKBmeEX4A`.

Within each: `report.json`, `inbox.json`, `confirmations.jsonl`, `receiver.jsonl`, `sessions.jsonl`, `provider-requests.jsonl` and `screen.bin`. Pi's native histories are in `sessions/`; OpenCode's native message API snapshots are `ses_*-messages.json`. `tui-text-evidence.txt` records the source/body/end fragments verified in ANSI-stripped native terminal output. It proves emitted rendering, not a human screenshot or that all results remain simultaneously visible in the final viewport.

Counterexample retained at `pi-run-20260912-175408`: `receiver.jsonl` shows `message_end` with `persisted:false` for r4/r5 followed by a second fetch. The original final assertion caught the duplicate r4 native entry. Current `pi-pull-extension.mjs` retains pending delivery and reconciles the native branch instead of retrying on that absence.

Initial environment/setup failures are also retained: `pi-run-20260912-175329` waited at isolated project trust; `opencode-run-20260912-175554` incorrectly treated the native absence of a status entry as non-idle. Neither is counted as a delivery success. They informed the final runner and version-correct native idle handling.

## What the result does and does not establish

The integration owns native selection locally, so Pi new/resume and OpenCode route changes do not depend on an external stale tab binding. Per-result IDs make second and third answers independent: a previously delivered first answer cannot hide newer pending answers from the same worker. A product UI can derive a worker badge from the number of result IDs without confirmed native-context inclusion, and show the results themselves in a durable inbox even when native transport is unavailable.

Pull does not eliminate native insertion, context limits, crash windows, ambiguous admissions, result ownership or receipt verification. The prototype is not a production outbox/claim service:

- No crash/restart fault injection, concurrent receiver lease tests, native switch exactly during submission, offline reconnection or high-volume backlog tests were performed.
- Pi's in-flight claim is process-local in this probe; production must persist claims and reconcile by result/native entry identity after a crash. OpenCode persists an attempted ID and fails closed rather than automatically retrying, but does not implement crash recovery reconciliation.
- OpenCode's native prompt API still receives a session ID. Moving its selection into the TUI removes the external resolver but does not make a route-switch/submission race impossible. An epoch/ownership rule is still required for in-flight work.
- Pi checks the native editor; this OpenCode prototype does not inspect arbitrary composer drafts. It uses the native prompt API without changing composer text. Draft preservation, modal behavior and contention need dedicated tests.
- Native transcript inclusion, model-request inclusion, visible TUI rendering and model acknowledgement are separate. The mock provider proves full context construction, not that a real model reasons about, summarizes or obeys the result.
- Payloads are short. Long results, multipart retrieval, compaction and context-window pressure remain untested.
- These probes cover Pi and OpenCode only. They cannot justify changing all four harnesses until Claude Code and Codex meet the same decision gate.
- Current ConsensFlow was left running unchanged. This is not installed-app acceptance or live delivery proof for ConsensFlow.

Recommendation for the shared decision: keep a durable result inbox and per-result unread/confirmed state regardless of transport. Prefer receiver-owned integration where native APIs can prove the current conversation, but retain the existing delivery ledger's distinction between waiting, admitted, included and uncertain. Evaluate all four harnesses before adopting this as the common production approach.

## Primary references

- [Pi official extension documentation](https://pi.dev/docs/latest/extensions): native lifecycle, settled event and displayed custom-message context. Installed 0.85.1 declarations: `/opt/homebrew/lib/node_modules/@earendil-works/pi-coding-agent/dist/core/extensions/types.d.ts:970` and `session-manager.d.ts:139`.
- Installed Pi behavior: `dist/core/agent-session.js:1084` defines custom-message insertion/queuing; `dist/core/messages.js:89` converts custom messages to model user messages. The probe verifies their actual runtime behavior.
- [OpenCode 1.18.30 TUI plugin specification](https://github.com/anomalyco/opencode/blob/v1.18.30/packages/opencode/specs/tui-plugins.md): plugin isolation, current route, synced session state, native SDK client and lifecycle cleanup.
- [OpenCode 1.18.30 plugin API types](https://github.com/anomalyco/opencode/blob/v1.18.30/packages/plugin/src/tui.ts): version-pinned public integration surface.
- [OpenCode 1.18.30 native status service](https://github.com/anomalyco/opencode/blob/v1.18.30/packages/opencode/src/session/status.ts#L27): missing status means idle, and idle status removes the stored busy entry.
