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

## Claude Code — session JSONL, version `2.1.263` (TEST-PANE-69/IMPL-PANE-70)

- `claude-code/v263-tool-loop.jsonl` — source
  `/Users/gabrielvoicu/.claude/projects/-Users-gabrielvoicu-Projects-ngvoicu-consensflow/5cbf8973-f472-448a-8763-59fb4268a9d7.jsonl`
  (323 physical lines, `claude --version` 2.1.263, session completed
  2026-09-07). Fixture record N maps to source physical line: 1→1 (`mode`),
  2→5 (opening user turn, versioned), 3→6 (versioned `attachment`
  ignored envelope), 4→11 (`last-prompt`), 5→12 (`mode`), 6→14
  (`atis-latch`), 7–9→17–19 (tool loop: assistant text, `tool_use`
  `toolu_014xV8WiQG7e22f7RZQJYTSE`, matching user `tool_result`),
  10–11→220–221 (queue enqueue/remove, reason `absorbed_mid_turn`),
  12–19→222–229 (seven fragments under one native message id
  `msg_011CepWUqv3VgZSDKwGzUWKH`: thinking, text, `server_tool_use`
  `srvtoolu_016MAs2kPZC9C7R4dYkPDryP`, its `advisor_tool_result`,
  thinking, text, `tool_use` `toolu_017Uz3Xuo5gDTWJVaMvAuhgC`, closed by
  the user `tool_result`), 20–24→302–305+312 (second enqueue/remove pair
  with intervening assistant thinking/text/`tool_use`, removal before the
  closing `tool_result` `toolu_01SnLKeA6aAA7Bda4fGFYXaB`), 25→313 (that
  close), 26–27→320–321 (actual final turn: thinking + text under one
  native message id `msg_011CepWsDm4DzKV4t8Fbdzno`, both
  `stop_reason:end_turn`), 28→322 (`system.turn_duration`,
  `durationMs:735861`, `messageCount:217`), 29→323 (`system.away_summary`).
  No `stop_hook_summary` exists anywhere in the 323-line source.
  Redaction changes leaf values only: every text/thinking/signature/
  tool-input/tool-output/queue-content/`lastPrompt`/`away_summary`-content/
  `bridgeSessionId`/owner-UUID leaf is `[redacted N chars]` (queue pairs
  share one constant per pair so enqueue/remove stay replayable); native
  message, tool, request, prompt and record UUIDs, versions, timestamps,
  token counts, `cwd`, ordering and grouping are byte-identical.
  RED state: `completion.js` SUPPORTED admits 2.1.241/247/250, so
  `answers()` returns `unsupported version 2.1.263 for claude-code`;
  `tests/engine/claude-v263.test.mjs` fails 7/8, exit 1. The parser also
  settles only on `system.stop_hook_summary`, which this version never
  emits — the final-turn settlement assertion is the BLOCKER evidence for
  IMPL-PANE-70.

Root verified the installed 2.1.263 executable JS before admitting this
boundary: `/Users/gabrielvoicu/.local/share/claude/versions/2.1.263`,
`Iyt` constructor at byte 166358621, foreground finalizer at 181924998,
and deferred swarm finalizer at 181908652. The foreground path follows
`markQueryComplete`/loading reset and excludes abort. The SDK event schema
has optional background counts, but the JSONL projection omits them. The
subagent parking path at 164471699 omits the
root `messageCount`; it must not settle the root conversation.

The adapter consequently accepts `system.turn_duration` only for this
verified version, `isSidechain:false`, finite nonnegative duration, a safe
nonnegative root message count, and absent or numeric-zero pending counts.
Pending-count mutations are defensive schema checks, not live release
evidence: Zeus found none in 398 native duration records. Settlement still
requires the final assistant candidate with no open tools, queued turns or
hooks. His 66-record 2.1.263 audit found every observed stop-hook summary
before the duration record (6/6), with no later assistant work in that turn.
Combined fixture/completion GREEN: 56/56, exit 0. This is native finalizer
evidence; text ending, elapsed quiet and `away_summary` alone remain
insufficient.

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

