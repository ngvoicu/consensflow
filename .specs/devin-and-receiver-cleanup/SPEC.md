---
id: devin-and-receiver-cleanup
title: Devin roles and receiver integration cleanup
status: paused
created: 2026-09-12
updated: 2026-09-13
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
- [x] [IMPL-PROC-02] Stop launch-owned children using native process identity;
  satisfy TEST-PROC-01 without global process-name matching.
- [x] [TEST-DEV-03] Test private native role hooks and shared-inbox collection,
  including stale events, repeated replies and uncertain receipt boundaries.
- [x] [IMPL-DEV-04] Implement the private Devin hook adapter and installation;
  satisfy TEST-DEV-03 without changing global native settings.
- [x] [TEST-DEV-05] Test native launch/binding/completion/resume and catalog/UI
  integration, including cancellation and scoped advisor/coordinator authority.
- [x] [IMPL-DEV-06] Implement Devin worker/advisor and coordinator integration;
  satisfy TEST-DEV-05 and preserve the native terminal UI.
- [x] Consolidate private integration installation and remove obsolete paths.
- [x] [TEST-READ-07] Reproduce stored reads blocked behind a background scan and
  repeated timer requests building an unbounded scan queue.
- [x] [IMPL-READ-08] Serve stored reports immediately and coalesce scan requests.
- [x] [TEST-PASTE-09] Reproduce oversized paste writes and pasted newlines generating submission events.
- [x] [IMPL-PASTE-10] Stream large pastes in order with bounded writes and preserve bracketed-paste boundaries.
- [x] Run focused regressions, all applicable existing gates and private native checks.
- [x] Prepare a separate verified candidate and document remaining acceptance limits.
- [x] [TEST-RECOVERY-11] Reproduce a failed state refresh removing live session
  views; verify recovery after both an existing session and a failed first load.
- [x] [IMPL-RECOVERY-12] Preserve the last successful session view on refresh
  errors and retry without requiring another session event. Verify and update
  only the separate candidate; investigate the reported native restart separately.
- [ ] [VERIFY-PASTE-13] ← current Capture the reported whole-app paste restart in an
  isolated candidate or obtain a matching installed diagnostic, then repair and
  verify the confirmed cause. Never provoke it in the user's running sessions.
- [x] [TEST-ROUTING-14] Verify lead and PM startup context contains only their
  saved eligible roster, model identity, effort, capabilities, route and correctly
  matched benchmark evidence across all coordinator harnesses.
- [x] [IMPL-ROUTING-15] Reuse catalog profiles in both role contexts; teach task
  selection and independent cross-model review of substantial coordinator,
  worker and advisor output. Preserve role permissions and native defaults.
- [x] [VERIFY-ROUTING-16] Run affected checks and update only the private candidate.

- [x] [TEST-TIERS-17] Reject obsolete skill-discovery settings in persisted output;
  preserve native skill discovery and unrelated legacy agent fields.
- [x] [IMPL-TIERS-18] Remove skillsPolicy/skillPaths plumbing and clean old keys on save.
- [x] [TEST-TIERS-19] Verify four model/effort defaults, saved tier overrides,
  coherent category tags and name-independent lead/PM tier instructions.
- [x] [IMPL-TIERS-20] Share critical/complex/standard/light profiles and validate
  user overrides without letting preset sync overwrite them.
- [x] [TEST-TIERS-21] Refuse critical-tier dispatches without an allowed purpose;
  cover initial tasks and follow-ups through CLI and controller admission.
- [x] [IMPL-TIERS-22] Require explicit critical review/architecture/hard problem/
  important question purpose; attach no-code guidance to every critical task.
- [x] [TEST-TIERS-23] Verify tier filters, grouping, pills and editable overrides
  in Your agents and Agent library, including shared model/effort cards.
- [x] [IMPL-TIERS-24] Render consistent task and role tags plus the work tier;
  remove Coding and coordinator recommendations from critical-tier entries.
- [x] [VERIFY-TIERS-25] Run affected automated gates and refresh only the private
  signed candidate; retain installed app and live-state hashes.

## Four work tiers and tag cleanup, 2026-09-13

This extends the authorized roster/skill work. These are owner allocation policies,
not prices or benchmark rankings. Defaults match curated model identity and effort,
never agent names: Astra/Fable max or ultra = critical; high/xhigh = complex;
medium = standard; low = light. Kimi K3 = complex. Opus/Sol = standard (low = light).
All remaining choices default to light; custom/unknown native models require the
coordinator to verify suitability, and images remain image-only. An optional
saved workTier overrides the derived default and survives model edits and sync.
The complete effective profile remains persisted. Clearing the override restores
model/effort defaults. All routes for one curated model/effort share defaults.

