# Completion fixtures — provenance

Prepared 2026-09-06 and extended 2026-09-07 from the native stores on this
machine. Every JSONL line and every SQLite row selected below is a complete
source record: records not needed by a case are omitted, but fields inside a
selected record are not.
Redaction changes leaf values only:

- long or opaque strings may be replaced by `[redacted N chars]`;
- the long-text guards replace one text leaf with repeated `B`, `X`, `P`,
  or `O` characters while preserving the asserted length;
- the cross-harness 60,000-character guard changes only that text leaf to
  repeated `L` in each staged temporary copy; the fixture on disk is unchanged;
- the version-rejection test mutates only the version leaf in its staged
  temporary copy.

The OpenCode JSON files are transport envelopes for complete rows. The
`session`, `message`, `part`, and `event` arrays retain every column;
JSON-valued `data` columns remain strings. Tests load those rows into a
throwaway SQLite database. Production stores are never written.

## Codex — rollout JSONL, `codex-cli 0.153.4`

- `completed.jsonl` — source
  `/Users/gabrielvoicu/.codex/sessions/2026/09/06/rollout-2026-09-06T07-14-10-01a074ec-7aff-74b0-8cf6-aa00d8e451cb.jsonl`;
  ordinals 0, 2477–2483, and 2485 for turn
  `01a077c2-5d0f-7452-a2be-ace096bbe3be`. It preserves both native user
  IDs, the `AgentMessage phase:final_answer`, its response mirror, and
  `task_complete.last_agent_message`.
- `errored-task-complete.jsonl` — the same source; ordinals 0, 2009,
  2013–2014, 2017–2018, 2020, 2045, 2047–2048, and 2056–2057. The decisive records are
  the commentary answer, spawned sub-agent activity, tool output,
  `task_complete.error.codex_error_info:server_overloaded` with
  `last_agent_message:null`, and the later sub-agent completion.
- `interrupted.jsonl` — source
  `/Users/gabrielvoicu/.codex/sessions/2026/09/06/rollout-2026-09-06T21-23-51-01a077f6-6663-7bc2-81cd-e287ccaabdbd.jsonl`;
  ordinals 0, 1, 9, 10, and 764, ending in the real
  `turn_aborted {reason:interrupted}`.
- `forked.jsonl` — source
  `/Users/gabrielvoicu/.codex/sessions/2026/09/06/rollout-2026-09-06T21-28-10-01a077fa-5968-7b62-8fdd-043410a3d4b9.jsonl`;
  ordinals 0, 7, 10, 93–94, and 97. It retains the complete opening
  `session_meta`, including `id`, `session_id`, and `forked_from_id`, plus a
  native user item, final `AgentMessage`, response mirror, and
  `task_complete` boundary.
- `big-answer.jsonl` — complete records at ordinals 0, 2477, 2480, and
  2481 from the `completed.jsonl` source. Only the final answer text leaf is
  replaced by exactly 60,000 `B` characters.

## Claude Code — session JSONL, versions `2.1.241`, `2.1.247`, and `2.1.250`

- `fragments.jsonl` — source
  `/Users/gabrielvoicu/.claude/projects/-Users-gabrielvoicu-Projects-ngvoicu-consensflow/15fba934-d727-4777-8791-123675a63649.jsonl`;
  physical lines 117–121 and 128. All five assistant fragments carry native
  message ID `msg_011CeTZ4moLoyUafGxCpFzhW`; they include
  `server_tool_use`, `advisor_tool_result`, later thinking, and the final
  text before `system.stop_hook_summary`. The 7,302-character final text
  leaf is replaced by 7,302 `X` characters.
- `frontier-history.jsonl` — the same source, physical lines 91, 93, 107,
  238, and 246. It retains a verified interrupt followed by a newer human
  turn, an orphaned advisor call from an older native message, and an
  enqueue/remove pair; only ordinary prompt and queue-content leaves are
  redacted.
