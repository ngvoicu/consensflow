# Codex receiver-pull experiment — 2026-09-12

## Decision evidence

**Plain shell fetch/wait is not a sufficient automatic-delivery replacement. A receiver adapter can deterministically fetch complete answers and provide them to native Codex without asking the model to run a fetch tool.** Both that variant and native wake followed by model fetch passed the bounded context tests. The deterministic variant did not display the supplied bodies in the stock TUI. This is a plumbing experiment, not approval to migrate production.

All native tests used stock `/opt/homebrew/bin/codex` **0.154.0**, isolated homes/workspaces under `~/.consensflow/tmp/receiver-pull/codex`, and a localhost Responses API mock. No remote model calls were made. The mock deliberately emits the tool calls being tested; it does not prove that a real model reliably chooses them. No installed ConsensFlow files, running application, user sessions, real projects, or global harness settings were changed. The C experiment imported the installed alpha.61 private broker as a library in a separate process.

## B: ordinary receiver shell fetch/wait

The prototype puts successive immutable bodies under `worker-one:a1`, `worker-one:a2`, etc. A normal Codex `exec_command` runs the prototype inbox helper. The helper obtains `CODEX_THREAD_ID` from its actual native tool environment and returns the full body in the ordinary tool result. Long polls that exceed the first tool yield require the model to call `write_stdin` to receive their eventual output.

Evidence: [B observations](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/r175547/observations.json), [independent exact-body verification](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/r175547/verified.json), [provider requests](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/r175547/provider-input.json), [native terminal output](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/r175547/screen-resume.bin).

| Scenario | Observed outcome |
| --- | --- |
| First completed answer | a1 fetched in native session A; exact full body persisted as `function_call_output` and appeared in the next mock-provider input. |
| Second answer during a waiting tool | a2 arrived after the initial one-second tool yield. Native `write_stdin` returned it; exact full body reached session A and provider input. |
| Third answer while idle | a3 remained pending for the three-second observation; provider request count stayed exactly 7. An inbox file alone did not wake Codex. |
| Native `/new`, then fetch | a3 was fetched by new session B. Exact full body appeared in B, not A. |
| Native CLI resume of A | a4 was fetched by the original native session A and entered its history/context. |
| `/new` while the lead waits | The TUI rejected the command: `'/new' is disabled while a task is in progress.` a5 subsequently reached the still-current A. |
| Escape interrupt, `/new`, then a6 arrives | The background shell remained alive and fetched a6 using A's environment. **Neither A nor the new session C ever received a6 as a native tool result or provider input.** C's subsequent fetch returned an empty list because the deliberately naive helper treated fetched as consumed. |

Session identities in this run:

- A: `01a0961e-100f-7c32-81e9-0ec8c0b297c3`
- B: `01a0961e-34cc-7922-9fc8-ef540c3240b8`
- C after interruption: `01a0961e-8703-74b1-a5c1-962cad127e28`

The interruption is a concrete counterexample to marking an answer delivered when a reader process obtains bytes. The helper's `fetched` entry for a6 exists, but its independently verified native-receipt list and provider-input list are both empty. Any real inbox must keep that answer outstanding unless receipt is confirmed; fetch leases/cancellation and recoverable attempts are still necessary. This failure was induced in the isolated prototype, not the running application.

## C: native wake followed by receiver fetch

