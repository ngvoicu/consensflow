# Alpha.37 acceptance — 2026-09-09

This pass implements Phase 19 and closes the earlier delivery/launch/restart
findings. No new ConsensFlow agent was dispatched for implementation. Final
review was performed locally against the requirements, code and actual runtime
evidence; earlier reviewer findings remain recorded in SPEC.md.

## Product checks

- Lead is fixed at twice worker width; two worker rows scroll horizontally.
- PM is a distinct role/session and maximized native window, outside the grid.
  Its own PTY renders and accepts input through scoped native commands. Parent
  deletion and stale-window generations cannot target other sessions.
- Lead/PM role documents are bundled and loaded privately; workers receive
  neither. Setup/roster/update do not write native global skill directories.
  The five old owned generic skills were removed manually, not by app migration.
- Each harness has installed/version/release diagnostics and integration status
  based on an accepted receipt matching its active session/generation. No native
  version allowlist blocks launching or reading. Pi is prepared only if present.
- Automatic full-result delivery produced native receipt evidence for Claude
  Code, Codex, Pi and OpenCode. Codex and OpenCode also visibly answered the
  delivered result. Pi kept an unsent draft unchanged, held delivery, and received
  the result automatically after the test cleared that draft.
- Restart retained the worker in navigation and its complete answer. Legacy
  worker rows already discarded by alpha.34 are reconstructed only for their
  original surviving tab. All seven Fortuna rows were recovered in the installed
  app, closed, with conversation files unchanged in the migration clone. Explicit
  Claude resume kept the same native session and did not redispatch the worker.
- The actual signed updater refused while panes were open, then replaced an
  isolated fixture app and restarted into alpha.37 after cleanup.

## Validation

| Check | Result |
|---|---|
| Node suites | 1198 passed; 3 optional absent-sibling parity skips; 2 packaged gates run separately |
| Rust unit / real-process tests | 89 / 16 passed |
| Browser UI | 85 passed |
| Real Node + Rust PTY integration | 25 passed |
| CLI final private-role/help regression | 37 passed |
| Packaged lead + separate PM terminal | passed |
| Packaged signed updater replacement/restart | passed |
| Biome / Clippy | passed; Clippy warnings denied |

A heavily concurrent Node attempt hit the one-second Pi editor-probe deadline;
the unchanged focused suite (20 tests) and full bounded-concurrency run passed.
One focused Rust attempt hung while macOS reaped a killed stress-test PTY; the
subsequent complete suite passed. These attempts are retained in the local logs.

## External limits

The default OpenCode Go provider reported its weekly usage quota. Pi's alternate
OpenAI login reported an invalidated OAuth token. ConsensFlow delivered and
recorded the worker result despite the model provider's response error. These
account limits require the native provider's normal quota/login resolution;
ConsensFlow does not modify credentials. OpenCode's free model completed the
acceptance exchange.

The build is ad-hoc macOS code-signed and has a separate signed updater archive;
it is not Apple-notarized. DMG, archive, signature and update metadata are prepared
locally. No GitHub release or rolling channel feed is published in this task.

## Evidence

Local detailed logs: `/tmp/cf-alpha37-node-legacy.log`,
`/tmp/cf-alpha37-rust-final.log`, `/tmp/cf-alpha37-ui-release.log`,
`/tmp/cf-alpha37-integration2.log`, `/tmp/cf-pm-packaged-green2.log`,
`/tmp/cf-alpha37-updater-smoke.log`, `/tmp/cf-alpha37-cli-final.log`.
Sanitized release evidence is copied with the release artifacts; credentials,
private transcripts and signing keys are excluded.

Final legacy recovery regression: RED missing navigation, then GREEN; full Node
1198/1203 (five documented skips), UI 85/85, packaged lead/PM smoke passed after
the final rebuild. Installed bundle matches all 55 built files. Evidence:
`/tmp/cf-alpha37-legacy-recovery.json`, `/tmp/cf-alpha37-installed-recovery.json`,
`/tmp/cf-alpha37-legacy-smoke.log`. No worker task was restarted by migration.