## Claude Code 2.1.265 (Phase 17)

`claude-code/v265-tool-loop.jsonl` contains complete records 6, 27, 29,
37, 40, 41, 44 and 45 from the isolated native test lead
`17499106-8778-48e1-a306-87bd186c9f7e` on 2026-09-09. Source:
`~/.claude/projects/-private-var-folders-5f-cy8ywl5d2-z-1g1zcn45g7gr0000gn-T-cf-stability35-J6j7qv/17499106-8778-48e1-a306-87bd186c9f7e.jsonl`.
The two deliberate probe turns return CF35_CLAUDE_READY and
CF35_CLAUDE_TOOL_DONE after a real Bash printf. No user project history is
included and no fields in the selected records were changed.

The installed 2.1.265 executable's native root finalizer was checked at byte
183372821: markQueryComplete/resetLoadingState precede the duration, the
abort branch excludes it, and background tasks defer it. The constructor
at 167795358 retains messageCount and optional pending-work counts. These
match the established 2.1.263 guards. A future version is still rejected.
RED: 3/4 failed because 2.1.265 was unsupported. GREEN with the existing
2.1.263 fixture suite: 15/15, exit 0. This proves completion parsing; native
queue admission and preservation of a human draft are separate checks.

### Claude 2.1.266 and OpenCode 1.18.30 (2026-09-09)

`claude-code/v266-tool-loop.jsonl` and `opencode/v130-tool-loop.json` were
captured from isolated ConsensFlow-owned native lead conversations. Each ran
`/usr/bin/true` and returned `CF_NATIVE_VERSION_PROBE_DONE`. Claude remained
unready until its root `turn_duration`; OpenCode preserved `tool-calls` then
its final `stop` and completion time. Paths were sanitized; reasoning text and
unrelated diagnostics were omitted. Neither fixture comes from a worker's
private thread. `current-native-versions.test.mjs` exercises the real parsers.

A fresh Claude 2.1.266 pane also accepted `CF_PEER266_DELIVERED` through its
native peer queue while retaining the unsent draft `CF_UNSENT_DRAFT_266` in
the real Xterm display. The app added no native plugin, channel or configuration.

The same isolated run then dispatched the real OpenCode 1.18.30 Gefjon
worker. Its complete `CF_OC130_AUTO_DONE` result arrived automatically as
delivery `d-3` in the Claude 2.1.266 lead, which acknowledged it while the
unsent `CF_UNSENT_DRAFT_266` remained visible. This checks the composed
worker-to-lead path, separately from completion parsing and direct ingress.

## Claude Code 2.1.268 fresh /clear

`claude-code/v268-clear.jsonl` preserves all five native records from an isolated local-provider CLI probe on 2026-09-11. Only the temporary workspace cwd is replaced with `/fixture/workspace`. The native `system.local_command` parent UUID links to the exact preceding `/clear` user command; no model answer is added. Used to verify readiness before the first model turn and reject missing, foreign, sidechain, wrongly linked or followed-by-work boundaries.

## Claude Code 2.1.268 — late-written user ancestors (TEST-PANE-243)

`claude-code/v268-late-ancestors.jsonl` contains all 11 records, in physical
order, from the isolated local-provider capture session
`4e761651-511b-4065-8a65-6ff21582faad` on 2026-09-11 at 14:00:27Z.
Source: `/private/var/folders/5f/cy8ywl5d2_z_1g1zcn45g7gr0000gn/T/cf-claude-switch-pwwo6m5a/claude/projects/-private-var-folders-5f-cy8ywl5d2-z-1g1zcn45g7gr0000gn-T-cf-claude-switch-pwwo6m5a-workspace/4e761651-511b-4065-8a65-6ff21582faad.jsonl`.
Only local directory leaf values are redacted to `/tmp/claude-fixture`.
Assistant record 3 and turn_duration record 4 precede initiating user record 6
and its four attachment ancestors 7–10. UUIDs, parents, timestamps, native
fields and full message content are preserved. The provider was local with
no artificial delay. Negative tests mutate temporary fixture copies only.