- `queued-turn.jsonl` — source
  `/Users/gabrielvoicu/.claude/projects/-Users-gabrielvoicu-Projects-ngvoicu-consensflow/1b09fb15-feb1-4595-9f47-5eb9ff768191.jsonl`;
  physical lines 2219, 2220, 2227, 2229, and 2232: queue enqueue, apparent
  answer, dequeue, three-hook summary, and the queued user turn.
- `queue-pop-all.jsonl` — the same source; complete queue-history records at
  physical lines 6765, 6767, 6774–6778, 7440/7444, 7755/7764/7768, and
  7774/7781, the consuming assistant fragments at 6782–6783, and the final
  answer/stop boundary at 8311–8312/8320. Repeated queue contents are redacted
  to the same leaf value so `popAll`, `dequeue`, and later consumption remain
  replayable.
- `interrupted.jsonl` — the same source, physical line 3261, including
  `interruptedMessageId` and user text `[Request interrupted by user]`.
- `provider-429.jsonl` — source
  `/Users/gabrielvoicu/.claude/projects/-Users-gabrielvoicu-Projects-ngvoicu-consensflow/33383216-87a0-4e6d-a273-07c4b229cdb1.jsonl`;
  complete physical line 1040, including `isApiErrorMessage:true`, status 429,
  native error details, message ID, and user-facing failure text.
- `compaction.jsonl` — source
  `/Users/gabrielvoicu/.claude/projects/-Users-gabrielvoicu-Projects-ngvoicu-consensflow/1b09fb15-feb1-4595-9f47-5eb9ff768191.jsonl`;
  physical lines 4495, 4497, 4500, 4508, and 4509: a completed answer and stop
  hook, the next user turn, the full `compact_boundary` record, and the
  preserved post-compaction user record.

## Pi — session JSONL, protocol `3` (Pi `0.85.1`)

- `between-tool-steps.jsonl` — source
  `/Users/gabrielvoicu/.pi/agent/sessions/--Users-gabrielvoicu-Projects-ngvoicu-consensflow--/2026-08-24T18-00-00-703Z_hazy-ridge.jsonl`;
  physical lines 1 and 4–8. Both tool results close by exact `toolCallId`;
  the next assistant tool step appears almost three seconds later.
- `tool-loop.jsonl` — the same source, physical lines 1, 4–7, and 114.
  It preserves the two native calls/results and a terminal assistant
  `stopReason:stop`. The final 1,878-character text leaf is replaced by
  1,878 `P` characters.
- `provider-429.jsonl` — source
  `/Users/gabrielvoicu/.pi/agent/sessions/--Users-gabrielvoicu-Projects-ngvoicu-consensflow--/2026-08-26T10-14-49-150Z_triton-jade-fern.jsonl`;
  physical lines 1 and 6–9. It retains both complete provider-429 attempts and
  the eventual successful assistant record, with no intervening user turn.

Pi 0.85.1 has no persisted post-run boundary:
`/opt/homebrew/lib/node_modules/@earendil-works/pi-coding-agent/dist/core/agent-session.js:772–810`
runs retry, compaction, and queued continuations before emitting
`agent_settled`, while the same file at lines 347–355 emits that event only to
the in-memory extension/runtime stream. The installed provider retry setting
names a 60,000 ms maximum at
`/opt/homebrew/lib/node_modules/@earendil-works/pi-coding-agent/dist/core/settings-manager.js:610–615`;
completion therefore uses twice that value, 120,000 ms, as its derived
quiet-window boundary and never labels Pi settlement native.

No Pi user-cancellation marker was captured, so there is no synthetic
cancellation fixture and provider errors are not reclassified as cancellation.

## Kimi — `wire.jsonl`, protocol `1.5`

- `tool-result.jsonl` — source
  `/Users/gabrielvoicu/.kimi-code/sessions/wd_consensflow-site_ed8b7a271238/session_11c123b3-dd33-4f21-8862-beabdc50cd18/agents/main/wire.jsonl`;
  physical lines 1, 747–748, 750, 755–759, 861, and 866–868. The real result has
  `parentUuid` and `toolCallId`, no `turnId`; the case ends with native
  `turn.ended {reason:completed}`.
