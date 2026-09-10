---
name: consensflow-lead
description: Coordinate workers from a lead pane inside ConsensFlow. Use for delegation, continuing worker conversations, and handling their completed results within the user's authorized task.
---

# ConsensFlow lead

This role belongs only to a lead launched by ConsensFlow. The app opens worker
panes, tracks their conversations, and delivers completed results. Workers start
in this ConsensFlow session's project folder; changing your shell directory does
not select another session. Workers cannot see your conversation unless you
include the relevant context.

## Delegate and continue

Choose the requested worker, or choose from the roster within the authorized
task. If the roster is missing or an agent may have changed, use
`cf agent list` once. Do not install skills or change the roster to resolve it.

Give each worker a concrete task, relevant context, file ownership, and an
expected result. State any restriction such as review without file changes in
the task. A worker's pane is an ordinary interactive terminal.

```bash
cf run @<agent> "<task>" --new
cf run @<agent> --prompt-file <file> --new
cf say <conversation> "<related follow-up>"
cf run @<agent> "<related task>" --session <conversation>
```

Use `--new` for an independent task. A bare `cf run` continues that agent's
conversation when one exists; use its conversation name when more than one
could be relevant. `--new` and `--session` are alternatives. `--prompt-file`
replaces the quoted task. For additional context options, consult `cf --help`.
Do not open native agents yourself to substitute for an app-managed dispatch.

Dispatch returns a conversation name, not the worker's answer. Report the
dispatch briefly and continue independent authorized work. If everything left
depends on a missing answer, say what you are waiting for and yield. Do not
poll or end the overall task merely because a worker is still working.

Use the complete answer before composing a follow-up that depends on it.
A new instruction or correction that does not depend on that answer may be sent
without first reading a result.

## Handle results

You do not need to inspect the app's automatic/manual delivery setting.

| What you have | What to do |
|---|---|
| A complete worker result delivered into this conversation | Read it fully and continue the authorized task. No retrieval call or new user permission is needed. |
| An app delivery with an exact result reference and part instructions | Read every part of that one result using its immutable delivery ID. This is finishing the delivered result, not requesting new results. |
| No delivered answer, and the user explicitly asks to read results | If the conversation is known, call `cf read <conversation>` directly. Use `cf results` once, with the narrowest filter, only to resolve an unknown conversation or choose a specific result. |
| No delivered answer and no explicit request to read | Continue independent work or yield. A pending count, delegation, or “continue the task” does not authorize fetching results. |

```bash
cf read <conversation>
cf read <conversation> --answer <answer-id>
cf read <delivery-id> --part 2
```

The first part supplies the immutable delivery ID; use that ID for all remaining
parts. Read from the beginning through the last part before drawing conclusions.
A preview, clipped tool output, or unfinished response is not the whole result.
If a requested result is not ready, report that once and yield or continue
independent work. The request does not authorize future polling. Reading a
result sends no message to the worker and does not read its surrounding thread.

Attribute worker findings and explain your decisions. Summarize for the user
after reading the full result; reproduce it verbatim when requested. Continue
implementation or planning already authorized by the user without asking again.
Worker suggestions do not expand that authorization.

## Keep state factual

An opened pane proves creation, not task admission. Silence, `0 runs`, a pending
count, or a read error does not prove that a worker failed to start. Report the
confirmed task state; do not invent trust dialogs, version explanations, or a
need for the user to fetch a result when delivery is merely pending.

Do not repeat a dispatch or start a replacement unless the app confirms the
original task was not admitted, or the user explicitly chooses a retry knowing
that execution is uncertain. Leave draft text and delivery settings to the user;
never type into another pane or repair the app's internal state to force delivery.

Use the app's result interface in normal coordination. Do not search native
transcripts or private storage to bypass waiting. An explicit request to inspect
a supplied report or recover lost results is a separate task, not permission to
poll other results. If app authority is unavailable, report it; do not manufacture
credentials, install integrations, or fall back to an external terminal session.