Critical work is reserved for consequential reviews, architecture, hard-problem
analysis and important questions. No coding, routine advice, or automatic lead/PM
recommendation. Both coordinator skills use tier labels without hardcoded agent
names, select the least costly sufficient tier, and reserve critical escalation
for important work after lower-tier analysis when useful. Cross-model review still
covers substantial coordinator, worker and advisor output; ordinary reviews use
lower tiers. PM alone writes specs; advisors and critical specialists return advice.

Dispatch requires --purpose critical-review|architecture|hard-problem|important-question
for every critical initial task/follow-up and attaches explicit no-edit guidance.
This validates declared purpose, not semantic truth or native tool sandboxing.
Other tiers keep ordinary cf run/cf say behavior. Roster/API edit validates exactly
four tier values. UI uses separate tier, task-capability and recommended-role pills,
a shared tier filter/group option, and an Automatic option in the saved editor.

Remove obsolete skillsPolicy and skillPaths from live source/preset/normalization
paths, preserving historical fixtures as migration inputs and cleaning keys on
explicit saves/profile refresh. No write to the user's current agents.json during
development. Tests run with private temp/profile paths; no external inference.

## Tier work log

| Task | Red | Green | Refactor |
|---|---|---|---|

## Model selection and independent review, 2026-09-13

The owner wants lead and PM coordinators to choose saved agents according to
their task, model abilities and reasoning effort. Cross-model reviews apply to
substantial work produced by coordinators, workers and advisors, not merely the
coordinator's own implementation/specifications. The owning coordinator requests
and resolves reviews; advisors remain read/research/test-only and the PM alone
writes or revises specifications. No native subagent delegation or cross-owner
access is introduced.

Use the existing generated role context and catalog profiles, not a separate
routing service. Both lead and PM receive a compact saved-roster capability table
at launch. Refresh discovery with `cf agent list --json` before new allocation
decisions when the snapshot might be stale. Saved choices are not a guarantee of
installed harness health, credentials or remaining quota; never change routes,
effort, billing or roster entries as a side effect.

Select for task/domain/complexity and role restrictions first; use task-relevant
benchmarks as secondary evidence, keeping model identity, effort, source, date,
index version and unspecified-reasoning labels. Missing scores are unknown, not
zero. Distinguish canonical model identity from agent name, provider and harness.
Prefer a different model family for review; another route or effort of the same
model is not independent. If model identity or an eligible alternative is
unavailable, state that limitation and continue safe local validation without
claiming independent review or creating agents.

This is coordinator instruction/context, not an automatic dispatch scheduler or
proof that a live model obeyed it. Source/context tests and private packaging are
required; live delegation quality remains a separate acceptance observation.

Routing acceptance: full Node 1,113 passed, six gated skips, zero failures;
CLI/role/skill focused 77 passed; packaged smoke 2 passed; changed-file Biome and
diff checks pass. Full integration initially had 17 passes and two failures
(early results-index assertion; concurrent worker launch reservation). Both passed
in the focused rerun (9 tests), then all 19 passed with test-file concurrency 1;
intra-test concurrent request checks remain enabled. Preserve the initial failures
as unresolved timing evidence, not a claim of proven concurrent-suite stability.
Logs are `routing-*.log` under `~/.consensflow/tmp/receiver-pull/`.

Five changed runtime files were copied only into the existing private candidate,
then its strict signature and all 62 CLI source hashes verified. UI/Rust are
unchanged by routing work; their earlier acceptance is not represented as rerun.
Candidate README and BUILD/verification records include current checks and limits.
Installed hashes and running PID 57943 remain unchanged; no current session or
saved agent was modified. The unavailable advisor tool was checked before the
approach and final verification; source review and regression tests were used.

## Installed paste failure follow-up, 2026-09-13

The owner identifies installed alpha.61, Claude Code, approximately 22,000
characters, and the whole application closing/reopening without its session list.
The owner does not know whether it occurs before or after Enter, so both paths
are tested. The private candidate has
not been installed. Read-only inspection confirms the current saved poker-lab
lead with 14 panes remains in tabs.json. No relevant macOS crash report was found;
the observed app exit/relaunch has no established paste trigger. Main/editor
stderr goes to /dev/null, so this inspection cannot establish a panic cause.

