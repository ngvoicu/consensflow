# Keeping Devin's own terminal UI

Date: 2026-09-12. Follow-up to the user's explicit request to explore the stock
TUI before selecting a ConsensFlow chat pane. This is research and private native
probing; no Devin product integration or UI replacement was implemented.

## Finding

**Keeping Devin's TUI is feasible for native hook-based collection. Full automatic
collection after genuine idleness is still unresolved.** The earlier ACP-pane
recommendation was not an approval to replace the TUI. Prefer the stock UI unless
the user explicitly accepts a different interaction surface.

The stock TUI can wait in a native `Stop` hook, receive a complete result as the
hook's continuation reason, and continue in the same native conversation. This
was verified with three consecutive results, not just one. Waiting did not issue
additional inference requests to the local mock backend. An unsent editor draft
survived the continuation. Native `/new` and `/resume` supplied the correct native
identities to lifecycle hooks after cancellation had finished.

The drawback is real: a running Stop command delays completion of native
cancellation until the command returns. Keeping that hook blocked forever would
make interruption and session changes unresponsive. Once the hook returns and
Devin truly stops, it no longer provides a standing receiver. A late result can
then enter through `UserPromptSubmit` when the user sends their next message.

This is an integration option with a clear limitation, not proof of parity with
the complete idle delivery requirement. Do not silently lower that requirement.

## Native evidence

The actual **3000.10.21 TUI** ran in private PTYs; this was not an ACP script
pretending to be a terminal. Its bundled native ACP child remained under the
stock TUI's control. Each probe used fresh HOME/XDG/config/data/cache/tmp paths,
a private workspace, fixed synthetic backend replies, dummy local credentials,
and a macOS sandbox denying external networking and writes outside its probe
root except `/dev`. Neither installed binary was modified or updated.

The core repeat/late/cancel cases were also run through the **installed 3000.6.14
TUI**. Final verification covers **11 scenarios and 54 assertions**, with no
assertion failures and no remaining probe-owned native processes. These are
native transport observations, not installed ConsensFlow acceptance tests.

| Scenario | Native result |
| --- | --- |
| Wait for a result, latest | Native inference count stayed at one during the wait; returning the complete result triggered the second inference in the same TUI |
| Three separate results, both versions | Three native Stop continuations yielded four total mocked inference calls; each exact result body existed once on the active native history chain |
| Unsent draft, latest | The draft remained in the editor and never appeared as a submitted native message |
| Cancel while waiting, both versions | Native cancel reached the ACP child, but the TUI reported cancellation only after the Stop hook returned; no second inference ran |
| `/new`, latest | After cancellation completed, a new native identity was selected; the old hook result remained absent from the new history |
| `/resume <exact id>`, latest | `SessionStart` reported `source: resume` and retained the exact native identity; the next prompt used that identity |
| Late result after a completed turn, both versions | No inference occurred during the idle observation; the next human prompt received the complete result through native additional context |
| Remote bridge environment variables, latest | Setting a private PTY bridge port and remote state directory did not start a listener or create remote state in an ordinary local TUI |

