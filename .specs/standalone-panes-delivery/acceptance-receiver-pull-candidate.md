# Receiver inbox and PM advisors — separate candidate acceptance

2026-09-12. Implements the user's shared receiver approach for Claude Code, Codex,
Pi and OpenCode, completes PM advisors and the separate grids, and removes the
superseded automatic sender. The installed application is deliberately unchanged.

## Delivered behavior

- Every completed worker/advisor answer has an immutable identity, full body and
  independently tracked parts. A prior received answer never suppresses later ones.
- Native receiver integrations register the current conversation, fetch a bounded
  part, persist insertion intent, recheck native selection and insert through the
  harness's supported interface. Only exact full native-context evidence confirms
  receipt. Transport success and a person opening Results are not receipts.
- New/resume/clear retire old receiver authority. A lost registration response can
  recover the identical binding; a predecessor cannot take over a newer selection.
  Ambiguous writes remain uncertain and are not automatically replayed.
- The app shows complete result history, per-answer state and unconfirmed counts
  on coordinator/worker cards and sidebar entries, including closed worker panes.
  Manual collection uses the same inbox; explicit complete CLI reads remain.
- PM advisors have private startup/resume instructions across all four harnesses,
  their own ownership and capabilities, result collection and restored native
  histories. PM and Lead have separate grids, focus/navigation and hidden terminal
  continuity. PM suspension/deletion does not close its parent or another session.
- Advisors research, review, search and run existing checks, then return findings
  to their PM. Only the PM authors/revises specifications. These are role
  instructions and application communication boundaries, not an OS sandbox claim.
- ConsensFlow runtime state and managed integrations stay under its configured
  private home. Legacy configuration is imported without modifying its source;
  links that could redirect later writes outside the home are not imported.

## Removed and retained

Removed the app-side automatic dispatcher, stale destination refresh, automatic
PTY result pasting, delivery resend/state transitions, obsolete readiness module,
old `deliver.now`/held-delivery UI/API, old store delivery mutators and unused
adapter exports/styles. Superseded sender-state tests were replaced by inbox,
receiver, migration, visibility and native-adapter tests; their removal explains
why the overall test count is lower than the previous installed release's count.

Small native integrations remain necessary: Pi extension, OpenCode TUI integration,
Codex broker and private Claude hooks. They now collect through the shared inbox.
Native identity and explicit user-requested task/PM-to-lead communication remain;
these are not the retired automatic sender. No global harness settings were edited.

Historical delivery records are retained read-only. Accepted evidence is preserved.
An uncertain attempt remains in its original record even if a later independently
verified native receipt establishes receipt in the new inbox. Explicit Claude
`continued-in` ancestry can verify manual reads in a successor; it never rewrites
an old automatic attempt's destination or claims its unknown socket fate proved.
The former pending-routing patch in Phase 34 is superseded by receiver registration.

## Final automated gates

All commands exited 0. Logs are under
`/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/`.

| Gate | Final result | Evidence |
|---|---:|---|
| Full `npm test` | 1,077 passed, 6 gated skips, 0 failures | `node-release-gate.log` |
| Full browser suite | 122 passed | `browser-candidate.log` |
| Real CLI + Rust bridge/PTY integration | 19 passed | `integration-storage-final.log` |
| Rust library + native integration tests | 93 + 16 passed | `rust-all-targets.log` |
| Clippy, all targets, release, warnings denied | Passed | `clippy-final.log` |
| Biome | Passed, pre-existing informational style suggestions remain | `lint-storage-final.log` |
| Packaged native app/terminal/PM and agent catalog smoke | 2 passed | `smoke-storage-final.log` |
| Exact bundled source comparison | 60 files matched; removed readiness absent | candidate `verification.json` |
| Local application signature | `codesign --verify --deep --strict` passed | candidate bundle |

