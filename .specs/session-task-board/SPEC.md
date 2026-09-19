---
id: session-task-board
title: Session task board and operational graph
status: completed
created: 2026-09-13
updated: 2026-09-13
priority: high
tags: [tasks, graph, pm, lead, visibility]
---

# Session task board and operational graph

The user requests a task board for each session combining PM/advisors and
Lead/workers, with a graph and operational visibility. Implement in the private
candidate. The installed app, live roster and sessions remain untouched.

## Decisions and scope

- A Tasks view beside PM/Lead opens the parent session's combined overview.
  Board and Graph share task data and filters. PM/Lead ownership stays separate;
  only the human app can view both groups together.
- Native Working/Idle/Failed/Unknown, declared task progress, and per-result
  receipt are independent. A reply or idle pane never implies task acceptance.
- Task states: Planned, In progress, Blocked, In review, Accepted, Cancelled.
  Lead/PM maintain their own tasks via cf task; the user can create/edit/correct
  tasks and answer questions. Changes use IDs and revisions inside Store.mutate.
- Capture one automatic assignment record per conversation when dispatching
  going forward. Keep the initial assignment and bounded followup history.
  Explicit tasks describe coordinator work or a distinct task; conversation
  links show all conversation replies without claiming each reply belongs to
  a particular request. No nth-request/nth-reply inference.
- Existing conversations appear with their actual names and a clear indication
  that the original assignment was not recorded. Never invent historical tasks.
- Tasks may link dependencies and reviews within their owner's group. The graph
  shows the whole session and both ownership trees; links do not schedule work,
  grant permissions, launch models, or send messages.
- Questions and answers have immutable IDs and revision-protected edits.
  Answering records a decision for the coordinator to read; it does not type
  into terminals or automatically launch a model turn.
- All task state is stored in each owner's tab record under ConsensFlow home,
  through the existing atomic store. No new database, project bookkeeping,
  global settings, ACP host, or delivery mechanism.
- Paginate task summaries and fetch full task/history only on demand. Use
  bounded text and arrays plus a 512 KiB aggregate edit limit. Automatic dispatch
  trims older history to preserve recent evidence below the native frame limit.
  Capture and review links preserve historical provenance and immutable conversation identity.
- Operational overview shows actual native activity, latest update, recorded
  failure reason, declared blockers/questions, model/harness when available,
  and every unconfirmed reply. No invented cost/context/quota numbers.
  Resource telemetry is shown only if actual normalized data is available;
  this change does not introduce new harness telemetry collectors.
- Interactive SVG/HTML graph with stable owner lanes, task/review/dependency
  links, fit/zoom controls, keyboard-selectable nodes and task detail.
  No raster image needed. No new runtime dependency.
- Preserve terminal instances, output streams and drafts when switching views.
- User asked to improve the application, authorizing implementation after
  forging this spec. Optional automatic/manual preference was asked; default
  automatic capture plus manual corrections follows existing editable controls.

## Design

Use the existing teal palette and system text/monospace pairing. PM uses its
existing lavender accent; Lead uses teal. Amber means attention, red failure;
status is always text as well as color. The combined graph's distinguishing
feature is two aligned owner lanes with their own advisors/workers and tasks.
No decorative office simulation, forced animation or image assets.

## Architecture

Panes dispatch -> Tasks record -> tab.tasks through Store.mutate
Lead/PM cf task -> scoped Tasks interface (own tab only)
Human task commands -> Tasks combined session projection -> Board / Graph
Page native activity + durable inbox results -> independent operational badges

Tasks owns validation, revision control, historical projection, pagination and
link checks. The UI never replaces the whole ledger. Transport follows existing
Node bridge + Rust command patterns; coordinator routes keep scoped authority.

## Acceptance criteria

