# Kimi Code reasoning effort

Reviewed 2026-09-10 against current Moonshot documentation and official `MoonshotAI/kimi-code` source at commit `221d112f3a3cccd9724ef12e5883c5f08cdf8b43`. That snapshot declares Kimi Code CLI version `0.42.0`. Research only: no live model requests, credentials, production changes, or user-config writes. [Version source](https://github.com/MoonshotAI/kimi-code/blob/221d112f3a3cccd9724ef12e5883c5f08cdf8b43/apps/kimi-code/package.json)

**K3 has Low, High and Max reasoning levels. K2.7 Code and K2.7 Code Highspeed expose thinking mode, without those named levels. ConsensFlow's blanket “Default” label hides that distinction.**

## Model capabilities and defaults

| Native model alias | Supported choice | Official coding-service default |
| --- | --- | --- |
| `kimi-code/k3` | `low`, `high`, `max` | `high` |
| `kimi-code/kimi-for-coding` | Thinking on; no documented named effort levels | Thinking on |
| `kimi-code/kimi-for-coding-highspeed` | Thinking on; no documented named effort levels | Thinking on |

Kimi Code's model table explicitly distinguishes the three K3 levels from K2.7's boolean thinking. K2.7 Highspeed has the same coding ability with faster inference. The coding gateway maps some other clients' efforts (`xhigh` to `max`, `medium` to `high`), but a native catalog should use the canonical K3 levels. Disabling thinking on these coding routes can route to K2.6, so “Off” must not be advertised as another K3/K2.7 variant. [Official model configuration](https://www.kimi.com/code/docs/en/kimi-code/models.html)

The upstream documentation's `high` default is not proof of a particular local session's effective effort. Kimi's configuration supports global thinking preferences and per-model `default_effort`; model metadata can be refreshed. The main agent prioritizes configured global effort over a model default. Only the managed K3 family currently declares `support_efforts`. [Configuration reference](https://www.kimi.com/code/docs/en/kimi-code-cli/configuration/config-files.html)

The implementing parent independently read the installed 0.42.0 configuration and reported `k3` with `support_efforts = [low, high, max]` and `default_effort = max`; the two K2.7 aliases had thinking capabilities but no named levels. Thus an explicit Max preset would be a deliberate catalog choice matching that local metadata, not a claim that every Kimi installation defaults to Max.

## Safe selection channels

The CLI reference supports `--model`, but lists no `--effort`, `--thinking`, or per-launch config-file argument. `--agent-file` only applies at fresh session creation and cannot accompany resume. [CLI reference](https://www.kimi.com/code/docs/en/kimi-code-cli/reference/kimi-command)

Agent Markdown frontmatter configures identity, instructions and tool boundaries; it has no supported model or thinking-effort field. Unknown fields are ignored. Adding `effort: max` to an agent file would therefore not pin effort. [Agent format](https://www.kimi.com/code/docs/en/kimi-code-cli/customization/agents.html)

`KIMI_MODEL_THINKING_EFFORT` is documented separately as a runtime override for the `kimi` provider. It is independent of the environment-model synthesis channel, so it does not require `KIMI_MODEL_NAME` or an API key and can preserve the user's existing managed OAuth model. It bypasses declared effort validation; ConsensFlow must therefore validate choices itself. [Environment reference](https://www.kimi.com/code/docs/en/kimi-code-cli/configuration/env-vars.html)

There is an important boundary: the implementation ignores this forced effort when the session's base thinking state is `off`. It cannot reliably turn K2.7 thinking on over a user/session Off preference. For always-thinking K3 metadata, an Off base resolves back to an allowed model effort, after which the override applies. [Forced-effort source](https://github.com/MoonshotAI/kimi-code/blob/221d112f3a3cccd9724ef12e5883c5f08cdf8b43/packages/agent-core-v2/src/llm-adapter/model/thinking.ts#L60), [Base-effort resolution](https://github.com/MoonshotAI/kimi-code/blob/221d112f3a3cccd9724ef12e5883c5f08cdf8b43/packages/agent-core-v2/src/human/llm/thinking.ts#L195)

The override is stored in a separate in-memory `forcedEffort` field and stripped from config writes. The effective profile state and status report the forced value when it applies. This is suitable for a child-process environment; it is not a persisted per-session setting and must be supplied again when restarting that agent process. [Environment binding and writeback exclusion](https://github.com/MoonshotAI/kimi-code/blob/221d112f3a3cccd9724ef12e5883c5f08cdf8b43/packages/agent-core-v2/src/app/kosongConfig/configSection.ts#L310), [Effective status resolution](https://github.com/MoonshotAI/kimi-code/blob/221d112f3a3cccd9724ef12e5883c5f08cdf8b43/packages/agent-core-v2/src/agent/profile/profileService.ts#L672)

`KIMI_CODE_HOME` changes the whole data root, including configuration and session-related state. It is not a single-field override. The project-local config implementation exposes additional workspace directories, not thinking configuration. Neither is a small, equivalent replacement for a missing effort flag. [Data-root resolution](https://github.com/MoonshotAI/kimi-code/blob/221d112f3a3cccd9724ef12e5883c5f08cdf8b43/apps/kimi-code/src/utils/paths.ts#L34), [Project-local config interface](https://github.com/MoonshotAI/kimi-code/blob/221d112f3a3cccd9724ef12e5883c5f08cdf8b43/packages/agent-core-v2/src/app/projectLocalConfig/projectLocalConfig.ts)

## Minimal integration recommendation

1. Stop coercing all Kimi profiles to `default`. Expose the supported K3 options and persist the selected effort in the saved agent's execution settings. A missing selection should say **Kimi setting**, because it inherits configuration/session state rather than a fixed model-level default.
2. For a reviewed native K3 alias, validate `low`, `high`, or `max` and pass it through the individual Kimi child process's `KIMI_MODEL_THINKING_EFFORT`. Preserve its existing model alias, OAuth authentication, user home and session binding. Verify fresh and resumed invocation paths independently.
3. Do not manufacture Low/High/Max K2.7 presets. Prefer an inherited **Kimi setting** label, with supported thinking capability described separately. If offering an explicit **Thinking on** choice, verify it is enforceable when config or resumed state is Off; fail clearly instead of promising that the environment override enables it.
4. A real Kimi binary test against an isolated loopback model endpoint can prove emitted effort and resumed behavior without billed requests or keys. Include the thinking-disabled control; testing only an enabled fixture would miss the override limitation. This research did not run that test.

The catalog can show the configured requested effort, but should not claim that arbitrary manual model/thinking changes inside a running native session remain synchronized with its saved card.

## Implementation verification

The user subsequently requested removal of both K2.7 variants; only K3 remains
in the ConsensFlow Kimi library. Actual installed 0.42.0 binary verified against
a loopback model server with isolated HOME/KIMI_CODE_HOME and dummy credentials:
Low, High, Max and inherited High each passed fresh and exact-session resume
(8/8, exit 0). The production ConsensFlow invocation builder supplied the child
environment. Config thinking.enabled=false plus K3 always_thinking metadata was
included; the selected named effort still reached thinking.effort exactly.
Every temporary config remained byte-identical. No paid provider requests.
Script: /tmp/cf-kimi-effort-probe.mjs; results: /tmp/cf-kimi-native-effort.log.