The Rust hang was reproduced and sampled. A test kept an unread PTY master open,
which blocked macOS child exit after SIGKILL. Draining it as production does fixed
that test. Release-mode updater fixtures now explicitly permit their localhost
HTTP fixture; production transport policy is unchanged. A packaged path assertion
was corrected to decode file URLs when the candidate name contains spaces.

One full Node run under simultaneous release compilation exposed two helper
startup timeouts; the final full run above passed without competing compilation.
No production timeout was relaxed. Browser failures during the rename caught
and fixed a stale selector that retained duplicate result badges.

## Stock native receiver checks

The actual candidate integrations, production inbox/Store/scanner and stock TUIs
were exercised in private homes with local mock providers. Completed results were
seeded through production `indexResult`; worker transcript ingestion is covered
separately by source/parser and real bridge tests. No remote model calls occurred.

| Native harness | Independent replies | Complete parts | Cases |
|---|---:|---:|---|
| Claude Code 2.1.269 | 6 | 9 | idle, multipart, native clear, resume, consecutive queued replies, empty signal |
| Codex 0.154.0 | 6 | 9 | idle, successive replies, native new/resume, busy hold, multipart |
| Pi 0.85.1 | 6 | 8 | idle, successive replies, native new/resume, busy hold, multipart |
| OpenCode 1.18.30 | 6 | 8 | idle, successive replies, native new/resume, home-route hold, busy hold, multipart |

Every part had one native stored receipt in its intended session and complete text
in a local provider request. Another owner's result stayed unclaimed in every run.
This proves context construction and receipt plumbing, not real-model reasoning.
App Results supplies persistent visible bodies even where a native TUI hides tool
or hook context. Native cancellation/switch-race feasibility and negative cases
are recorded in the linked research reports and source regressions.

A separate crash-window replay preserved durable insertion intent but removed
receipt bookkeeping, reopened the Store and reconciled against those real native
histories: all 24 results / 34 parts recovered without native insertion or replay.
Evidence: `candidate-recovery.json` and `candidate-recovery.log`.

Successful raw runs:

- `filewake-resume-e15i5gvj` — Claude, `result.json`, `final-inbox.json`, `requests.json`.
- `r194923` — Codex, `result.json`, `final-inbox.json`, `provider-input.json`.
- `pi-run-20260912-194612` — Pi, `report.json`, `final-inbox.json`, native sessions and provider log.
- `opencode-run-20260912-194924` — OpenCode, `report.json`, `final-inbox.json`, native messages and provider log.

Reproduction entrypoints in the same private evidence directory:
`candidate-service.mjs`, `candidate-claude.py`, `candidate-codex.py`,
`candidate-codex-broker.mjs`, `candidate-pi.py`, `candidate-pi-provider.mjs`,
`candidate-opencode.py`, `candidate-recovery.mjs`. These are isolated acceptance
fixtures, not installed runtime modules. Unit regressions remain in the repository.

## Candidate handoff and limits

Stable candidate directory:
`/Users/gabrielvoicu/.consensflow/candidates/receiver-pull-20260912/`.

- `ConsensFlow Candidate.app`: separate identifier, local ad-hoc signature,
  version alpha.61 with the candidate source; not a published/notarized release.
- `Launch candidate.command`: starts that bundle with the adjacent private profile;
  inherited ConsensFlow pane capabilities are cleared. Syntax checked with `zsh -n`.
- `profile/agents.json`: one copy of existing saved agent definitions. No production
  sessions, threads or delivery ledgers were copied or modified.
- `verification.json`: bundled source hashes and artifact/profile paths.

The installed `/Applications/ConsensFlow.app` still reports 3.0.0-alpha.61. It was
not restarted, reinstalled, replaced or modified. No commit, GitHub release, NAS
push or real-account model trials were performed. User live acceptance remains
separate from these automated/local-provider gates. The old repository debug
folder remains absent; Cargo artifacts are under `~/.consensflow/build/`.
