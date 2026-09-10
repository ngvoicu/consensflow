# Alpha.41 live delivery acceptance — 2026-09-10

Installed `/Applications/ConsensFlow.app` and bundled `cf`: 3.0.0-alpha.41.
All 57 installed files compared byte-for-byte to the tested signed app bundle.
User histories and sessions preserved; test app state isolated from production.

## Live native receipt matrix

| Lead | Claude worker | Codex worker | OpenCode worker | Pi worker | Kimi worker |
|---|---|---|---|---|---|
| Claude Code | PASS | PASS | PASS | Quota blocked | Balance blocked |
| Codex | PASS | PASS | PASS | Quota blocked | Balance blocked |
| OpenCode | PASS | PASS | PASS | Quota blocked | Balance blocked |
| Pi | PASS | PASS | PASS | Quota blocked | Balance blocked |

PASS means the complete unique worker token exists as an inbound user message
in the real native lead history and the daemon recorded an accepted receipt.
This was checked directly for all 12 successful combinations, not inferred from
the UI. The real app CLI, real Rust PTY bridge, real harnesses, and xterm terminal
responses were used. The headless test bridge was compiled from the installed
app's source; the packaged app also passed its separate smoke test.

Pi's configured lead model returned invalid OAuth: inbound delivery is proven,
but its subsequent AI response is not. The configured Pi worker returned weekly
usage exhaustion; Kimi returned insufficient balance. Those workers produced no
completed answer to deliver. No credentials, provider settings or billing were
changed. Eight matrix cells therefore remain provider-blocked, not passed.

## Regressions and fixes

- Claude /clear: actual native conversation changed; OpenCode worker reply was
  accepted in the new conversation of the same pane. No manual result read.
- Codex initial matrix attempt failed binding: /var and /private/var spellings
  differed. Exact originator metadata existed. A failing alias regression was
  added, then path comparison canonicalized without relaxing launch identity.
  Canonical-path matrix rerun passed all three working worker harnesses.
  Final alpha.41 live alias-path test also passed (Codex receiving OpenCode).
- Pending results follow a verified same-pane conversation change; uncertain
  submissions retain their original receipt target and never blindly replay.
  Existing failed alpha.39 records were not silently resent.

## Automated checks and evidence

- Delivery / Claude peer: 87 passed.
- Session binding (including path alias): 26 passed.
- Rust unit / process checks: 89 passed.
- Packaged smoke: passed for alpha.40 and alpha.41.
- Native complete-token receipts: `native-receipts.json` in release directory.
- Local raw test evidence: `/tmp/cf40-native`, `/tmp/cf40-codex-retry`,
  `/tmp/cf41-alias-native`. Test processes closed by their harness cleanup.

Other harness conversation-reset variants are not claimed verified by the
Claude /clear test. No GitHub publication or application-wide reset performed.
