# Devin native transport: focused follow-up research

Date: 2026-09-12. Scope: find a supported receiver-pull integration that preserves the stock Devin terminal UI. No product changes, installed-app changes, real-session reads, real credentials, or account/model requests were used in this investigation. Later native turn probes used synthetic responses from a loopback mock service with external networking denied.

**Later user steering:** the user prefers exploring the stock TUI before choosing
this ACP-pane recommendation. [Subsequent real-TUI probes](research-devin-tui.md)
verify native lifecycle collection and its idle/cancellation limitations. No
custom conversation pane has been selected or implemented.

## Decision

**The cleanest supported complete integration remains one ConsensFlow-owned ACP connection and pane for Devin.** That is an architectural recommendation, not a claim that the pane exists or that the user has approved its interaction design. No supported way to attach the receiver to the already-running stock terminal was found. Several earlier uncertainties can now be resolved with native evidence, including the latest stable binary.

The strongest negative findings are concrete:

- `FileChanged` is rejected by the native hook parser, rather than merely missing from documentation.
- Adding `async: true` does not make a command hook asynchronous.
- A second ACP process cannot load an actively owned session: native Devin rejects it as `session_locked`.
- The terminal launches its own `devin acp` child using the current executable. The documented flags/configuration do not offer a replacement child command, socket, or endpoint.

This makes a private hook installer useful for role instructions and collecting on a subsequent native event, but insufficient for the existing promise that a reply arriving after complete idleness can wake the selected conversation. A binary interception or terminal-input workaround would add fragile machinery around the same ownership problem the inbox redesign removed.

## Versions and provenance

