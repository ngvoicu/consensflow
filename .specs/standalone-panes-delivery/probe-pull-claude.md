# Claude Code receiver integration experiment

Date: 2026-09-12. Experimental evidence, not production implementation or installed-app acceptance.

## Verdict

Stock Claude Code 2.1.269 can receive complete inbox results through a private native hook integration across idle/busy turns, `/clear`, native resume, and an actual background continuation with the original process still alive. This establishes feasibility of the receiver integration, not production reliability or model comprehension.

The continuation test used the native left-arrow background action. Claude itself wrote `continued-in`, launched a successor process, and invoked the inherited SessionStart hook with `source: "fork"`. The successor received the next full result; the predecessor did not. No transcript or native session registration was fabricated to induce this transition.

## Isolation and mechanism

Runnable probes and evidence live under `/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/claude`. Each launches the installed Claude binary in a separate PTY with a private HOME, CLAUDE_CONFIG_DIR, working directory, settings file, temporary directory and dummy API credential. Its Anthropic endpoint is a localhost SSE mock. Tools, external MCP configuration, prompt suggestions and nonessential traffic are disabled. No remote model calls, production configuration changes, installed ConsensFlow changes, real-session commands or live delivery-ledger writes were made.

ConsensFlow-owned experiment files are below its private home. Claude itself creates native ephemeral `/tmp/cc-socks/<pid>.sock` sockets despite a private config/temp directory. The recorded test processes are stopped and their identified sockets were verified absent; no other harness sockets were touched. Background cleanup used `claude daemon stop --any` with this experiment's isolated CLAUDE_CONFIG_DIR, whose supervisor is a separate instance. See `cleanup-verification.json` and each background run's `daemon-cleanup.txt`.

The prototype uses one local inbox with immutable result IDs/bodies and a current native receiver. SessionStart registers the native `session_id`. SessionStart and Stop launch an `asyncRewake` command that waits for an unclaimed result, obtains its full body, and exits 2 with an attributed envelope. Native Claude queues that output and wakes itself. An old hook retires when its session no longer matches the current receiver. A file lock serializes claims across overlapping hooks.

`fetched` deliberately does **not** mean acknowledged. The independent test observer verifies complete bodies in both native user-message records and actual model requests. The toy hook does not implement a production receipt service, authorization, durable leases or crash reconciliation.

## Executed cases

Main evidence: [run-w_guylgz result](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/claude/run-w_guylgz/result.json), [native body verification](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/claude/run-w_guylgz/verified.json), [provider requests](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/claude/run-w_guylgz/requests.json).

| Case | Evidence |
| --- | --- |
| R1: first answer while idle | Full body wakes the current session and enters its native history/model request. |
| R2: second answer from the same worker | Independent ID and complete 12,010-character body received. |
| R3: third answer during an active model response | Result arrives during a deliberately delayed main response; its next model request starts after that response ends. |
| R4: native `/clear` | Native SessionStart selects a new UUID; result appears only in the new conversation. |
| R5: native `/resume <original UUID>` | Result appears in the resumed original conversation. |
| R7: result while user has an unsent draft | Result reaches context; the draft is not submitted by the receiver and remains available for the user's later Enter. |

All six results have exactly one matching native user-message entry in the expected conversation and their complete body in provider input. The mock returns a deterministic acknowledgment; it does not choose whether to fetch a result. Eleven local requests were recorded, including native helper requests.

Continuation evidence: [run-5w4vdk54 result](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/claude/run-5w4vdk54/result.json), [native body verification](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/claude/run-5w4vdk54/verified.json), [hook events](/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/claude/run-5w4vdk54/hook-events.jsonl).

After receiving R1, a two-second empty-idle interval caused no model request. Left-arrow backgrounding produced predecessor `0f43ced8-511d-418b-8386-bacaa6a02851` → successor `2ecb0552-f203-4415-a97c-3e0cca0007dc`. The inherited successor hook registered, fetched R6, and supplied its complete body to a successor model request. R6 exists only in the successor's native user-message history. The predecessor process was still alive at verification. Its earlier R1 is copied into the continuation under the same native message UUID; that inherited history is not a duplicate submission. Seven local requests, zero remote model calls.

## Counterexamples and limits

- Initial resume probe `run-5x2mirfx` failed: SessionStart hooks execute in parallel, so the async waiter could check before synchronous registration and retire. Waiting briefly for same-event registration corrected this prototype. Production requires explicit receiver registration/epoch handling, not this timing workaround.
- Initial background runner `run-vx864qcy` treated the original process exiting after `/bg` as failure and stopped before verifying the successor. Native hook inheritance was visible, but that run is not credited as a delivery pass. The successful left-arrow run additionally covers a surviving predecessor.
- An earlier main runner's busy delay could match a native helper request. The final `run-w_guylgz` delays the main conversation request containing R2 and asserts the response completion timestamp precedes the R3 request. Only this final evidence supports busy sequencing.
- Hook output is persisted as a native task notification/system reminder, with `Stop hook feedback` and error-oriented hook wording. The TUI can collapse the body. A native receipt does not establish that the human saw the full result. A ConsensFlow results view is still required.
- The waiter has a 35-second prototype lifetime. Long-idle renewal without model polling, hook deduplication, reloads and process restarts are not implemented or accepted. The short empty-idle observation is not an all-day idle guarantee.
- A native SessionStart identity alone is not authority to take over a ConsensFlow owner. Production must distinguish explicit owned continuation from independent forks/subagents, bind a receiver lease to launch/owner/revision, and retire old claims. This prototype uses one trusted isolated launch and does not solve that authorization problem.
- No crash-after-insertion-before-ACK, concurrent receiver, claim/switch race, long backlog, multipart pagination, compaction, or cross-owner authorization test was performed here. Plain reads must never remove visibility of unconfirmed answers.

## Reproduce

```sh
python3 /Users/gabrielvoicu/.consensflow/tmp/receiver-pull/claude/probe.py
python3 /Users/gabrielvoicu/.consensflow/tmp/receiver-pull/claude/probe-background.py
```

Each creates a new private run directory and retains `result.json`, `requests.json`, `hook-events.jsonl`, `screen.bin` and native transcripts. Source: `hook.py`, `probe.py`, `probe-background.py`. These are experimental fixtures and must not be copied into production as a finished receiver service.

## Primary references

- [Claude hooks reference](https://code.claude.com/docs/en/hooks): native hook identity, parallel hook execution, asyncRewake, and output visibility.
- [Claude agent view](https://code.claude.com/docs/en/agent-view): background continuation, configuration inheritance and isolated supervisor state through CLAUDE_CONFIG_DIR. The tests above establish the installed version's behavior; documentation alone did not establish acceptance.
