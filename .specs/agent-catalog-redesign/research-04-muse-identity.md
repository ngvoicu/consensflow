# Muse Spark 1.3 identity and Contributor routes

Reviewed 2026-09-10. Read-only source and documentation research; no model calls, credentials, or user-state access.

## Findings

Contributor is presented as a commercial/data-use tier of Muse Spark 1.3, not a separately named capability release. OpenRouter describes its Contributor offering as the discounted tier of Meta's multimodal reasoning model and identifies Meta as the serving provider. Its standard and Contributor listings use the same release date and context size. This supports one model family card with separate route choices; the available public evidence does not independently attest to byte-identical weights or identical serving configurations. [OpenRouter Contributor](https://openrouter.ai/meta/muse-spark-1.3-contributor), [OpenRouter standard](https://openrouter.ai/meta/muse-spark-1.3)

| ConsensFlow route | Provider's documented identifier | Difference to retain in the row |
| --- | --- | --- |
| OpenRouter standard | `meta/muse-spark-1.3` | Paid API route; standard tier |
| OpenCode Go Contributor | `muse-spark-1.3-contributor` | Go subscription; Contributor data-use terms |
| OpenCode Zen Contributor Free | `muse-spark-1.3-contributor-free` | Free offering for a limited time; Contributor data-use terms |

Go explicitly lists the Contributor identifier at its Responses endpoint and marks its data as used for training, without zero data retention. Go is a subscription with usage limits. Zen explicitly lists the free identifier at its Responses endpoint; free access is limited-time, and prompts/completions may be used to train future Meta models. These are material route differences even when the model is grouped together. [Go documentation](https://opencode.ai/docs/go/#endpoints), [Go privacy](https://opencode.ai/docs/go/#privacy), [Zen documentation](https://opencode.ai/docs/zen/#endpoints), [Zen pricing](https://opencode.ai/docs/zen/#pricing), [Zen privacy](https://opencode.ai/docs/zen/#privacy)

Artificial Analysis publishes a separate Muse Spark 1.3 **xhigh** page and a **max** page. Their displayed Intelligence Index values differ (45 and 48 respectively at review time); therefore, do not fill an Xhigh row with the Max record. AA's performance documentation describes figures as measurements of the first-party API, or a median across providers where no first-party API exists. A shared model score must not imply that OpenCode Go, Zen, or the complete ConsensFlow agent was separately benchmarked. [AA Xhigh](https://artificialanalysis.ai/models/muse-spark-1-3-xhigh), [AA Max](https://artificialanalysis.ai/models/muse-spark-1-3)

The Meta pricing documentation linked by OpenCode required login in this browsing environment. Meta's public launch announcement names one Muse Spark 1.3 release but does not discuss Contributor identity. Do not claim a stronger first-party checkpoint guarantee than the accessible sources establish. [Meta pricing, login required](https://dev.meta.ai/docs/pricing-rate-limits), [Meta launch](https://research.meta.ai/blog/introducing-muse-spark-1-3)

## Recommendation for this change

1. **Group all five existing Muse Xhigh presets under “Muse Spark 1.3 · Xhigh.”** Treat the Contributor/free suffixes as explicitly reviewed route aliases for this model only. This is an inference from the provider's tier descriptions, not a generic rule for stripping suffixes. Preserve the exact execution model string and separate Add/Remove identities.
2. Retain clear row labels such as “OpenRouter · API”, “OpenCode Go · Contributor”, and “OpenCode Zen · Contributor · Free”. Keep a concise Contributor data-use explanation on those rows: “Prompts and replies may train Meta models.” Do not imply that Contributor routes are equivalent in billing, privacy, quota, speed, or reliability.
3. **A shared AA model score is reasonable when the exact model generation and reasoning match.** Reuse only the reviewed `muse-spark-1-3-xhigh` record for these Xhigh choices, preserve AA provenance, and label the scores as model benchmarks. Do not copy Max scores, invent missing metrics, or claim route-level benchmark validation. If the required AA record is absent from the authenticated cache, show the coverage explanation instead.

## Current source seam

`hosts/lib/presets.js` currently keeps `muse-spark-1.3`, `muse-spark-1.3-contributor`, and `muse-spark-1.3-contributor-free` as three profile keys/labels. `agentProfile` strips provider paths only after an exact curated harness/model match; retain that boundary and explicitly canonicalize these known Muse route aliases there. The five presets are Eos/Logi (standard), Urania/Odrerir (Go Contributor), and Gefjon (Zen free Contributor).

`hosts/lib/benchmarks.js` currently maps only the standard profile key. Once canonicalized, the existing model/effort lookup can attach the same Xhigh AA record without changing launch configuration. The existing provider route labels are too broad to communicate Contributor/free terms after the shared heading stops displaying those suffixes.
