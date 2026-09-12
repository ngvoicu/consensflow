# Claude receiver: file notification and synchronous body fetch

Date: 2026-09-12. Stock Claude Code 2.1.269. Isolated localhost-provider experiment, not production or installed-app acceptance.

## Decision

Use a private native **FileChanged asyncRewake notification containing no result body**, then fetch the result inside native **UserPromptSubmit** using that invocation's current session identity. The native notification does trigger UserPromptSubmit, so the complete body can enter the very same provider request without a model-generated tool call. This supersedes the earlier proposal to emit full bodies directly from long-running asyncRewake hooks.

Direct async body output has a reproduced wrong-native insertion race. A helper claimed a result under the old session, paused after validation, and emitted after native `/clear`. Its complete body was inserted into the successor's user history and provider request. A final helper-side check alone cannot atomically fence native queue admission.

The wake-only version passed that deliberately stretched race: the stale notification contained no result, and the current session's UserPromptSubmit claimed and returned the body. A blocked synchronous body fetch was also interrupted with Escape and followed by `/clear`, then in a separate run by `/resume`; native discarded the canceled body in both cases. These tests establish those boundaries, not all possible cancellation/crash interleavings.

## Mechanism and native receipts

1. ConsensFlow creates an owner-scoped signal file below its private home before launching the native session.
2. A short synchronous SessionStart callback registers the receiver and returns the signal file in `watchPaths`.
3. A FileChanged callback verifies that exact signal path and whether work is pending. Empty inbox: exit 0. Pending work: emit only a harmless notification and exit 2 using `asyncRewake`.
4. Native UserPromptSubmit receives a fresh `session_id` and `prompt_id`. Its synchronous callback validates receiver ownership/revision, claims immutable result content, records admission intent, and returns framed content via `hookSpecificOutput.additionalContext`.
5. Receipt verification observes actual native context records. Hook exit, file notification and claim completion never acknowledge receipt.

In successful runs, the exact body is stored as:

```json
{
  "type": "attachment",
  "sessionId": "<native session UUID>",
  "uuid": "<native attachment UUID>",
  "timestamp": "<native timestamp>",
  "attachment": {
    "type": "hook_additional_context",
    "content": ["<exact complete framed result or fragment>"],
    "hookName": "UserPromptSubmit",
    "hookEvent": "UserPromptSubmit",
    "toolUseID": "hook-<native UUID>"
  }
}
```

`rendered[].content` additionally contains a system reminder wrapper. Match the framed content in `attachment.content`, expected native session, owner/claim identity, and full hash/part coverage. The native pointer is a separate user/task-notification item; it is not a body receipt. Native attachments may be hidden from the human-facing terminal, so the app still needs complete visible results and per-answer pending badges.

## Executed evidence

All paths below share `/Users/gabrielvoicu/.consensflow/tmp/receiver-pull/claude/`. Each retained run includes settings, hook events, provider requests, result JSON, terminal output and native transcripts.

| Run | Result |
| --- | --- |
| `filewatch-0dst_f34` | Direct async body: idle first, second 12KB body, busy third, clear, resume, and unsent draft passed. Controlled claim/clear/exit race inserted the old claim body into the new native session. |
| `filewake-x7qmo2pu` | Wake-only small R1 passed; 12KB R2 failed full-body verification because native replaced additionalContext with a persisted-file pointer and preview. |
| `filewake-8_1o0qpp` | Wake-only first, second 6KB body, busy third, clear, resume, draft preservation, empty signal with zero model calls, stale-wake/current-fetch, and synchronous cancellation across Escape + clear passed. Seven complete results each have one expected native additional-context attachment; canceled BOUNDARY has none. The runner inherited a misleading “12k” observation label; `expected.json` and verification establish the actual 6KB test. |
| `filewake-resume-er2msy4j` | Repeated the wake-only matrix and proved synchronous output cancellation across Escape + native resume. Seven complete body receipts; canceled BOUNDARY absent from all provider requests and native attachments. |
| `filewake-batch-5waahfiu` | A 30,094-character canonical envelope split into four unique strings (8,000/8,000/8,000/6,094) reached one provider request through four synchronous handlers. Exactly four native additional-context attachments contain the exact fragments. No extra model turn. |

The successful clear/resume wake-only runs each made 12 localhost requests, including native helper requests; the four-fragment run made 3 total, with exactly one containing the fragments. Zero remote model calls were made. Mock output only confirms supplied context; it does not prove model comprehension.

`verified.json` files record native session/UUID/type and exact content checks. The first batch attempt `filewake-batch-co84xeb7` used repetitive interior fragments and is superseded by the unique-fragment run. `filewatch-cleanup-verification.json` confirms all six probe parent processes are stopped and their exact native sockets are absent. Only owned dead-probe sockets were removed; current app/native sessions were untouched.

## Output-size limit and alternatives

