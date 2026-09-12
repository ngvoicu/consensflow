# Artificial Analysis score pills

Research date: 2026-09-10. Scope: primary Artificial Analysis (AA) sources and the current catalog; no account creation, authenticated API calls, paid calls, application changes, or agent-data changes.

## Recommendation

The expanded request is to expose every useful, available AA benchmark as a pill and support sorting by it. The priority table below defines that set. These three are the leading general-purpose measures, alongside and visually separate from category pills:

| Pill | Meaning | Direction |
| --- | --- | --- |
| Intelligence 53 | AA Intelligence Index v4.3, a composite score on a 0–100 scale | Higher is better |
| Terminal coding 59.1% | AA's Terminal-Bench v4.0 result | Higher is better |
| Hallucinations 51.3% | AA-Omniscience hallucination rate, with the definition available on focus/hover | Lower is better |

These examples describe **GPT-6 Astra at max**, not every Astra configuration. The overall index combines ten evaluations. The coding score gives a more specific view of terminal tasks; AA evaluates 66 tasks with mini-SWE-agent v2.4.6, averaging first-attempt success over three repeats. The score is evidence about the model under AA's evaluation setup, not an evaluation of ConsensFlow, Codex, Pi, OpenCode, or Claude Code. [Intelligence Index](https://artificialanalysis.ai/evaluations/artificial-analysis-intelligence-index), [Terminal-Bench v4.0](https://artificialanalysis.ai/evaluations/terminalbench-v4-0), [methodology](https://artificialanalysis.ai/methodology/intelligence-benchmarking).

### Hallucinations need a precise explanation