The official App Server API documents `turn/start.toolOutput` with empty `input` for client-provided tool output. It starts generation while idle and queues output into an active regular turn. The output remains a tool-output item. Native thread resume takes the recorded thread ID. [Official App Server documentation](https://learn.chatgpt.com/docs/app-server#start-a-turn).

The prototype launches a separate stock app-server and stock TUI through the existing ConsensFlow private broker. The broker observes the TUI-selected native session. A separate control client submits **only** `INBOX_WAKE: fetch pending results.` via `turn/start.toolOutput`; the mock model then requests the same ordinary shell fetch used in B. Worker bodies are read by the receiver's native tool, not carried in the wake notification.

| Scenario | Exact full body in native tool output | Exact full body in provider input | Native TUI emitted body markers |
| --- | --- | --- | --- |
| Idle first answer a1 | Yes, A | Yes | Yes |
| Second answer a2 | Yes, A | Yes | Yes |
| `/new` into an empty thread, a3 | Yes, B only | Yes | Yes |
| Native `/resume A`, a4 | Yes, A | Yes | Yes |
| Wake queued during a busy model request, a5 | Yes, A | Yes | Yes |

Final C run: **5/5 exact-body cases**, 14 localhost provider requests, zero remote model calls. [C observations](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/r180007/observations.json), [exact-body verification](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/r180007/verified.json), [native RPC responses](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/r180007/events.json), [provider requests](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/r180007/provider-input.json), [native TUI bytes](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/r180007/screen-native-wake.bin).

Session A: `01a09622-0be6-7a13-82cd-b8705721df8b`; session B: `01a09622-1b9b-7822-8e01-3aa6b6bb42ba`. Broker selection revisions advanced 1 → 2 on `/new`, then 3 on resume. The C harness retained and parsed native responses rather than treating transport success as a receipt.

## C refinement: deterministic receiver fetch and full native tool output

The final variant moves fetching into the separate receiver integration process. It reads the existing private broker's selected session and revision, loads the pending answer batch from the same prototype inbox, preserves each `resultId` and exact body, records a fetched attempt, and supplies the full attributed batch through `turn/start.toolOutput`. The model receives the bodies directly and only emits a mock acknowledgment. **No model-generated fetch or shell-tool calls are involved.**

| Scenario | Exact full body in native tool output | Exact full body in provider input | Native TUI emitted body markers |
| --- | --- | --- | --- |
| Idle first answer a1 | Yes, A | Yes | No |
| Second answer a2 | Yes, A | Yes | No |
| `/new` into an empty thread, a3 | Yes, B only | Yes | No |
| Native `/resume A`, a4 | Yes, A | Yes | No |
| Complete body queued during busy model request, a5 | Yes, A | Yes | No |

Deterministic C run: **5/5 exact-body context cases**, **zero model fetch/tool calls**, 9 localhost provider requests, zero remote model calls. Native TUI rendered the assistant's acknowledgment messages, but **0/5 supplied result bodies** appeared in the captured terminal output, including the resumed history. Context receipt and user-visible result presentation are therefore separate requirements.

Evidence: [deterministic C observations](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/r180351/observations.json), [independent exact-body verification](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/r180351/verified.json), [native responses](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/r180351/events.json), [provider requests](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/r180351/provider-input.json), [native TUI output](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/r180351/screen-native-wake.bin).

Session A: `01a09625-74eb-73b1-bedf-fe99e34c1b52`; session B: `01a09625-8417-7841-9a1d-6ab3448a28e1`. The mock remains useful for plumbing only; it supplies no evidence about a real model's interpretation or quality. Removing model tool-choice dependence does establish a more deterministic context-delivery step.

This supports a **targeted inbox and ownership refactor**, reusing the existing Codex broker and native transport. It does not support a wholesale delivery rewrite or the claim that native routing disappears: the receiver integration still must admit its result into the correct native conversation. Durable result ownership, fetched-versus-received accounting, admission fencing and a visible result history are the changes being evaluated.

### What this establishes and what remains open

- C avoids keeping a shell long poll active solely to wake an idle lead. It uses native API operations already supported by this installed Codex version.
- A single common inbox/result-status contract is compatible with Codex; the wake transport is harness-specific. Codex can reuse its existing broker for selection ownership. This does not eliminate the need for that broker or imply that every harness has Codex's native interface.
- The experiment reads broker selection, then calls a separate native control connection. That comparison and submission are **not atomic**. Production must perform its final generation/session/revision comparison and native submission inside the broker's owned admission boundary, as the current `/deliver` path does. No switch-race guarantee is claimed for this prototype.
- C still needs durable per-result attempts, receiver leases, independent receipts, exact-part coverage for large outputs, and pending badges. The toy helper intentionally lacks locking, transactions, retention, pagination and secure ownership capabilities; it must not be copied into production.
- Native `CODEX_THREAD_ID` proves which thread executed the fetch. It does not alone authorize ownership of a ConsensFlow parent tab, prove that the thread remains selected, or cancel an old background reader.
- Provider-input verification proves that the complete answer entered model context. It does not prove understanding or incorporation into a real model's final answer.
- In B and pointer-then-fetch C, short answer bodies were emitted by the native TUI as command-result rows. They were not separate assistant messages. In deterministic full-body C, the supplied bodies were absent from TUI output even though native history and model context contained them. Terminal bytes were captured; no real ConsensFlow UI screenshot or human-visible expanded/scroll position was verified. Long/partially collapsed result rendering was not tested. A ConsensFlow results view should therefore expose bodies and per-answer statuses independently of the harness's terminal rendering.
- B and C tests used the same result IDs and exact-body verifier. A per-worker Boolean would lose a2/a3 and is insufficient; the unconfirmed count must include every completed answer lacking its own receipt, including fetched a6.
- B's live native session switch and C's wake feasibility do not prove the reported Claude `continued-in` case fixed. Nothing in that incident's stored state was modified or reclassified.

## Reproduce without touching production

All scripts create fresh run directories under the same private experiment root. Run from any directory:

```sh
/usr/bin/python3 /Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/probe.py
/usr/bin/python3 /Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/native-wake.py
/usr/bin/python3 /Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/native-wake.py --full
```

Each prints its run root. Verify a chosen run:

```sh
python3 /Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/verify.py /Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/r180007
```

Prototype entrypoints: [B probe](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/probe.py), [C probe](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/native-wake.py), [private control client](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/native-control.mjs), [independent verifier](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/codex/verify.py). B's printed `PASS` means the scripted experiment completed, including its negative observations; it does not mean plain pull passed an automatic-delivery acceptance gate.