A distinct reproducible source defect is in the state refresh: an error is
normalized to an empty tabs array, erasing visible sessions and retiring their
terminal views. Preserve the successful view, report the failure, and retry one
request at a time. A valid empty snapshot must still remove deleted sessions.
This does not establish or fix the cause of the reported application restart.

Recovery implementation: errors no longer replace the last successful snapshot;
one retry is scheduled after the failed request, with existing request coalescing
preserved. Successful recovery clears its own error. Startup retries also work;
valid empty snapshots still retire deleted panes. Three RED regressions now pass;
full browser suite 126 passed, packaged smoke 2 passed (including the exact
630,012-byte paste), changed-file Biome and diff checks passed.

Stock Claude TUI probe, with a private HOME/config and loopback mock provider:
22,000-character ASCII (22,000 bytes) and Unicode (35,200 bytes) pastes both
remained unsubmitted until Enter, then reached the provider intact. No native
process exit occurred. This was a native PTY probe, not a reproduction through
ConsensFlow's WebKit. Evidence: `~/.consensflow/tmp/receiver-pull/`
`paste-22000-myd7bsjm/result.json`. The reported restart remains unresolved.

The separate Devin candidate was rebuilt with its own bundle identifier and
strict signature verified. All 62 CLI bundle/source files still match; executable
and UI hashes are recorded in its BUILD/verification JSON. Its launcher now saves
app/editor stderr plus start/PID/exit status under its private `profile/logs/`.
Installed app hashes are unchanged and PID 57943 continues running; no installed
session data was modified. Repository `app/src-tauri/target/debug` remains absent.
Logs: `recovery-red.log`, `recovery-green.log`, `recovery-ui.log`,
`recovery-build.log`, `recovery-smoke.log` under the same private temp root.

### Complete app/native paste probe

An isolated packaged probe uses the production app, frontend, xterm, Rust IPC,
PTY, receiver integration and stock Claude TUI. Only the private self-test driver
and bundle identity differ; all other frontend assets match source. It dispatches
clipboard events through xterm's textarea (not an OS-level Cmd-V), waits without
Enter, then submits through the normal input path. A loopback mock provider
verifies complete bodies; visible xterm replies establish the return path.

Five 22,000-codepoint cases passed: ASCII, mixed Unicode, wide emoji, CRLF and a
repeated paragraph paste in the same conversation. Input sizes were 22,000,
35,170, 87,874, 22,000 and 22,000 bytes. None reached the provider before Enter;
all reached it intact afterward (with normal CRLF normalization), and all five
replies rendered. App PID 93372 remained unchanged, session/pane identity remained
present, no UI errors appeared, and normal shutdown exited zero and stopped the
native child. No real account or remote inference was used.

The first probe stopped at native login because production subscription launch
correctly removed inherited API credentials. After setting the dummy credentials
explicitly inside the private wrapper, all five cases passed; this setup failure
is retained, not counted as a product defect. Evidence is under
`~/.consensflow/tmp/receiver-pull/paste-app-probe/`: initial `run-g27umji7`, passing
`run-r2a_32lz/result.json`, source/hash `verification.json`, private driver and
local provider scripts. Installed binary/config hashes remain unchanged; its PID
57943 continues running. No new production changes were made for this probe.

VERIFY-PASTE-13 stays open: the reported intermittent installed restart is still
unreproduced. The candidate launcher retains stderr and exit diagnostics for a
future occurrence; successful isolated cases do not establish a crash fix.

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
| TEST-RECOVERY-11 | Three browser failures: both error/unavailable snapshots remove the current pane, and first-load failure never recovers without another event. An initial incorrect status selector was corrected before recording this RED. | — | — |
| IMPL-RECOVERY-12 | — | Three focused regressions; full browser 126; rebuilt packaged smoke 2, all passed. | Changed-file Biome and diff checks pass. Native restart reproduction remains VERIFY-PASTE-13. |
| TEST-ROUTING-14 | Focused role/skill run: 24 passed, 15 failed. Ten coordinator startup cases lacked capability/review context; PM had no roster; effort, benchmark scope and model identity were absent. | — | — |
| IMPL-ROUTING-15 | — | Role/skill 39 passed, zero failed, including Claude Code/Codex/OpenCode/Pi/Devin lead/PM/advisor startup. | Reused catalog profiles and benchmark metric definitions; shared selection/review guidance, no new routing service or files. |
| Routing CLI follow-up | PM discovery regression failed with an administration refusal; existing tests caught missing raw model ID and explicit authorization wording. | CLI/role/skill 77 passed after allowing only read-only agent listing, preserving configured IDs and authorization scope. | Roster reads skip automatic role-file refresh; PM add/edit/remove/sync remain blocked. |
| VERIFY-ROUTING-16 | Initial full Node retained three context-contract failures; initial concurrent integration 17 passed / 2 failed. | Corrected Node 1,113 passed / 6 gated skips; integration focused 9 then sequential-file full 19 passed; packaged smoke 2 passed. | Private candidate signed, 62 bundled source hashes verified; live model decision quality remains untested. |
| TEST-PROC-01 | cargo test pty::tests::close_stops: 3 tests, 3 failed; drop_stops_detached_children: 1 test, 1 failed. Detached children survived. | — | — |