The installed executable remains **3000.6.14 (18033302)**. The official stable changelog and public distribution manifest now identify **3000.10.21 (611c1cba)** as the September 10 release. Its changes include keeping a turn active while native background subagents run; they do not document an external idle receiver or configurable terminal transport. Native subagents are Devin-managed agents, not arbitrary ConsensFlow workers. [Official stable changelog](https://docs.devin.ai/cli/changelog/stable)

The official installer was downloaded and read, **not executed**. Its public current manifest selected:

```text
https://static.devin.ai/cli/current/manifest.json
https://static.devin.ai/cli/3000.10.21/devin-3000.10.21-aarch64-apple-darwin.tar.gz
SHA256 c0b97f8197bf3ce895ff14aa19257c511154b49a0a195bba4962acb5e475c68e
```

The 56,193,695-byte archive was checksum-verified and extracted only beneath:

```text
~/.consensflow/tmp/devin-native-research/docs/devin-3000.10.21/
```

The installed binary, global configuration and current app were not replaced. Download provenance: [official installer](https://cli.devin.ai/install.sh), [official manifest](https://static.devin.ai/cli/current/manifest.json). Captured files are `official-install.sh` and `latest-manifest.json` in the private research directory.

## What the supported surfaces actually provide

### Hooks

The public hook interface has eight lifecycle events: tool-before/tool-after, permission request, user prompt, stop, post-compaction, session start, and session end. Command hooks receive native `session_id` and per-turn `prompt_id`. The documented fields provide a command or prompt evaluator and a timeout; no asynchronous re-wake or watch-path contract is provided. The same format can be installed in a privately supplied user config, so project files are unnecessary. [Hook interface](https://docs.devin.ai/cli/extensibility/hooks/overview)

`Stop` is a point before a turn finishes. Returning a blocking decision can continue the agent with a reason; the documentation warns that this can loop. Startup and prompt hooks can supply additional context. These are event-triggered callbacks, not a persistent independently callable receiver. [Lifecycle hooks](https://docs.devin.ai/cli/extensibility/hooks/lifecycle-hooks)

**Native probe, both versions:** a SessionStart command recorded start/end files, slept two seconds, and returned a fixed private context marker. `session/new` returned without invoking it; the first `session/prompt` invoked it before the deliberate unauthenticated error. The control took 2.11 seconds on 3000.6.14 and 2.10 seconds on 3000.10.21. With `async: true`, the prompt still waited approximately 2.1 seconds and the command had finished before the response. Thus that unknown extra field does not activate Claude-style asynchronous behavior. This is timing/side-effect evidence; it does not infer async behavior from the presence of generic parser strings.

**Native probe, both versions:** a `FileChanged` entry is rejected with the native warning:

```text
Ignoring invalid value for "hooks" ... unknown variant `FileChanged`,
expected one of `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `Stop`,
`PostCompaction`, `SessionStart`, `SessionEnd`, `PermissionRequest`.
Using the default ({}) for this setting.
```

All hook discovery was consequently empty in that probe. Copying the existing Claude receiver configuration to Devin would disable those hooks; it would not create a watcher.

Private evidence: `probe-hooks.py`, `probe-hooks-latest.py`, each case's `summary.json` and `wire.json`, and the corresponding `data/devin/cli/logs` beneath `docs/hook-probes*/`.

### Plugins and cogs

The documented plugin package can contribute skills, rules, hooks, MCP servers and custom subagent profiles. Its manifest is not a JavaScript TUI extension entrypoint comparable to Pi or OpenCode. Installing a plugin is user-scoped; default installs also sync to Devin Cloud, whereas `--local` limits installation to the device. A role-only integration should use private per-launch configuration instead of invoking the default global installer. [Plugin format and installation](https://docs.devin.ai/cli/extensibility/plugins/overview)

The binary contains `Cog`, `CogAction`, declarative rule and lifecycle types. Those strings demonstrate internal implementation concepts; they do **not** establish a supported loadable native UI extension or an external event receiver. The bundled `declarative-repo-setup` skill generates environment blueprints; it is not a live TUI extension. No conclusion here depends on treating internal Rust type names as callable APIs.

### Terminal configuration and notifications

`notify` controls output from Devin to the terminal: terminal bell and OSC desktop-notification sequences. It does not accept external notifications or add a session input channel. The documented agent configuration controls its model and history display; no ACP executable or endpoint override is listed. [Configuration reference](https://docs.devin.ai/cli/reference/configuration/config-file)

The exact latest native `--help` confirms `--config`, `--resume`, `--prompt-file`, `--export`, and the separate `acp` subcommand. Native `acp --help` states that the REPL spawns a `devin acp` child; the child inherits model and refusal-fallback environment values. `--agent-type` selects the built-in summarizer or review specialization, not an externally supplied implementation. Captured help: `docs/native-help.txt`, `docs/native-acp-help.txt`, `docs/native-version.txt`.

Targeted binary inspection of both versions found the diagnostic `resolving current executable for devin acp` in the native child-launch module. A PATH wrapper is therefore not an established interception point. `CHISEL_PURE_ACP_WIRE_LOG` and `CHISEL_PURE_ACP_STDERR` are diagnostic clues, not documented writable command channels. No binary modification or descriptor injection was attempted.

### ACP ownership

`devin acp` is the documented JSON-RPC stdio server intended for an editor or host. This is a supported host-owned connection, rather than a command for attaching to the terminal's private child. [Devin command reference](https://docs.devin.ai/cli/reference/commands)

In the latest private probe, initialization advertised `loadSession: true`, session listing/deletion/additional directories, embedded prompt context, modes and configuration choices. There was no advertised session-resume or session-close capability. Native loading should follow advertised `session/load`; the earlier 3000.6.14 probe already rejected `session/resume`. ACP distinguishes loading history from optional resume capabilities. [ACP session setup](https://agentclientprotocol.com/protocol/v1/session-setup)

**Native two-process probe on 3000.10.21:** process A created a private session and persisted one user marker; A remained alive. Process B initialized and attempted `session/load` with the same session/database. The response was:

```json
{
  "code": -32015,
  "message": "Session 'level-jackrabbit' is already open in another process. Close the other instance before opening it here.",
  "data": {
    "cognition.ai/errorKind": "session_locked",
    "cognition.ai/retryable": true
  }
}
```

B's subsequent `session/prompt` returned `-32016`, `session_not_found`. This rules out a second native ACP process as a clean delivery path into an existing terminal session on the tested version. Evidence: `docs/probe-two-clients.py`, `docs/two-clients/summary.json`, `docs/two-clients/wire.json`.

## Alternatives and why they do not solve the complete requirement

| Alternative | What it could do | Remaining problem |
| --- | --- | --- |
| Private startup/prompt/Stop hooks | Fetch during supported native lifecycle events | No callback for an arbitrary reply arriving after the turn ended |
| Long-poll a Stop hook | Wait for known workers without paid empty model turns | Keeps the turn open; timeout or cancellation ends the opportunity; draft, new-session and resume fencing still require proof |
| Force a Stop-hook continuation repeatedly | Periodically check again | Creates empty model turns or a loop and defeats real idle behavior |
| Poll via a normal tool | Wait during an agent-controlled tool invocation | The model must keep invoking/waiting; it is not an independent idle receiver |
| Second ACP process using the same session | Attempt access to stored history | Explicitly rejected while another native process owns the session |
| Restart the terminal on each incoming reply | Reopen stored history with a fresh initial prompt | Disrupts editing and requires a trustworthy selected-session and draft fence before terminating the old process |
| Write text into the PTY | Make the terminal process input | Cannot prove the exact conversation/draft state through a supported atomic boundary |
| Intercept the private child transport | Potentially observe native selection and submit commands | No supported override found; binary/descriptor interception would create a brittle additional integration |
| One ConsensFlow ACP host | Own selected session, prompt lifecycle and idle fetch directly | Requires a real chat surface and its permission/cancellation/history interactions |

The long-poll option is worth distinguishing from a paid heartbeat: the waiting hook itself need not run inference. Nevertheless it does not meet the requirement for **all** replies, including later replies after all previous activity ended, without indefinitely preventing completion. It should not be presented as equivalent automatic idle delivery.

## Existing ACP terminal clients

A configurable third-party ACP terminal could preserve a terminal-shaped pane,
but it would still replace Devin's stock UI. Source review found no smaller
complete receiver integration in the candidates checked:

- **Toad** accepts a configurable ACP command. Its draft and submission state
  remain internal Python/Textual UI state; an external protocol proxy cannot
  observe an unsent draft just by seeing ACP requests. This adds a runtime and
  another client adapter without establishing the required insertion boundary.
  [Pinned Toad command implementation](https://github.com/batrachianai/toad/blob/dd4f90e8b3700c3de80ad4b0eaa488ad0105e2c1/src/toad/cli.py#L166)
- **Martty** exposes ACP session status and timers to plugins, but its documented
  plugin API keeps the follow-up queue private and does not expose prompt
  submission or draft access. Those missing controls would require upstream
  changes or a maintained fork for this receiver contract.
  [Martty plugin contract](https://github.com/openma-ai/Martty/blob/main/docs/plugins.md)
- **acpx** is explicitly headless. It does not remove the need for a conversation
  surface or its draft/permission interactions.
  [acpx project](https://github.com/openclaw/acpx)

These are source-level comparisons, not native acceptance tests of those
clients. The inference is architectural: owning the ACP pane inside the existing
ConsensFlow app requires fewer independent components than a new terminal client
plus a custom receiver integration. It still entails UI work and is not a tiny
hook-only change.

## Smallest coherent ACP design, if selected

Keep the common inbox, receiver claims and receipt rules. Add **one Devin ACP adapter and one host-owned conversation surface**, used consistently for worker, lead and advisor roles. Do not maintain separate stock-terminal and ACP delivery implementations for different Devin roles unless a concrete requirement justifies that complexity.

The host would:

1. Spawn one native `devin acp` process with private configuration/storage and let Devin retain responsibility for its authentication.
2. Create or load an exact session, register that identity with the common receiver, and revoke the old lease before selection changes.
3. Serialize human prompts and inbox prompts on that connection. Fetch only when the selected session is idle and no human draft or permission interaction is active.
4. Preserve all streaming updates and distinguish successful turn completion from cancellation, interruption, refusal and errors. ACP returns a final stop reason for each prompt; assistant chunks alone are not completion. [ACP prompt lifecycle](https://agentclientprotocol.com/protocol/v1/prompt-turn)
5. Confirm the complete unique inbox claim in native conversation history. RPC success alone remains insufficient receipt evidence.
6. Render user/assistant/tool messages, approvals, cancellation, model/mode selection and replay. Existing Results history and pending counts remain shared with the other harnesses.

Native success, cancellation, draft protection, new/load, restart and multiple queued reply probes must pass before calling this a complete integration. The hook probes stop at missing authentication. The subsequent local mock probes below do establish native nonempty turns, cancellation and session lifecycle behavior. They do not establish live account/model behavior or a finished ConsensFlow integration.

## Successful native ACP lifecycle probes

The main investigation exercised the actual installed **3000.6.14** and privately
extracted **3000.10.21** binaries against a loopback-only mock vendor service. The
service returned fixed synthetic text; it did not run a model. Private test
credentials were dummy values usable only by that mock. A macOS sandbox denied
external networking and writes outside the private probe root except `/dev`.
The native executables were not patched, replaced or given real account secrets.

On **each version**, the probe produced eight nonempty `end_turn` replies and one
streaming cancellation, with **36 assertions passing**. Across the two versions:
16 completed native turns, two cancelled turns, 72 assertions, no assertion
failures. Those counts concern transport probes, not product acceptance tests.

| Native operation | Observed result on both versions |
| --- | --- |
| Initial prompt, then three separate complete synthetic inbox bodies after completed turns | Each yielded a nonempty assistant reply; all three exact input bodies persisted once on the active native chain |
| Create a second session in the same owner process | Distinct native identity; its prompt did not enter the old session |
| Load the first session | History replay contained all complete inbox bodies, with native message IDs; subsequent prompt stayed in that session |
| Cancel after a streamed assistant chunk | Prompt ended with `stopReason: cancelled`; partial assistant text remained in native history |
| Prompt the other session after cancellation | Completed normally, without appearing in the cancelled conversation |
| Terminate the sole owner process, start its replacement and load the exact session | Load and a further prompt succeeded; earlier inbox bodies remained present |

The response includes native `cognition.ai/userMessageId`; `session/load` replay
supplies the same `cognition.ai/clientMessageId` with each user message and its
full text. The private database independently held those same complete bodies.
Thus a complete receipt can be checked against native evidence; success of the
outgoing RPC alone need not become the receipt definition.

**Completion trap:** in these probes, both ordinary and cancelled assistant rows
had `metadata.finish_reason: null`. Both had text and a native message UUID. A
collector that treats the final assistant database row as a finished answer would
misclassify the cancelled output. The ACP host must retain the corresponding
turn result and publish only a completed answer. If the host crashes before it
has durable completion evidence, recovered text alone remains unconfirmed.
Incoming-result receipt and completion of the coordinator's subsequent answer
are separate facts: a cancellation does not erase a user message already present
in native history.

Private evidence (no real user conversations):

- [Probe script](/Users/gabrielvoicu/.consensflow/tmp/devin-native-research/main/probe-lifecycle.py)
- [Latest binary results](/Users/gabrielvoicu/.consensflow/tmp/devin-native-research/main/probe-1789239064676774000/summary.json)
- [Installed binary results](/Users/gabrielvoicu/.consensflow/tmp/devin-native-research/main/probe-1789239064676198000/summary.json)
- [Hashes, counts and process cleanup](/Users/gabrielvoicu/.consensflow/tmp/devin-native-research/main/verification.json)

Each result directory also contains `wire.json`, the private `sessions.db`, mock
request evidence and the exact sandbox profile. All probe-owned processes were
terminated. An independent review identified mutable process labels in the first
probe script; reader identities were bound to their own process before the final
two passing runs linked above. No production inbox claims, role launches or UI
actions were tested.
The mock emits a small predetermined text response; it does not verify model
reasoning, tools, permissions, multimodal content, long multipart answers or
real authentication. Draft fencing and crash recovery still need product tests.

## Concrete implementation boundary

The proposed transport is one **ConsensFlow-owned `devin acp` connection per
pane**, used for workers, leads, PMs and advisors. The visible surface would be a
ConsensFlow conversation pane with messages, an editor, tool/permission cards,
cancel, native command discovery and model/mode selection. It uses the native
Devin engine; there is no claim that the stock Devin terminal can be retained
with equivalent delivery guarantees.

Reuse `hosts/lib/receiver.js` for polling and exact selected-session checks,
`src/inbox.js` and `hosts/lib/inbox.js` for scoped claims and receipts, and the
existing PM/Lead ownership, Results history and pending counts. The added ACP
adapter owns process lifecycle, active native identity, serialized prompts,
streamed updates and durable completion evidence. Normal prompts, explicit
`cf run`/`cf say` requests and inbox bodies enter that same owner connection.
No second delivery queue or terminal-injection path is needed.

A session change first stops collection and retires the old receiver lease.
Only after cancellation/selection is settled does the host register the chosen
new or loaded native identity. Old replies stay recorded under their originating
session and cannot silently become results for the new conversation. An unused
old native session need not be deleted to prevent delivery: the exact receiver
lease and host selection own that decision. A replacement process is appropriate
for recovery from a stuck or crashed connection, not for every incoming reply.

Bundle the adapter with ConsensFlow and prepare role/config/runtime files under
the configured ConsensFlow home. Use the installed native executable and its
supported authentication flow; no global plugin installation is needed. Real
credential reuse, tool permissions and storage locations must be verified before
implementation is called usable. Advisors use the existing advice-only policy;
only PMs may write or revise specifications. Role policy is not inferred merely
from Devin's `Ask` mode.

The existing four harness receivers remain on the same shared inbox architecture.
Adding ACP for Devin is a native transport adapter choice, not another automatic
sender design. A migration of all harness UIs to ACP is outside this task.

## Research boundaries and remaining uncertainty

All native processes used new private HOME/XDG/data/cache/tmp directories and a macOS sandbox denying non-loopback network and writes outside that probe directory except `/dev`. No credentials were copied. All owned processes were terminated after the probe.

Absence of an endpoint from documentation and help is **not proof that no internal endpoint exists**. The supported public surfaces and tested hook/parser/session-lock behavior do establish why the proposed small hook installer cannot promise the same complete behavior today. If a future release exposes a supported terminal transport override, active-session callback or idle event, the common receiver can use it without changing the inbox design.
