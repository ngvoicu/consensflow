# Devin stock TUI: remaining wake surfaces

Date: 2026-09-12. Research only; no product implementation, installed-app change, global harness configuration, existing credentials, or real model call. This supplements [the earlier native transport investigation](./research-native-transport.md), concentrating on keeping Devin's own terminal UI.

## Decision

**The viable native-TUI path is lifecycle-based collection with an honest idle limitation.** A private receiver can fetch at `UserPromptSubmit` and `Stop`, including subsequent replies. Waiting inside a Stop hook can collect replies from known outstanding workers without empty inference calls. However, a completed turn has no demonstrated independent wake callback. Replies arriving after complete idleness must remain visibly pending until another native event, unless a new supported or independently verified wake mechanism becomes available.

The additional investigation did not uncover a smaller complete wake mechanism. MCP sampling is explicitly rejected by both tested Devin versions. The PTY bridge is implemented by a separate remote toolbox executable and transports terminal operations; setting its environment variables on the ordinary TUI did not start it. Neither result rests only on missing documentation.

The native-TUI requirement should remain explicit. These findings do not authorize replacing its UI, silently weakening automatic delivery promises, or reintroducing unfenced terminal input.

## Native MCP probe

The stock native agent engine was tested using installed **3000.6.14 (18033302)** and privately downloaded official stable **3000.10.21 (611c1cba)**. A fresh configuration, workspace and database lived below `~/.consensflow/tmp/devin-tui-research/docs/`. A private stdio MCP server exposed a prompt, tool and resource. Only a loopback mock answered vendor requests; the child process sandbox denied external networking and writes outside the private directory.