| IMPL-PROC-02 | — | Full cargo test: 97 unit + 16 headless passed. Clippy all-targets with -D warnings passed. | Reviewed private ownership helper and formatted only new test code; focused PTY rerun 25/25 passed. |

| TEST-DEV-03 | node --test tests/devin-receiver.test.mjs: 1 failed test file; ERR_MODULE_NOT_FOUND for devin-receiver.mjs. Ten hook/config/role scenarios specified. | — | — |

| IMPL-DEV-04 | Two additional edge cases failed: partial log tail and unavailable app suppressing role instructions. | Hook/role/receiver 44 passed. Actual product helper on installed 3000.6.14 and private 3000.10.21 passed repeated replies, exact native context, idle next-prompt collection, new and resume. | Formatted helper/installer; malformed complete log lines still fail closed. |
| TEST-READ-07 | Stored report read timed out behind a blocked scan; 50 overlapping requests produced 51 scans. | — | — |

2026-09-12 new owner report: Calliope's `cf read` stayed alive for minutes.
Read-only inspection found the complete 20,802-byte report in legacy `d-185`,
while the installed reader serialized requests behind background delivery scans.
During inspection the app was restarted externally and that command returned
`Watcher is closed`. No live processes/settings/marks were changed here. A verbatim
recovery copy is in `~/.consensflow/recovery/20260912-calliope/calliope-review.md`.
The exact stuck installed operation was not captured before restart. A candidate
regression reproduces both stored-read blocking and an accumulating scan queue.
Focused fix: inbox/receiver 30 passed. Full Node suite: 1,090 passed, six gated
skips, zero failures (inbox-stall-full.log).

Product TUI probe evidence: `~/.consensflow/tmp/devin-product-probe/`
`probe-1789243779263768000` (3000.10.21),
`probe-1789243841723584000` (3000.6.14). Both run the actual immutable helper,
a stock TUI and loopback mock provider/inbox; no real account or app state used.
A raw SQLite revision-count assertion was corrected to traverse the native main
chain, matching the already documented native storage contract.

| IMPL-READ-08 | — | 30 focused inbox/receiver passed; full Node 1,090 passed, six gated skips, no failures. | Biome formatting and focused regression passed. |
| TEST-PASTE-09 | Browser: two failures (one 720 KB input frame; oversized-refusal test received 65,537 bytes). Rust: one failed test because 2,000 pasted lines emitted Enter events. | — | — |

New owner report: large TUI pastes sometimes restart ConsensFlow. Read-only macOS
inspection found no ConsensFlow crash report. Logs record app exit handlers and
subsequent launches but do not establish the trigger. The owner was asked which
harness, approximate size and whether the whole app or only its terminal resets.
Confirmed independent faults: frontend passes the entire paste despite Rust's
64 KiB per-write limit; the arbiter emits Enter for CR inside bracketed paste.
Two browser regressions pass with bounded ordered chunks. Full UI: 123 passed;
full Rust: 98 unit + 16 headless passed; Clippy all-targets passed. Do not claim the reported restart itself reproduced.
Only a small relevant system-log excerpt is retained under the private temp root.

| IMPL-PASTE-10 | — | Full UI 123 passed; full Rust 98 unit + 16 headless passed; Clippy all-targets passed. | Formatted changed UI and arbiter; repeated full gates after formatting. Native app restart cause remains unconfirmed. |

## Resume context