- `superseded-tool.jsonl` — the same source, physical lines 1, 320–321, 323, 329,
  747–748, 750, 755–759, 861, and 866–868. It retains the unmatched
  `Bash_47` from turn 1, the later native prompt, and turn 5's completed native
  boundary; only prompt, command, and final-answer leaves are redacted.
- `provider-429.jsonl` — the same source, physical lines 1, 5–6, 315, 317,
  and 318. It preserves `turn.ended {reason:failed}`, the provider rate-limit
  code/name/message, and its final error step; only the prompt leaf is
  redacted.
- `protocol-1.4-no-turn-ended.jsonl` — source
  `/Users/gabrielvoicu/.kimi-code/sessions/wd_btb_3cabe80dc1f7/session_159aa36f-e114-4bef-a9d2-144efdb84c10/agents/main/wire.jsonl`;
  physical lines 1, 191, and 1586/1588–1590. The final step says
  `finishReason:end_turn`, but protocol 1.4 has no `turn.ended`; it is an
  unsupported-version fixture, never an in-flight fixture.
- `admitted-prompt.jsonl` — the protocol-1.5 source above, complete physical
  lines 1 and 866–872. The prefix through real line 870 ends on
  `prompt.accepted` after a settled turn; line 871 is the following
  `turn.prompt` that consumes that admission, and line 872 supplies the same
  native `message.id` in `context.append_message`.

Real protocol-1.5 `turn.prompt` records in this source have neither UUID nor
`promptId`; their immediately following user-origin `context.append_message`
records carry the native message IDs. A preceding `prompt.accepted` exposes
that same ID one record earlier. The adapter reconciles those records instead
of minting positional or text-derived IDs. Assistant IDs are native
`step.begin/step.end.uuid`; tool-result IDs are the native call `parentUuid`.
No Kimi cancellation reason was captured, so none is inferred.

## OpenCode — `opencode.db`, versions `1.18.27` and `1.18.29`

All OpenCode row fixtures come from
`/Users/gabrielvoicu/.local/share/opencode/opencode.db`.

- `completion-window.json` — session
  `ses_f88c0c7cdffeANJRVLwiBceADi`, message
  `msg_0773f385a001oy2xD1d5J3DNge`, all four part rows, and complete event
  rows 12, 13, and 15. Event 12 is `step-finish reason:stop`; event 13 has
  native `finish:stop` but no `time.completed`; event 15 adds
  `time.completed:1788707030665`, 92 ms after event 12.
- `finish-length.json` — session
  `ses_f886ed7dbffe1myK161SPisoR2`, assistant message
  `msg_0779166e7001sYP03lgChIzdvh`, and all four parts. It retains native
  `time.completed` and `finish:length`.
- `api-error.json` — session `ses_f9905d94effe57fADEYMVwfKVF` and
  assistant message `msg_066fa2779001GkIYu5W7ugNXUR`. It retains
  `time.completed`, `error.name:APIError`, status 403, and the provider
  message; cookie/body leaves are redacted.
- `tool-result.json` — session
  `ses_f87e22f72ffewC2qJ2dAyyfPe1`, its original user message and text part,
  assistant messages
  `msg_07834cf2a001ZxKIEEils8Twxd` and
  `msg_07834e4450017pCEw2eVdPoUxQ`, and all eight part rows. It retains the
  completed tool-part output and terminal native completion. The final text
  leaf is replaced by exactly 4,515 `O` characters.
- `native-events.json` — complete decisive `message.updated.1` or
  `message.part.updated.1` event rows for every message/part row in the four
  cases, plus the completion-window sequence. For `ses_f87…`, these include
  original-message admission at sequence 1, assistant admissions at 2342 and
  2354, final completion at 2362, the intervening session update at 2363, and
  the original user's metadata-only summary update at 2364. Native `seq` values
  supply the positions encoded in adapter-minted opaque cursors; timestamps
  remain display evidence and are never ordering positions.