AA defines the rate as `incorrect / (incorrect + partial answers + not attempted)`. Correct responses are excluded from this denominator. It measures whether the model guesses incorrectly on questions it cannot answer correctly, instead of abstaining. It is **not the percentage of all responses that are wrong**. Abstention can reduce this rate without improving knowledge, so show the corresponding accuracy in the detail text; accuracy uses all questions as its denominator. [AA-Omniscience](https://artificialanalysis.ai/evaluations/omniscience).

Suggested detail: “AA-Omniscience: incorrect answers among questions not answered correctly. Lower is better. Accuracy: …%. Factual knowledge benchmark; not a measured rate of coding mistakes.” This is a product interpretation of that definition. Software-engineering facts are one evaluated domain, but the overall result does not measure how often an agent invents APIs while editing a repository. Do not use green/red absolute thresholds without a reviewed rationale.

### Knowledge-work scores

AA-Briefcase measures completion and quality of professional deliverables such as spreadsheets, presentations, and memos in multi-week business scenarios. Its headline is Elo, combining rubric success, analytical quality, and presentation; it is not a task-success percentage. It can help compare PM-style knowledge work, but it is not a direct test of coordinating ConsensFlow workers. Under the expanded request it is useful when a supported data field is verified. [AA-Briefcase](https://artificialanalysis.ai/evaluations/aa-briefcase).

GDPval-AA v2 similarly evaluates professional work and uses an Elo scale anchored to human deliverables at 1000. Keep its name and units distinct from an overall agentic index. The current methodology lists legacy Terminal-Bench v2.1 as still used in the Coding Index. Therefore a free API “Coding Index” must not be relabeled “Terminal-Bench v4.0.” [Methodology](https://artificialanalysis.ai/methodology/intelligence-benchmarking).

## Verified score availability

The public model page’s already-served chart payload contained these values. Rounded here for review; this is a research sample, **not an approved redistributable production snapshot**. All four listed records had `intelligenceIndexIsEstimated: false`. [Public models page](https://artificialanalysis.ai/models).

| AA model/configuration | Intelligence | Terminal v4.0 | Hallucinations | Omniscience accuracy |
| --- | ---: | ---: | ---: | ---: |
| GPT-6 Astra max | 52.8 | 59.1% | 51.3% | 62.6% |
| Claude Fable 5.1 max, default fallback | 53.4 | 52.0% | 72.6% | 67.2% |
| GPT-5.6 Sol max | 47.1 | 39.9% | 92.2% | 59.4% |
| Claude Opus 5 max, adaptive reasoning | 50.7 | 49.0% | 60.8% | 60.9% |

AA publicly lists Astra low **46**, medium **50**, high **51**, xhigh **53**, and max **53** on its release page. It lists six configurations including non-reasoning, with no ultra configuration. [Astra release](https://artificialanalysis.ai/models/releases/gpt-6-astra).

Fable 5.1 low **47**, medium **49**, high **51**, xhigh **53**, and max **53** are separately published, all described with default fallback. [Fable release](https://artificialanalysis.ai/models/releases/claude-fable-5-1).

AA's Terminal v4.0 page reports Astra xhigh **59.6%**, Astra max **59.1%**, and Fable 5.1 xhigh **55.1%**. This illustrates why configuration scores cannot be forced to follow the UI's effort order. [Terminal v4.0](https://artificialanalysis.ai/evaluations/terminalbench-v4-0).

Low/medium terminal and hallucination values were not comprehensively verified in this research. Availability of an Intelligence score does not establish the other two values, and a chart’s default selection is not the complete evaluated model set.

## Supported ingestion path

The current docs are `/data-api/docs`; `/api-reference` contains older endpoint and quota examples. Current access is Free **100 requests/day**, Pro **500**, and Commercial by agreement. Free serves headline indices; Pro exposes individual evaluations. Authentication uses an organization-scoped `x-api-key`, kept out of clients. Lists paginate. Null means unmeasured/not applicable, never zero. The response's `intelligence_index_version` identifies the score version; it contains major.minor, not patch revisions. [Current API documentation](https://artificialanalysis.ai/data-api/docs).

The downloadable **OpenAPI YAML**, linked from those docs, confirms these exact fields:

| Data | Endpoint and field |
| --- | --- |
| Intelligence, Free+ | `/api/v2/language/models/free`: `data[].evaluations.artificial_analysis_intelligence_index` |
| Intelligence, Pro+ | `/api/v2/language/models`: same field |
| Terminal v4.0, Pro+ | `/api/v2/language/models`: `data[].evaluations.terminalbench_v4_0` |
| Hallucinations, Pro+ | Same endpoint: `100 * (1 - evaluations.aa_omniscience_non_hallucination_rate)` |
| Accuracy, Pro+ | Same endpoint: `100 * evaluations.aa_omniscience_accuracy` |
| Direct hallucination rate, Pro+ | `/api/v2/language/models/{slug}`: `data.aa_omniscience_breakdown.total.hallucination_rate`, multiplied by 100 |

The non-hallucination field is explicitly defined as the complement of the hallucination rate. The schema has stable identity fields (`id`, `slug`, `name`) and optional `openrouter_api_id`; it does **not** document a dedicated reasoning-effort field or Intelligence estimate flag. The list sample omits Terminal v4.0, but the actual downloadable schema declares it. Live response coverage remains unverified because no authenticated call was made. [OpenAPI specification](https://artificialanalysis.ai/api/v2/openapi).

### Distribution matters

AA's product page says Free is for internal use without redistribution and advertises Commercial agreements for customer-facing integrations and redistribution rights. Consequently, “free API plus attribution” does not establish permission to bundle a score dataset in the published app. [Data API access options](https://artificialanalysis.ai/data-api).

The August 19, 2026 Data Platform Terms distinguish internal use from external distribution. Sections 2.3–2.4 permit some charts/brief citations but restrict redistribution of raw structured data and embedding raw data in customer-facing products. Section 3 requires faithful presentation. Treat a broadly distributed bundled JSON snapshot as requiring appropriate redistribution rights, not as implicitly permitted. [Data Platform Terms](https://artificialanalysiscdn.com/legal/ProDataPlatformTerms.pdf).

The site Terms also restrict automated scraping. Public pages were read for this bounded research; do not build a scraping updater. Automated public score extraction stopped once these restrictions were established. [Site Terms](https://artificialanalysis.ai/docs/legal/Terms-of-Use.pdf).

Practical path: retain links/explanations now; for personal/internal scores, use the user's own eligible API access and a local cache. For distributed product scores, obtain rights covering that integration, then fetch/cache the licensed data. A dated snapshot is technically simple but does not remove the distribution question. Do not sign up, subscribe, send inquiries, or publish a scraped dataset as part of the UI change without authorization.

## Catalog matching and presentation rules

Local sources inspected: `src/catalog.js` and `hosts/lib/presets.js`. Their normalized `profile.modelKey` is a useful model identity, but a score key must also include the exact evaluation configuration.

- Astra is one model across native Codex, `openai-codex/…`, and `openrouter/openai/…`; existing presets use low, medium, xhigh, and max. Never copy max scores onto low, medium, or ultra.
- Fable 5.1 normalizes `claude-fable-5-1` and `claude-fable-5.1`. AA's fallback configuration must remain in score metadata/detail text. A model-name match does not prove identical fallback behavior across providers.
- Sol, Terra, and Luna are distinct models. Their existing xhigh entries must not receive max records merely because AA defaults to max on the public chart.
- DeepSeek V4 Pro and its 0813 release must stay distinct unless the actual provider alias has been verified. Current 0813 presets use high while the observed AA chart record was max.
- Preserve Qwen size, Gemini version, reasoning versus non-reasoning, contributor variants, and provider-specific aliases. Unknown/default effort is not a license to select the model's best score.
- `codex-image` deliberately does not name the underlying image model. Do not attach a GPT Image score based on that logical identity.
- Curate an explicit identity/configuration-to-AA-slug mapping. Use exact IDs where verified. Do not fuzzy-match unfamiliar models or parse maximum effort from whichever row sorts first.
- Record source URL, AA model ID/slug, benchmark/version, evaluated configuration, fetched date, and estimate status when available. An absent estimate flag means unknown provenance, not confirmed measured.
- Show a dash with “No published score for this configuration” for unmeasured values; distinguish API access unavailable from no published result. Retain a dated cached score on fetch failure rather than manufacturing a zero or silently changing the configuration.
- Keep family/effort order as the default and as the deterministic tie-breaker when a benchmark sort is selected. Category membership pills remain product recommendations, not labels awarded by AA.

## Remaining verification before numeric integration

1. Establish the intended internal or distributed access path and its rights.
2. With already-authorized credentials, inspect one real response's tier, version, field presence, pagination, and matching model IDs without logging keys.
3. Verify the full catalog's exact effort and fallback matches, missing fields, and estimate handling. The research sample above is deliberately not full coverage.
4. Test conversion, null handling, provenance, wrong-effort rejection, stale cache behavior, and accessible explanations before displaying scores.

## Expanded metric set and implementation mapping

This section supersedes limiting the UI to three scores. Priority describes usefulness for this product, not an AA ranking. Show useful metrics with measured values as individual pills; a user's chosen sort determines ordering. Do not combine these overlapping metrics into a new “total” score.

All field names below are properties of `data[].evaluations` from `/api/v2/language/models`, unless stated otherwise. “Free+” fields are also available from `/language/models/free`; all other declared evaluation fields require Pro+. This is verified schema availability, not confirmation that every model has a non-null result. Fields, tier declarations, and transforms follow the [OpenAPI specification](https://artificialanalysis.ai/api/v2/openapi).

| Priority/use | Pill label | AA field | Unit/transform | Best first | Limitation |
| --- | --- | --- | --- | --- | --- |
| Core | Intelligence | `artificial_analysis_intelligence_index` | Index points, 0–100 | Descending | Composite; retain version. Free+. |
| Core coding | Terminal coding | `terminalbench_v4_0` | `100*x`, % | Descending | Terminal-Bench v4.0 under AA's harness, not native-harness performance. |
| Core | Instruction following | `ifbench` | `100*x`, % | Descending | Verifiable output constraints; not every kind of instruction obedience. |
| Core lead/PM | Long-context reasoning | `aa_lcr` | `100*x`, % | Descending | AA-LCR v1.1 document reasoning, not maximum context capacity. |
| Core factuality | Hallucinations | `aa_omniscience_non_hallucination_rate` | `100*(1-x)`, % | Ascending | Excludes correct answers from the denominator; detail must explain this. |
| Core factuality | Knowledge accuracy | `aa_omniscience_accuracy` | `100*x`, % | Descending | Correct answers over all AA-Omniscience questions. |
| Factuality | Knowledge reliability | `aa_omniscience_index` | Index points, −100 to 100 | Descending | Balances correct answers against hallucinations; abstentions neutral. |
| Coding summary | Coding index | `artificial_analysis_coding_index` | Index points | Descending | Overlaps constituent scores; do not label it Terminal v4.0. Free+. |
| Agent summary | Agentic index | `artificial_analysis_agentic_index` | Index points | Descending | Composite, not direct ConsensFlow coordination quality. Free+. |
| Lead/PM | Professional work | `gdpval_aa_elo` | Elo | Descending | GDPval-AA v2 professional deliverables, not percent success. |
| Scientific coding | Scientific coding | `scicode` | `100*x`, % | Descending | SciCode scientific Python subproblems, not general repository repair. |
| Hard reasoning | Reasoning & knowledge | `hle` | `100*x`, % | Descending | Humanity's Last Exam; advanced academic questions, not project completion. |
| Visual work | Visual reasoning | `mmmu_pro` | `100*x`, % | Descending | MMMU-Pro image understanding; not image generation or UI design quality. |
| Specialist science | Physics reasoning | `critpt` | `100*x`, % | Descending | CritPt research physics, low relevance to everyday app work. |

The labels and priorities are product recommendations. IFBench tests unseen, verifiable output constraints. AA-LCR requires reasoning across long documents. SciCode tests scientific programming. HLE targets difficult academic knowledge/reasoning; MMMU-Pro measures visual understanding. These names must remain available in the pill's accessible explanation so “coding” and “reasoning” do not imply a broader measurement than performed. [IFBench](https://artificialanalysis.ai/evaluations/ifbench), [AA-LCR](https://artificialanalysis.ai/evaluations/artificial-analysis-long-context-reasoning), [SciCode](https://artificialanalysis.ai/evaluations/scicode), [HLE](https://artificialanalysis.ai/evaluations/humanitys-last-exam), [MMMU-Pro](https://artificialanalysis.ai/evaluations/mmmu-pro).

Use **Professional work** for the GDPval Elo pill and **Knowledge work** for a future AA-Briefcase Elo pill; their detail texts supply the benchmark names and scales. This keeps the two distinguishable. [GDPval-AA v2](https://artificialanalysis.ai/evaluations/gdpval-aa), [AA-Briefcase](https://artificialanalysis.ai/evaluations/aa-briefcase).

### Relevant current evaluations whose API mapping is not yet established

The following are published current evaluations and potentially useful, but none has a declared field in the downloaded `Evaluations` or model-detail schema. Do not manufacture snake_case names from website JavaScript properties. Only promote one to an implemented sortable metric when its actual supported API field, scale, and access are established. Null/unavailable placeholders cannot establish that AA has never evaluated the model. [OpenAPI specification](https://artificialanalysis.ai/api/v2/openapi), [AA evaluation directory](https://artificialanalysis.ai/evaluations).

| Priority/use | Suggested pill | Benchmark | Unit / best first | Key limit |
| --- | --- | --- | --- | --- |
| Lead/PM | Knowledge work | AA-Briefcase | Elo / descending | Deliverable quality in business scenarios. |
| Agent workflows | Workflow automation | AutomationBench-AA | % / descending | Partial objective completion, zero credit for guardrail violations. |
| Lead/PM | Document reasoning | GDP.pdf | All-pass % / descending | Professional PDF questions; keep all-pass separate from mean pass. |
| Long tasks | Long-horizon work | APEX-Agents-AA | % / descending | Professional-services tasks, not code-only projects. |
| Data work | Data analysis | AA-AnalystAgent | % / descending | Repeated-run reliability (`pass^5`), not ordinary `pass@1`. |
| Tools/business | Business operations | EnterpriseOps-Gym-AA | % / descending | Stateful MCP business workflows. |
| Operations | Incident diagnosis | ITBench-AA | Precision at full recall / descending | Kubernetes incident diagnosis, not general reliability. |

These scoring distinctions and current benchmark descriptions are documented in the [AA evaluation directory](https://artificialanalysis.ai/evaluations) and [methodology](https://artificialanalysis.ai/methodology/intelligence-benchmarking). API support may be ahead of or behind the public schema; this research agent has not inspected an authenticated response.

### Current, additional, legacy, and out of scope

The current Intelligence Index is v4.3. Its newer constituents include Terminal v4.0, AA-Briefcase, AutomationBench-AA and GDP.pdf. A benchmark being outside that index does not make it obsolete: IFBench, MMMU-Pro and specialized agent evaluations are still separately published. [API versioning](https://artificialanalysis.ai/data-api/docs), [evaluation directory](https://artificialanalysis.ai/evaluations).

- **Legacy versions:** `terminalbench_hard`, `terminalbench_v2_1`, and `tau2_telecom` remain declared API fields. Do not present them as the latest coding/workflow scores or silently substitute them when a current score is missing.
- **Additional, domain-specific:** `tau_banking` is the newer banking benchmark; it is no longer an Intelligence Index constituent but remains separately evaluated. Its domain is too narrow for the principal coding/PM pills.
- **Older academic checks:** `gpqa_diamond` remains declared, but GPQA is listed among legacy evaluations. Preserve it only as an explicitly named optional historical/science metric; do not use “Reasoning” as a generic replacement label.
- **Other specialized fields:** `mlcr_overall` is medical document reasoning. `artificial_analysis_multilingual_index` and `artificial_analysis_openness_index` measure different concerns; they are not coding-task success. Keep them out of this initial useful coding/lead/PM set unless the corresponding need appears.
- **Duplicate representations:** Do not expose both Hallucinations and its exact non-hallucination complement. Likewise show GDPval Elo once, not also `gdpval_aa_normalized` as if it were another independent result.

Field existence is verified in the [OpenAPI specification](https://artificialanalysis.ai/api/v2/openapi); current and legacy status comes from the [methodology's evaluation sections](https://artificialanalysis.ai/methodology/intelligence-benchmarking).

### Speed and cost are separate decision metrics

If included in sorting, keep these separate from benchmark capability pills. They are not estimates of a subscription user's actual bill or elapsed ConsensFlow task time.

| Label | Model response field | Unit | Best first |
| --- | --- | --- | --- |
| Output speed | `performance.median_output_tokens_per_second` | tokens/s | Descending |
| First answer | `performance.median_time_to_first_answer_token_seconds` | seconds | Ascending |
| Response time | `performance.median_end_to_end_response_time_seconds` | seconds | Ascending |
| Benchmark cost/task | `artificial_analysis_intelligence_index_cost.cost_per_task.total_cost` | USD/task | Ascending |

The response-time convention is 500 answer tokens; model-level performance is aggregated across providers. Store the benchmark workload and source context. A first-token latency excludes some thinking time, so first-answer latency is the clearer choice for reasoning models. The API declares nullable metrics; absent cost is not free. [API data conventions](https://artificialanalysis.ai/data-api/docs), [API specification](https://artificialanalysis.ai/api/v2/openapi).

### Sorting contract

1. Sort by the metric's unrounded numeric value with the declared direction; format only after sorting. Preserve valid zero and negative values. Never compare percentage strings or Elo with an index.
2. Missing, inaccessible, and inapplicable values sort after measured values in either direction. Explain the actual reason; never substitute another effort, another benchmark version, or zero.
3. Use the current family/model/effort order and then a stable agent identity as tie-breakers. Choosing one score changes only that list's sorting; it does not change model/effort grouping or the other screen's settings.
4. Group sorting needs an explicit rule. For a Model and reasoning group, use the exact shared configuration score once. For a Harness group containing different scores, preserve harness order and sort rows within it; do not invent an average harness score from duplicate AA model records.
5. Show a sort option only if the integration supports the metric, with availability clear when no current filtered entries have values. Unknown API fields require review rather than automatic exposure under guessed labels.
6. Retain the score's source, benchmark version, evaluated configuration and retrieval date. An explicitly estimated score must say so and must not silently outrank measured entries under an indistinguishable badge.

No custom ConsensFlow benchmark proposal or implementation is included, following the user's later instruction.