Child cleanup is implemented and verified in source; installed app unchanged.
The first full Rust run exposed a reaping hang after native process metadata
became unreadable during exit. Preserving the existing process-group teardown
fallback fixed it; the diagnosed private test process was explicitly terminated
before rerunning. Logs: `~/.consensflow/tmp/receiver-pull/child-process-*.log`.
Devin native completion/launch integration is implemented. Minimum CLI 3000.10.21 is enforced; installed 3000.6.14 is unchanged.
Stored-read stall fix passed full Node suite. Large-paste regressions took priority;
ordered chunks and Rust bracketed-paste parsing passed all applicable gates.
Native receiver and worker probes passed against the private latest CLI. All
applicable gates pass. The separately signed candidate is complete; user live
acceptance remains pending and the installed app is unchanged.


2026-09-12 Devin implementation evidence:
- TEST-DEV-05: initial five completion/selection tests failed with unknown Devin;
  two launch/channel tests then failed. Catalog/default-model/diagnostic tests
  failed until the missing integration paths were added. The single-harness UI
  check exposed another missing allowlist entry and now passes.
- Native completion requires the canonical SQLite main chain, stable message IDs,
  matching native `turnClientMessageId`, streamed final text and an explicit
  complete boundary. A streaming-message UUID is not a database message UUID.
  Installed 3000.6.14 lacks the request identity; private 3000.10.21 provides it.
  Launch refuses older versions with an actionable message. No global native
  binary or settings were updated.
- Production Inbox/Store/Watcher with the stock TUI confirmed all three initial
  parts and one late part received exactly once, then exact /new and /resume
  selection. Probe: `~/.consensflow/tmp/devin-product-probe/probe-1789245499500905000`.
- Worker TUI: first, second and resumed third answers retain their IDs and index
  once. An independent session answer and cancelled fifth inference stay out.
  Probe `probe-1789245570096144000`; redacted native fixture retained in tests.
- App controller tests cover Devin idle lead/PM, private hooks, receiver scope,
  advisor restrictions and exact worker resume. Catalog uses native configured
  model with no invented effort, score or lead/PM model recommendation.
- Full Node: 1,105 passed, six gated skips. Real bridge: 19 passed. Browser:
  122 passed in the final full run; the remaining catalog group-count expectation
  was corrected for the added model and its focused test passed (123 scenarios).
  Lint passes with informational style suggestions; no errors. Existing Rust
  sources remain at 98 unit + 16 headless passing; Clippy passed. Packaged smoke
  passed both app and catalog checks before adding native large-paste acceptance.
- The `advisor` tool is not available in this environment (metadata search before
  implementation and completion); local source/ownership review was performed.


## Final candidate acceptance

Source work and private candidate completed. Candidate:
`~/.consensflow/candidates/devin-20260912/ConsensFlow Candidate.app`.
Use its sibling `Launch candidate.command`; this selects the private profile and
private Devin 3000.10.21. Its own PATH is retained instead of replaced by login
shell startup. Six test presets are saved; no live sessions or credentials copied.

- Both packaged smoke checks pass. The main-window check now sends a 630,012-byte
  bracketed Unicode paste through the real WebKit page, IPC and raw PTY child.
  SHA-256 and byte count match exactly; subsequent work and clean exit succeed.
  This verifies bounded input in the built candidate. It does not establish the
  trigger for the owner's previously reported restart.
- All 62 bundled source files match the checkout. Deep/strict signature and
  launcher syntax checks pass. `verification.json` retains hashes and exact gate
  counts. Lint passes with informational suggestions; `git diff --check` passes.
- Native Devin production-inbox and worker probes use loopback mock providers.
  No paid accounts were used. Minimum-version behavior, first/second/resumed-third
  replies, cancellation, late results, /new and /resume are covered.
- The repo debug directory remains absent. Build output stays in the private
  ConsensFlow build directory. The installed app, original runtime profile,
  current sessions and global Devin binary were not modified.
- Live user acceptance and any public release/install are separate. Stock Devin
  still collects truly idle late results only on the next human prompt, as approved.
  Large-paste restart cause remains unconfirmed; no false claim of reproducing it.

| TEST-DEV-05 / IMPL-DEV-06 | Native completion/launch/catalog and UI allowlist failures recorded in devin-*-red.log. | Full Node 1,105 passed / six gated skips; 19 bridge; 123 browser scenarios; native worker and production-inbox probes passed. | Reused Store/Inbox/Watcher, role installation and native TUI; no new dependency or second receiver system. |
| Final private candidate | — | 114 Rust; Clippy; lint; packaged smoke 2/2 including exact 630,012-byte paste; 62 bundle/source hashes; strict signature. | Candidate and profile isolated under ~/.consensflow; installed app unchanged. |

