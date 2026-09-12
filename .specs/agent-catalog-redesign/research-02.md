# Model recommendations and image route

Researched 2026-09-10. This is a dated research snapshot, not a live provider or
account-availability guarantee. Application source is still alpha.45.

## Artificial Analysis

The current [Intelligence Index methodology](https://artificialanalysis.ai/methodology/intelligence-benchmarking)
uses version 4.3. It combines coding, reasoning and professional-work evaluations.
Do not compare its scores directly with earlier release articles using a different
index version. A model benchmark does not establish ConsensFlow lead/PM reliability.

| Release and source | Low | Medium | High | Xhigh | Max |
|---|---:|---:|---:|---:|---:|
| [GPT-6 Astra](https://artificialanalysis.ai/models/releases/gpt-6-astra) | 46 | 50 | 51 | 53 | 53 |
| [Claude Fable 5.1, default fallback](https://artificialanalysis.ai/models/releases/claude-fable-5-1) | 47 | 49 | 51 | 53 | 53 |

These are aggregate index scores, not coding percentages. Fable results explicitly
include its default fallback policy. Latency, output speed and cost per task are
different measurements; Low must not promise universally faster tokens per second.

[Sol's release page](https://artificialanalysis.ai/models/releases/gpt-5-6-sol)
also exposes an estimate qualifier. Do not silently present every displayed value
as an independent completed evaluation. Numeric rankings and prices will stay out
of the catalog UI; they age quickly and do not describe subscription charges.

Editorial recommendation: prefer Astra/Fable medium and above for lead and PM
suggestions; retain Sol/Opus high-and-above as alternatives, plus Opus medium for
planning. These are conservative product recommendations based on model family,
effort and task suitability, not claims that AA tested the ConsensFlow roles.

[Anthropic's Fable 5.1 announcement](https://www.anthropic.com/claude-fable-and-mythos-5-1)
describes coding, knowledge work and long tasks, with low/medium improving its
cost tradeoff. Model capability does not establish subscription eligibility in Pi.

## Descriptions and provider identity

The public [OpenRouter model catalog](https://openrouter.ai/api/v1/models) was
read on the same date. Its provider descriptions support conservative task hints
for coding, codebase analysis and agent workflows. It is a source of provider
metadata, not independent benchmark proof. Current IDs include the catalog's
Gemini 3.8 Flash, DeepSeek V4 snapshots, Grok 4.6, Qwen 3.8 27B, MiniMax M3,
GLM 5.3, Kimi K3, Nemotron 3 Ultra, Laguna S 2.1 and Muse Spark 1.3.

Do not upgrade unrelated models just because this endpoint lists a newer version.
Do not merge contributor/free variants or dated snapshots into a different model.
Billing routes (OpenAI Codex subscription, Anthropic API, OpenRouter, OpenCode
Go/Zen) remain distinct from harness identity and model grouping.

## Pygmalion

Read actual `hosts/lib/image-run.js` before documentation: it delegates to
`codex exec`, strips OPENAI_API_KEY, and asks Codex to use its image tool. Neither
the instruction nor the child arguments select the image model. The agent's
configured model is not used to select generation. IMAGE_BACKEND is a hard-coded
gpt-image-2 label, not observed generation metadata.

Official [Sunburst model documentation](https://developers.openai.com/api/docs/models/gpt-image-2.5-sunburst)
and [image guide](https://developers.openai.com/api/docs/guides/image-generation)
confirm GPT Image 2.5 Sunburst and Flare. Sunburst targets precise editing; Flare
targets everyday generation. The Image API selects them directly; Responses
selects them on the image-generation tool. Quality settings are not reasoning
effort. These API facts do not establish selection through Codex subscription.

The [Codex configuration reference](https://developers.openai.com/codex/config-reference)
does not document an image-model selector. The installed-version upstream
[image tool instructions](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/ext/image-generation/imagegen_description.md)
and [image generation result type](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/ext/items/src/image_generation.rs)
do not establish a selectable or returned Sunburst/Flare identity either.

Gabriel confirmed the existing Codex login must remain. Therefore present
Pygmalion as Codex Images, preserve its route, and remove unsupported exact-model
claims from current display/result metadata. This is not a verified 2.5 upgrade.
Do not rewrite historical result files or introduce an API client/key flow.

## Coherence risks found in source

- `hosts/lib/presets.js` owns the canonical 87 entries; `src/catalog.js` derives
  the manager catalog. A second hand-written UI model list previously drifted.
- `src/ui.js` currently hides entries whose names are taken; added entries must
  instead remain visible. Preset identity and name collision are different cases.
- `src/roster.js` preserves preset provenance after edits. Recommendations must
  derive from actual harness/model/effort, not a stale preset name.
- Missing Pi thinking-map keys are not the same as explicit null. A prior comment
  treating omitted medium/high as unsupported was too strong; verify transport.
- OpenCode interactive launch drops effort; the researched launch fix belongs in
  the same implementation as the new low/medium presets.
- Existing user descriptions and saved model/provider choices must not be
  overwritten merely by browsing or installing an updated catalog.

Advisor and Context7 tools were searched for but are unavailable in this session.
No live model invocation or billed image generation was performed for this forge.

## Display ordering correction, 2026-09-10

Gabriel explicitly requested descending effort and family/tier ordering and then
confirmed family blocks (Claude, GPT, Gemini first). This UI policy is not a
cross-provider leaderboard. Tier order follows his Claude/GPT choices; Gemini
Pro/Flash, DeepSeek V4 Pro/Flash, Qwen Max/27B and GLM base/Flash are curated
product-tier conventions, not guaranteed cross-generation benchmark comparisons.
Provider aliases and snapshots remain distinct, with numeric label tie-breaks.

Primary sources checked for current model positioning:
- [Google model overview](https://ai.google.dev/gemini-api/docs/models) and
  [Gemini 3.8 Flash](https://ai.google.dev/gemini-api/docs/models/gemini-3.8-flash).
- [DeepSeek V4 model overview](https://deepseek.com/en/news/v4-preview/).
- [Qwen model descriptions](https://chat.qwen.ai/legal-agreement/models).
- [GLM 5.3 overview](https://docs.z.ai/guides/llm/glm-5.3).

Do not infer a live provider's alias routing or upgrade models from this order.
DeepSeek published [V4.1 Flash on the same day](https://deepseek.com/news/deepseek-v4-1-flash/),
illustrating why suffix names alone are not a future-proof intelligence score.
This correction preserves the existing catalog and agent model IDs; introducing
new model versions requires a separate catalog update.