The documented MCP prompt route `/mcp__inboxprobe__inbox` caused native discovery and one completed synthetic reply. This establishes a working connection before probing idle notifications. Devin documents MCP tools and prompts, with prompts exposed as slash commands. [MCP overview](https://docs.devin.ai/cli/extensibility/mcp/overview)

The native MCP client sent, in both versions:

```json
{
  "protocolVersion": "2025-11-25",
  "capabilities": {},
  "clientInfo": { "name": "rmcp", "version": "3.1.0" }
}
```

After the synthetic prompt ended, the server sent the probes below. The mock's inference-request count remained **one** through all of them.

| Probe | Observed result in both versions | What this establishes |
| --- | --- | --- |
| `notifications/prompts/list_changed` | Client requested `prompts/list` again | Discovery updates work; this did not execute a prompt |
| Tools/resource list-change notifications | No model request or result body entered history | No automatic receiver behavior in this path |
| `notifications/message`, with an inbox marker | No model request or marker in history | Logging is not a demonstrated conversation-input channel |
| `sampling/createMessage` | JSON-RPC error `-32601`, message `sampling/createMessage` | Native client explicitly does not implement this request |
| `notifications/resources/updated` | No resource read/subscription, model request or marker | No spontaneous resource subscription/wake was observed |
| `elicitation/create` | `{ "action": "decline" }` | No wake in this basic ACP-host probe; native TUI-specific elicitation UI was not tested |

Important limits:

- This tested the released **native MCP engine through ACP**, not the stock TUI's elicitation UI. The sampling rejection is concrete; elicitation's exact frontend behavior remains a narrower unknown.
- The MCP client did not advertise sampling. Sending that request was a negative compatibility probe, not a proposed production protocol. MCP requires supporting clients to advertise the capability. Sampling is a client-controlled model request, not a guarantee that the main visible conversation receives a message. [MCP sampling specification](https://modelcontextprotocol.io/specification/2025-11-25/client/sampling)
- Resource notifications normally follow a client subscription and identify a URI to re-read. Our client never subscribed. The negative probe does not prove every possible model-requested resource read is unsupported; it proves this advertised resource did not independently establish a wake path. [MCP resources specification](https://modelcontextprotocol.io/specification/2025-11-25/server/resources)
- Native history rows were searched only to establish absence of the notification markers. Raw database rows include revisions; they were not used to count successful deliveries or substitute for canonical-chain receipt verification.
- Earlier preparation runs that never connected the MCP server are excluded. An empty mocked `GetCliTeamSettings` response fabricated `allow_mcp_servers=false`. The successful runs returned protobuf field 1 boolean true (`08 01`) from the **private mock only**. No real organization's settings were read or changed.

Reproduction artifacts:

```text
~/.consensflow/tmp/devin-tui-research/docs/probe-mcp.py
~/.consensflow/tmp/devin-tui-research/docs/mcp-server.py
~/.consensflow/tmp/devin-tui-research/docs/probe-1789239850736129000/  # installed
~/.consensflow/tmp/devin-tui-research/docs/probe-1789239850741908000/  # latest
```

Each successful run retains `mcp-wire.jsonl`, `wire.json`, `summary.json`, its sandbox profile, and synthetic mock requests. Both native processes and their MCP children were terminated.

## MCP tasks and native background work

MCP tasks wrap an originating request in a durable task identity, with status polling and deferred result retrieval. They do not define an unsolicited main-conversation message operation. A terminal would still need to initiate and retain a task and consume its completion. The specification currently labels tasks experimental. No task-augmented native tool call was tested here; it would be incorrect to claim tasks are universally unsupported from this probe. [MCP tasks specification](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/tasks)

Devin's `/loop <prompt>` runs a prompt and repeatedly reviews its diff. It is not an interval scheduler or inbox wait command. [Commands reference](https://docs.devin.ai/cli/reference/commands)

Native subagents are real Devin agent sessions. Foreground work blocks the parent's tool call; background work allows the parent to continue. The subagent documentation warns that moving a foreground subagent into the background does not feed its result into the parent's current pipeline, although completion is notified. This is not a generic registration mechanism for ConsensFlow worker completions. [Native subagents](https://docs.devin.ai/cli/subagents)

Stable 3000.10.21 keeps a turn active while background native subagents run and makes normal interruption cancel them. Consequently, a subagent waiting for the inbox is not proof of a truly idle receiver and adds an agent invocation. The shell's background-command display also has polling/termination behavior; no documented conversion from arbitrary shell exit to a fresh main-conversation turn was found. Actual background-shell idle completion was not probed here. [Stable changelog](https://docs.devin.ai/cli/changelog/stable)

## Outposts and the PTY bridge

Outposts serves cloud sessions on user-controlled infrastructure. `devin worker start` claims sessions, downloads `devin-remote`, and connects the remote toolbox to the Outpost gateway. Its session identifiers and ownership contract concern these cloud sessions; the docs do not provide an attachment operation for an existing local Devin TUI conversation. [Outposts overview](https://docs.devin.ai/cloud/outposts/overview)

The explicit spawn contract is `devin-remote serve` with the gateway URL, connect token, session ID and per-session state directory. The separate operator-message API changes a session banner; it is not a conversation prompt. [Outposts reference](https://docs.devin.ai/cloud/outposts/reference)

The official remote executable was downloaded privately from the distribution described in that reference and checksum-verified:

```text
Version/commit: 412f0e9b19
URL: https://static.devin.ai/devin-rs/remote/devin-remote_412f0e9b19_macos_arm64
Bytes: 60519328
SHA256: b2b54737eb0b68b88f88c035ba5ad74e6b33b4c894a6622114ccec9d81f86636
Private file: ~/.consensflow/tmp/devin-tui-research/docs/devin-remote-412f0e9b19
```

Native help identifies this as an RPC/toolbox server. `serve --port` defaults to 9876, or an ephemeral port in Outpost mode. The CLI worker flag describes `--pty-bridge-port` / `DEVIN_PTY_BRIDGE_PORT` as the bridge port **for the remote**.

Targeted binary inspection found the actual implementation module `remote/src/pty_bridge.rs`, remote RPC method `enable_pty_bridge`, and concrete terminal message vocabulary:

```text
write: pty_id + content
resize
terminate
subscribe
output
exited: pty_id + exit_code
pty_created / pty_removed
subscribed / subscription_confirmed
```

The findings are stronger than a search for a hypothetical environment variable: these are native parser/error/output strings for an implemented terminal bridge. They do **not** establish an atomic local-Devin session/draft snapshot or enqueue operation. No such semantic fields were found. This remains an inference from targeted binary evidence, not a claim that every remote method was exhaustively reverse-engineered.

A private launch of the remote reached its startup sequence but was denied wildcard bind/listen by the loopback-only sandbox. That attempt is **not** counted as a successful remote RPC or WebSocket probe. The sandbox was not relaxed to expose its broad toolbox API, and both attempts terminated. Startup's attempted `/var/log/devin` creation was denied; logs and helper files stayed inside the private research root. Retained evidence: `remote-source.json`, `remote-help-sandbox/`, `bridge-strings.txt`, `remote-bridge-strings.txt`, `remote-rpc-strings.txt`, and `bridge-1789240105424735000/`.

The parent investigation separately tested ordinary stock TUI launch with `DEVIN_PTY_BRIDGE_PORT`, `DEVIN_REMOTE_STATE_DIR` and a dummy remote token. The specified port did not listen and the remote state directory was not created. Evidence: `~/.consensflow/tmp/devin-tui-research/main/probe-1789240049101869000/`.

Thus adding `devin-remote` would introduce a separate remote toolbox and raw terminal transport without a demonstrated solution to selected-conversation or unsent-draft ownership. A version guard would identify a supported binary; it would not supply the missing semantic contract.

## Native-TUI delivery contract to consider

The parent stock-TUI tests establish a useful path through repeated Stop hooks: wait without another inference request, release a full result body through a blocking decision, and continue in the same native conversation while preserving a draft. Later results after complete idleness are collected at the next `UserPromptSubmit`. The native cancellation test also establishes a material limitation: cancellation is requested promptly, but a held Stop must return before cancellation completes; a returned block body can enter history after cancellation without starting another inference. These are private mock results, not installed ConsensFlow acceptance.

Therefore a shippable native integration should distinguish:

1. **Result waiting:** persist every result independently, including second and later replies; leave it pending until actual native receipt evidence exists.
2. **Collection during a turn:** fetch at native lifecycle boundaries; wait only for known outstanding work, with a bounded cancellation-aware policy. A model-generated answer is not required merely to poll.
3. **Result arriving after idle:** show it as pending and collect on the next native prompt. This must not be described as immediate automatic wake.
4. **New/resume/cancel boundaries:** use the hook's native session and prompt identity; recheck ownership before releasing a body, preserve uncertain evidence, and never mark a claim or requested read as receipt.

The exact permissible wait duration and cancellation gate still need product-level tests; an endless Stop wait would obstruct ordinary terminal interaction. Those tests belong to the parent investigation. This report recommends no additional runtime or remote server and makes no implementation-completion claim.