Tier cleanup RED: roster and both runner suites fail on obsolete fields/--no-skills as expected; tiers-cleanup-red.log.

| TEST-TIERS-17 / IMPL-TIERS-18 | 95 tests: 4 failed | 92 passed, 3 platform skips | same 92 passed; removed unused list/enum normalizers and preset defaults |

Core tier RED recorded in tiers-core-red.log: missing workTier and unchanged coding/role recommendations.

| TEST-TIERS-19 / IMPL-TIERS-20 | 61 tests, 4 failed | 61 passed | 61 passed; reuse profiles for tier defaults and role context |

Dispatch RED: CLI guard missing; the controller fixture first omitted its tab and failed authorization. After correcting that fixture, policy errors returned HTTP 500 until mapped to PaneError/HTTP 400. All intermediate outputs are retained in tiers-dispatch-*.log.

| TEST-TIERS-21 / IMPL-TIERS-22 | missing CLI purpose guard; controller test fixture initially lacked tab | 2 passed after fixture correction and mapping policy errors to HTTP 400 | 181 CLI/pane regressions passed |

UI RED: Work tier control absent in real page, 1 failed. Test output initially landed in the existing repo test-results directory; subsequent runs use the private output path.

| TEST-TIERS-23 / IMPL-TIERS-24 | 1 failed: absent tier control | 1 passed: real HTTP/browser tier, tags, filters, grouping and persisted override | 33 administration/browser tests passed after updating one obsolete lead-tag expectation |


Tier verification review: additional input-boundary regression rejected arrays,
objects, prototype keys and empty strings (RED: 1/26 failed; GREEN: 26 passed).
Legacy roster discovery now computes current tier/tags in memory without writing
(RED: 1/27 failed; GREEN: 106 roster/CLI/role/skill checks). Static role prose uses
policy labels, never the owner's agent-name examples; dynamic roster names remain
available for addressing actual saved agents. Advisor/Context7 tools are unavailable
in this tool surface; no external library or paid inference was needed.

First full Node run: 1,118 passed, six gated skips. Repeated full run: 1,117 passed,
one failed, six skips: TEST-PANE-109 read an absent reserved.channel in the native
Codex fixture. The isolated test passed immediately. Its precise historical timing
cause is unproven; failure evidence stays in tiers-final-full.log and the rerun in
tiers-native-repeat.log. The final sequential-file full run passed: 1,119 tests, six gated skips, zero failures (1125 total, exit 0).

Private packaged verification: updated catalog smoke first failed against the old
candidate (missing saved workTier), then both smoke tests passed against the new
candidate, including real native pane input and exact 630,012-byte Unicode paste.
61 source files match byte-for-byte; the packaged package.json intentionally contains
only runtime manifest fields and is verified separately. No binary rebuild or change
to installed ConsensFlow is required for these JavaScript/catalog changes.


## Latest verification and resume context, 2026-09-13

Four-tier assignment and tag cleanup are complete in source and the private signed
candidate. Final Node 1,119 passed / six gated skips / zero failures, integration
19/19, browser administration 33/33, packaged smoke 2/2, focused roster/CLI/role/skill
106/106. Changed-file Biome has zero errors (one pre-existing style suggestion in
the paste smoke), and git diff --check passes. The sequential-file run preserves
intra-test concurrency. Earlier failures and fixture corrections remain documented
above; do not treat those trials as passing. Live model adherence remains untested.

The candidate has 61 exact source copies plus a reduced runtime manifest matching
source metadata; strict signature verified. Six private roster profiles refreshed,
with obsolete skill keys removed on write. Source/native UI binary is unchanged by
this task; no Rust rebuild was necessary. Installed binary/Info.plist/watcher
hashes, the live agents.json hash, and installed PID 57943/start time are unchanged.
No global harness configuration, live sessions, release or commit was changed.
Candidate launcher: ~/.consensflow/candidates/devin-20260912/Launch candidate.command.

Remaining task is VERIFY-PASTE-13 only: obtain a reproducible isolated case or
matching diagnostic for the reported intermittent whole-app restart. Existing
22,000-character/local-provider and 630,012-byte packaged paste checks pass but do
not establish the reported crash's cause. Keep installed ConsensFlow untouched.


## Bundled role skill cleanup, 2026-09-13

Authorized by the owner's request to review, simplify and remove unnecessary
skills code. Keep installed ConsensFlow, its roster and running sessions untouched.
Only source and the separate private candidate change.