Installed source hardcodes `mAr = 10000`. `sye()` retains strings at or below 10,000 characters, otherwise persists them and returns a preview/file reference. UserPromptSubmit command stdout and additionalContext both go through this function without a threshold override. No corresponding process-local hook output-size environment option was found. The command-hook schema accepts a single additionalContext string; multiple JSON documents do not yield multiple context records.

The simplest common protocol is a bounded part envelope, for example a total UTF-8 byte budget of 8,000 including framing. This also stays below the native character threshold. Do not budget 8KB of body plus unbounded metadata. Subsequent parts need durable coverage and another receiver opportunity; never treat the first part as the whole result.

A same-turn larger batch is technically feasible through multiple command handlers. The experiment uses a file-locked compare-and-set bundle keyed by native session + prompt_id, so parallel handlers read one shared canonical claim and each returns a different slot. Native writes one attachment per handler, and arrival order is not guaranteed. Production would therefore require numbered, hashed fragment framing, all-fragment receipts, bounded slots, and crash/cancellation handling. That complexity is optional; one command returning several attachments is not supported by the inspected command-hook output path.

## Minimal configuration shape

This is an interface example, not a production implementation. All callback paths and files are private ConsensFlow paths; the settings file is passed only to the launched pane.

```json
{
  "hooks": {
    "SessionStart": [{"hooks": [{
      "type": "command",
      "command": "/Users/gabrielvoicu/.consensflow/runtime/node",
      "args": ["/Users/gabrielvoicu/.consensflow/integrations/claude-receiver.mjs", "register"],
      "timeout": 10
    }]}],
    "FileChanged": [{"hooks": [{
      "type": "command",
      "command": "/Users/gabrielvoicu/.consensflow/runtime/node",
      "args": ["/Users/gabrielvoicu/.consensflow/integrations/claude-receiver.mjs", "wake"],
      "asyncRewake": true,
      "timeout": 10
    }]}],
    "UserPromptSubmit": [{"hooks": [{
      "type": "command",
      "command": "/Users/gabrielvoicu/.consensflow/runtime/node",
      "args": ["/Users/gabrielvoicu/.consensflow/integrations/claude-receiver.mjs", "receive"],
      "timeout": 10
    }]}]
  }
}
```

Register returns:

```json
{"hookSpecificOutput":{"hookEventName":"SessionStart","watchPaths":["/Users/gabrielvoicu/.consensflow/workspaces/<workspace>/receivers/<owner>/signal"]}}
```

Receive returns, on exit 0:

```json
{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"<complete bounded framed part>"}}
```

Wake writes only `ConsensFlow inbox changed.` to stderr and exits 2 when work is pending, or exits 0 silently when empty. Its output must not contain a body, executable task, old native destination, or bearer capability.

## Remaining production requirements

- Native identity is not ownership authorization. Bind registration to the private launch capability. Reject independent forks; explicit continuation still requires a verified predecessor `continued-in` record and owned successor. Background continuation was tested by the earlier receiver probe, not repeated with FileChanged here.
- A synchronous read canceled after claiming must retain its body and uncertain claim. Absence in a short transcript snapshot alone does not justify replay. Confirm cancellation or reconcile durable native evidence.
- Startup watcher installation/catch-up, missed/coalesced filesystem events, repeated signals during in-flight claims, backlog draining, app reconnects and power/sleep cycles were not exhaustively tested. File notification is a hint; durable inbox state remains authoritative. No indefinitely-running hook waiter is needed, but robust catch-up/re-notification still needs design and tests.
- UserPromptSubmit empty callbacks caused no model calls; empty filesystem signals caused no model calls over the observed two-second window. This is not an all-day energy or reliability measurement.
- Source completion ingestion, native receipt parsing, app visibility and permission scoping remain separate responsibilities.

## Primary sources and reproduction

Official [Claude hooks reference](https://code.claude.com/docs/en/hooks) documents SessionStart watchPaths, externally triggered FileChanged events, asyncRewake, synchronous additionalContext, parallel execution and output visibility. The installed-binary and runtime evidence above establish the exact behavior claimed here; documentation alone did not establish the race or its mitigation.

Read-only binary anchors in `/Users/gabrielvoicu/.local/share/claude/versions/2.1.269`: `mAr` at byte 162148411; `ua` cancellation cleanup at 165231701; `YFr` async callback around 170755490; `sye` at 170757464; `nve` async spawn around 170771679; additionalContext persistence at 170820251. These offsets are version-specific inspection aids, not APIs.

Runnable isolated fixtures: `probe-filewatch.py`, `probe-filewake.py`, `probe-filewake-resume.py`, `probe-filewake-batch.py`, with `filewatch-hook.py` and `filewatch-batch-hook.py`. Re-run only in an explicitly isolated environment. No production source, installed app, live configuration, account credentials or live ledger was edited by this research.