- [x] One session overview includes only its Lead and linked PM, with advisors/workers.
- [x] Board and Graph show consistent tasks, owners, statuses and relationships.
- [x] Automatic assignments retain initial text and followups; historical text is honest.
- [x] Explicit tasks, progress, notes and human Q&A persist across reopen/restart.
- [x] Stale edits, bad links and cycles are refused without overwriting newer state.
- [x] Lead/PM can manage only their own tasks; workers cannot use coordinator task commands.
- [x] Every conversation reply is visible independently; viewing/editing creates no receipt.
- [x] Activity, receipt and declared acceptance remain independent; unknown stays unknown.
- [x] Filters, graph navigation, keyboard controls, detail and pagination work.
- [x] Switching Tasks/Graph/PM/Lead preserves terminal instances and drafts.
- [x] Applicable Node, browser, Rust/bridge and candidate smoke checks pass.
- [x] Private candidate matches source; installed app and live profile remain unchanged.

## Testing architecture

Use Node's test runner against the real Store/Tabs with isolated HOME beneath
~/.consensflow/tmp; test pure graph/state behavior at the public Tasks interface.
HTTP/bridge tests use existing local server + fake native transport boundaries.
Playwright tests exercise app UI with the existing fake Tauri boundary, plus
packaged smoke through the private candidate. No external requests/model trials
in tests. Cover every task state, ownership/refusal, stale revision and navigation
path; no coverage percentage invented without instrumentation.

## Phase 1: Task state and projection [completed]

- [x] [TEST-TASK-01] tests/tasks.test.mjs: persistent scoped tasks, links/cycles, revisions, questions, historical assignment projection and bounded pagination.
- [x] [IMPL-TASK-02] src/tasks.js: small task interface on existing Store queue; satisfies TEST-TASK-01.
- [x] [TEST-TASK-03] tests/ui-panes.test.mjs: coordinator/human task routes and dispatch capture, multiple replies and permission isolation.
- [x] [IMPL-TASK-04] src/store.js, src/panes.js, bin/cf.mjs, src/ui.js, src/launch.js: capture within sent-record transaction and expose scoped operations; satisfies TEST-TASK-03.

## Phase 2: Coordinator workflow [completed]

- [x] [TEST-TASK-05] tests/cli.test.mjs and tests/skill.test.mjs: task commands and concise role instructions across supported coordinator harnesses.
- [x] [IMPL-TASK-06] bin/cf.mjs and src/skill.js: list/get/add/update workflow and truthful task/review policy; satisfies TEST-TASK-05.

## Phase 3: Session board and graph [completed]

- [x] [TEST-TASK-07] app/tests/page.spec.mjs: combined session board, filters, edits/Q&A, graph edges/navigation, pagination and terminal preservation.
- [x] [IMPL-TASK-08] app/ui/tasks.js, app/ui/panes.js, app/ui/index.html: interactive board/graph using shared task projection; satisfies TEST-TASK-07.
- [x] [TEST-TASK-09] app/src-tauri/src/commands.rs and tests/integration: task command bridge and malformed payload refusals.
- [x] [IMPL-TASK-10] app/src-tauri/src/commands.rs and lib.rs: wire bounded native page task operations; satisfies TEST-TASK-09.

## Phase 4: Acceptance [completed]

- [x] [VERIFY-TASK-11] Review implementation, run applicable suites, inspect browser screenshots and fix defects.
- [x] [VERIFY-TASK-12] Rebuild/sign separate candidate, validate source/bundle and smoke; preserve installed canaries.

## Resume context

> Complete: 12/12. Final checks: Node1120 pass/6 gated or platform skips; browser136; Rust98 unit+16 headless; current-source bridge integration19; packaged smoke2; lint and Clippy pass. Both review rechecks clear. Candidate signed, CLI/roles63 files exact, installed bundle/live roster hashes unchanged. Live model behavior remains user acceptance, not an automated claim.
> Prior devin-and-receiver-cleanup paused at 38/39; only intermittent installed
> paste restart remains unreproduced. All prior candidate functionality retained.

## Research