The role loader already injects complete context on fresh/resumed pane launch.
Remove the redundant lead-only install/manifest/staleness lifecycle, skill
administration CLI/HTTP routes, and unused global-payload removal code. Keep
private launcher/receiver preparation and read-only legacy-hook diagnostics.
All three roles come from one generation entry point. Roster edits take effect
through live roster reads and the next pane launch, without rewriting role files
on unrelated CLI commands. Existing private/global files are not swept or migrated.

Consolidate shared coordinator dispatch/result/model/review instructions. Preserve
explicit-user-only manual fetching, automatic full-result consumption, immutable
multipart reads, no polling/unsafe retries, role ownership, PM-only specification
authoring, four work tiers, critical-purpose guards and cross-model review of
substantial coordinator and delegate output. Advisor remains read-only.

| Task | State | Verification |
|---|---|---|
| TEST-SKILLS-27 | complete | Regressions for absent admin routes, read-only CLI/roster operations, launch-time refresh and global-file preservation. |
| IMPL-SKILLS-28 | complete | Remove duplicate lifecycle and dead retirement code; retained setup/receiver integration checks pass. |
| TEST-SKILLS-29 | complete | Shared lead/PM result rules and all three generated roles, current roster and full startup injection. |
| IMPL-SKILLS-30 | complete | Concise role templates and shared coordinator guidance; no obsolete skill-policy/install advice. |
| VERIFY-SKILLS-31 | complete | Focused/full Node, integration, relevant browser and signed candidate smoke; source hashes and installed canaries. |

Advisor and Context7 tools are not available in this tool surface. No new library
or native harness API is required; native startup adapters retain their verified
full-instruction injection mechanisms.

Lifecycle RED is retained in skills-lifecycle-red.log: existing commands rewrite role files, setup generates a lead-only copy, and hidden skill endpoints remain callable.

Context RED: 39 passed, three failed (missing shared PM policy, unsupported advisor generation, redundant role writes); skills-context-red.log. Initial lifecycle GREEN: 99/101 passed; two obsolete test fixtures needed explicit role-directory setup and HTTP 404 expectation.

Skill cleanup focused GREEN/refactor: 115/115 pass, exit 0. All three generated
skills pass Skill Creator validation. Full startup injection remains covered for
Claude Code, Codex, OpenCode, Pi and Devin; role documents refresh at launch from
the current roster and avoid unchanged writes. Previous exact-wording tests were
adjusted to the retained semantics. No model behavior trial is claimed.

Initial full run: 1,096 passed, four failed, six gated skips. Three failures were
obsolete CLI/help or exact skill wording expectations; updated to the current
command set and preserved authorization semantics. A Pi duplicate-followup test
observed an empty send list; its isolated rerun passed without production changes.
The initial failure remains in skills-full.log; do not treat it as passing or
claim its historical timing cause is proven. Metadata/help RED and the first
filtered rerun (fixture initially missed two executable stubs) are retained.

Refactor verification: 130 passed / two platform skips, including harness
metadata removal, actual CLI help contract, preserved authorization, and Pi
follow-up checks. The historical duplicate-followup failure was not reproduced;
no Pi production code changed. Browser 33/33 and integration 19/19 already passed.


## Skill cleanup acceptance, 2026-09-13

All five skill-cleanup tasks are complete in source and the separate signed
candidate. Final Node: 1,099 passed, six gated skips, zero failed (1,105 total,
exit 0). Final integration 19/19, browser administration 33/33 and packaged smoke
2/2 pass. All three generated roles pass Skill Creator validation; changed-file
Biome and git diff --check pass. Initial failing trials are retained above and
in skills-*.log; no Pi production code was changed for the intermittent assertion.

Lead startup policy shrank from 9,762 to 7,647 characters without a roster. PM
context is 9,378 characters (previously 8,830) because it now has the complete
shared result/authority rules. Advisor remains a concise read-only role. These
measurements and startup/contract checks do not claim live model adherence.

The candidate contains 60 exact owned source files plus the reduced runtime
package.json and 19 verified dependency files. Removed sync/manifest modules are
absent from the bundle; strict signature and executable hash verify. Installed
binary/Info.plist/watcher hashes, live roster hash and PID 57943/start time remain
unchanged. No global harness configuration or live sessions were modified.

Use ~/.consensflow/candidates/devin-20260912/Launch candidate.command to test this
private build. Only VERIFY-PASTE-13 remains open in the wider spec: the reported
intermittent installed whole-app restart still has no proven trigger.