The native Stop payload includes `session_id`, `prompt_id`, `stop_hook_active` and
`last_assistant_message`. Returning a blocking decision continues the agent with
the reason. `UserPromptSubmit` accepts `additionalContext`; its native session and
prompt identity provide the boundary for a synchronous fetch. The public hook
contract documents these event-driven mechanisms, although the tested Stop
payload includes more fields than the event table lists.
[Native lifecycle hook contract](https://docs.devin.ai/cli/extensibility/hooks/lifecycle-hooks)

Native documentation specifies Enter to submit and Escape/Ctrl+C to cancel.
Initial PTY input trials sent adjacent Escape bytes and text plus Enter too
quickly; the terminal treated them as a single escape/paste-like input sequence.
Those trials are excluded from new/resume claims. Final probes separated key
presses and confirmed the resulting native session identities.
[Keyboard contract](https://docs.devin.ai/cli/reference/keyboard-shortcuts)

### Receipt and completion are different

The complete Stop continuation reason was stored as a native **user** message.
The next-prompt additional context was stored as a native **system** message.
Each had a native message UUID. The fixed bodies were also present in actual
requests to the loopback backend when inference resumed.

On cancellation, the Stop reason still entered native history even though no
second inference ran. Therefore a body present in history proves insertion, not
that the agent has acted on it. Do not describe a cancelled continuation as a
processed result. Preserve the result, receipt evidence and cancellation status.

The documented `--export <path>` option produced ATIF-v1.7 files in the private
root. Exports include the selected native session and full steps, but the tested
export did not supply a definitive per-turn completion status. A cancelled
history could contain previous assistant text and a later user continuation.
Thus neither export existence nor the last assistant row is sufficient by itself
to mark a worker's next answer complete. Completion evidence still needs its own
verified native event association.
[Export option](https://docs.devin.ai/cli/reference/commands)

### Private reproducible artifacts

- [PTY probe script](/Users/gabrielvoicu/.consensflow/tmp/devin-tui-research/main/probe-tui.py)
- [Case manifest, 54 assertions and evidence hashes](/Users/gabrielvoicu/.consensflow/tmp/devin-tui-research/main/verification.json)
- [Latest: three replies](/Users/gabrielvoicu/.consensflow/tmp/devin-tui-research/main/probe-1789240079678162000/summary.json)
- [Installed: three replies](/Users/gabrielvoicu/.consensflow/tmp/devin-tui-research/main/probe-1789240111280024000/summary.json)
- [Latest: draft preservation](/Users/gabrielvoicu/.consensflow/tmp/devin-tui-research/main/probe-1789239731411809000/summary.json)
- [Latest: cancellation](/Users/gabrielvoicu/.consensflow/tmp/devin-tui-research/main/probe-1789240024760700000/summary.json)
- [Latest: new session](/Users/gabrielvoicu/.consensflow/tmp/devin-tui-research/main/probe-1789239900740816000/summary.json)
- [Latest: resume](/Users/gabrielvoicu/.consensflow/tmp/devin-tui-research/main/probe-1789239900756165000/summary.json)
- [Latest: collection on next prompt](/Users/gabrielvoicu/.consensflow/tmp/devin-tui-research/main/probe-1789240024755980000/summary.json)

Each directory retains its private configuration, raw terminal output, native
hook events, child logs, private SQLite history and mock request evidence. The
native wire logging environment option was used as diagnostic evidence; it is
not asserted to be a supported production API. No model quality, real account,
permission/tool workflow, long multipart, crash recovery or production receiver
acceptance is established by these probes.

## Other wake paths checked

The separate [wake-surface investigation](research-tui-wake-surfaces.md) covers
actual native MCP probes and first-party Outposts/remote evidence. MCP list,
resource and log notifications did not start another model request. Native
sampling was explicitly unsupported. Devin's `/loop` performs iterative code
review; it is not a timer for fetching external results. Native background
subagents can notify their parent, but they are model-driven Devin subagents and
the latest release keeps the parent turn active while they run.

Remote PTY support controls raw terminal bytes, resizing and process lifecycle.
It does not supply an atomic local-TUI selected-session/draft/submit operation.
An additional remote shell/toolbox process would not close that gap. No binary
patches, debugger injection, stolen file descriptors, credential proxying or
public cloud-session calls were used to make the stock TUI appear integrated.

## Smallest viable TUI design

If delayed collection is acceptable, keep the native TUI and use one private
adapter for role initialization, exact native session registration and fetching
from the existing inbox during supported native events. Every reply keeps its
own result identity and pending state; a later reply never replaces an earlier
one. The existing lead/PM Results view and worker badges remain the visible place
to see what has not been collected.

A short bounded Stop wait could collect replies for work the coordinator is
already awaiting. The next user prompt collects anything that arrived afterward.
An indefinite Stop wait is not recommended because of the demonstrated
cancellation delay. A wait timeout must return without silently discarding or
marking any outstanding result as read.

The helper/configuration would ship with ConsensFlow and be prepared under the
configured ConsensFlow home. It would use the same inbox claims, receipt checks,
PM/advisor ownership and per-result history as the other harnesses. No global
plugin install or project files are needed. Advisor role restrictions remain
unchanged; native hook experiments do not prove those restrictions implemented.

For full automatic collection while retaining the stock TUI, the missing native
capability is a supported idle notification or TUI extension which can fetch for
the current native session without modifying the editor. A Claude-style
wake-only notification followed by synchronous native context fetching would be
a suitable contract. Alternatively, a native callback that reports selection and
can atomically enqueue external context would work. The existing plugin surface
does not currently expose that contract; no such API is fabricated here.

This follow-up keeps the native TUI under consideration. The user has not accepted
delayed collection, and no custom ACP chat pane is selected or implemented.