[Source comparison and implementation notes](/Users/gabrielvoicu/.consensflow/research/munder-difflin-20260913/session-board-research.md).
The research subagent verified Munder's graph is primarily agents/messages,
not task scheduling. Reuse our current store and receipt evidence; avoid their
stale whole-array question updates. Advisor and Context7 tools are unavailable.

## Decision log

| Date | Decision | Reason |
|---|---|---|
| 2026-09-13 | Native SVG/HTML, existing store, no new dependency | Live interactive graph and smallest coherent implementation. |
| 2026-09-13 | Separate task, activity, receipt dimensions | Idle and received are not acceptance. |
| 2026-09-13 | Optional preference defaults to automatic capture + manual corrections | User requested action; no permission barrier to reversible implementation. |

## TDD log

| Task | Red | Green | Refactor |
|---|---|---|---|
| TEST-TASK-01 | node --test tests/tasks.test.mjs: one failed file, missing src/tasks.js; eight cases specified. | — | — |
| IMPL-TASK-02 | Two historical-pane fixture setup errors corrected to use the real allocator; behavior expectations unchanged. | 8/8 Node cases pass. | Biome formatting; 8/8 rerun. |
| TEST-TASK-03 | node --test --test-name-pattern='task board' tests/ui-panes.test.mjs: 3 failed; missing scoped operations/bridge handlers. | — | — |

| IMPL-TASK-04 | Three missing operations reproduced. | 3/3 focused pass. | 239/239 task/store/launch/UI tests pass after formatting. |

| TEST-TASK-05 / IMPL-TASK-06 | CLI task verb and role commands absent: 2 focused failures. | CLI routes pass; role assertion corrected for newline whitespace. | CLI/role suite green after formatting. |

| TEST-TASK-07 / IMPL-TASK-08 | 5 browser tests fail: missing Tasks view. | 3/5 first run; fixed accessible field label and detail heading. 5/5 pass. | Pending formatting/full browser suite. |

| TEST-TASK-09 / IMPL-TASK-10 | Live bridge rejects unknown task_list command. | 17 command tests pass; malformed changes refused before bridge. | Rustfmt applied; full native suite pending. |
| Review regressions | Real Q&A mismatch; four domain failures; graph focus loss (after correcting fixture event name). | 12 task-domain + 7 browser pass, including real-store Q&A. | Full suites pending. |

| VERIFY-TASK-11 | Initial full-suite stale help/label expectations corrected; 1 transient current-bridge read failure retained. | Node1120/0/6, browser136/0, Rust114/0, integration19/0. | Lint/Clippy pass; graph/board screenshots inspected; both reviewer rechecks clear. |
| VERIFY-TASK-12 | Old packaged smoke had no task-board event. | Sealed candidate smoke2/2, including real WebKit board/graph and Q&A. | CLI/roles63 files match; signed; installed/live-roster canaries unchanged. |

## Acceptance evidence

Candidate: /Users/gabrielvoicu/.consensflow/candidates/session-board-20260913/
Full evidence, source hashes and previews: verification.json, source-hashes.json,
task-board.png and task-graph.png in that folder. Logs retain every RED and initial
failed run under ~/.consensflow/tmp/session-board. The initial integration run
used the pre-existing default bridge; final19/19 used the freshly built source
bridge explicitly. A transient app-unreachable result read on the first current
bridge run passed in isolation and the complete rerun; its cause is unproven.

## Standards review

Three original findings (answer contract, assignment identity, escaped aggregate
size) fixed and rechecked. No remaining material finding;13 domain tests passed.

## Spec review

Four original findings (answer contract shared with Standards, historical followup
provenance, referenced history after deletion, graph focus/scroll refresh) fixed
and rechecked. No remaining issue within the focused review.

## Deviations

| Task | Spec said | Actual | Reason |
|---|---|---|---|

| VERIFY-TASK-11 | Keep existing formatting | Formatted one assertion in the previously changed Claude-core test | Required lint failure; no test semantics changed. |