## Automatic pane activity and permanent deletion, 2026-09-13

User confirmed automatic detection. Show Working only from native task evidence,
Idle only from positive settlement; unavailable or stale evidence is Unknown.
Starting, Closed and Failed stay distinct. Reuse the existing background native
scanner for lead, PM, worker and advisor activity, scoped to exact pane generation
and current receiver/binding. Do not block page reads on native history or change
result/receipt state to paint a status. No terminal-output heuristic or new plugin.

Expose deletion in pane headers as well as the sidebar. Workers/shells delete one
pane; PM deletion removes PM and advisors; lead deletion explicitly deletes the
session. Confirm the scope. Closed panes must delete without trying to kill a
missing process. Invalid host lists, unresolved launches or a still-running process
after failed stop must not silently remove the saved entry. Native histories and
project files remain intact. Keep the installed app and live profile untouched.

| Task | State | Verification |
|---|---|---|
| TEST-PANES-32 | complete | Closed/live deletion, exact generation and stop failure regressions. |
| IMPL-PANES-33 | complete | Confirm process absence and expose scoped Delete controls. |
| TEST-PANES-34 | complete | Native Working/Idle transitions, identity changes, unknown/stale state and UI updates. |
| IMPL-PANES-35 | complete | Background activity projection and readable header/sidebar labels. |
| VERIFY-PANES-36 | complete | Focused/full Node, browser, signed private candidate smoke and installed canaries. |

Acceptance is source and isolated candidate verification. Live user testing follows.

Pane deletion RED: one passed, two failed (missing process kill and malformed host
list). GREEN: 3/3. Activity RED: absent projection; GREEN confirms active/settled
turns, every follow-up, lead/PM separation, receiver change, generation replacement,
stale expiry, unreadable native state and closed/starting/failed distinctions.
UI RED: both new scenarios fail for absent labels/header controls; GREEN 4/4,
including existing closed-worker and confirmation coverage. Focused regression:
227/227, including the supported native parser contracts. Logs: panes-*.log in
~/.consensflow/tmp/receiver-pull. No live model requests or installed mutations.


## Pane activity and deletion acceptance, 2026-09-13

All five pane tasks are complete in source and the signed private candidate.
Full Node: 1,102 passed, six gated skips, zero failures (1,108 total, exit 0).
Integration: 19/19. Full browser: 129/129; final focused UI: 2/2. Packaged smoke:
2/2. Changed-file Biome and git diff --check pass. No Rust implementation changed;
the production binary was rebuilt to embed the current UI. Strict signature, all
80 bundled file hashes and 13 UI source hashes verify.

Working/Idle uses the existing native parser contract. Lead/PM activity follows
the currently registered receiver; worker/advisor activity requires the exact
reservation/binding generation. Unknown covers absent, unreadable, replaced or
stale evidence. Cache expiry is ten seconds, including when the scanner stalls;
page/result reads remain independent of native scans. Starting/Closed/Failed are
separate, and ordinary shells show Open. No manual activity toggle or extra native
plugin was introduced. Native model live acceptance has not been performed.

Closed-worker deletion regression is fixed. Host lists are validated; only the
requested process generation is killed, and a failed kill is harmless only after
a second valid list confirms it absent. Stop failure with a present process keeps
the saved entry. Header and sidebar controls retain captured identities and
explicit confirmation scope. Native histories and project files remain intact.

The initial full browser run retained 128 passes/one failed viewport measurement
across resize frames. Its measurement was made atomic and then checked against
the viewport. Visual inspection also found names squeezed to 33 pixels by badges;
a new failing width assertion led to status/actions below pane names. Final wide
and 560-pixel window screenshots were inspected. A screenshot taken before resize
settled was replaced by a check that waits for controls to fit the viewport. No
extra production resize change was needed. All initial logs/screenshots remain.

Installed executable/Info.plist/watcher hashes, live agents.json and installed
PID 57943 (start Sun Sep 13 07:33:23 2026) remain unchanged. No install, publication,
commit, global harness change or real account trial was performed.

Candidate: ~/.consensflow/candidates/devin-20260912/Launch candidate.command.
The only remaining task in the wider spec is VERIFY-PASTE-13: the reported
intermittent installed whole-app paste restart remains unreproduced.


Paused on 2026-09-13 for the authorized session-task-board feature. The existing
private candidate work is preserved. The intermittent installed large-paste
restart remains unreproduced; no installed-app changes are authorized here.
