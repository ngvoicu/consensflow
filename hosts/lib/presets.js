import { slugify, stripMention } from "./utils.js";


// Image agents use the Codex login; Codex selects the underlying image model.
// --- Effort ceilings (audited 2026-08-27) --------------------------------
// Every preset names the HIGHEST level its model actually takes, and no preset names a level the
// model does not have. Both facts come from the harnesses' own catalogs, which each publish the
// per-model list: pi's `thinkingLevelMap` (~/.pi/agent/models-store.json, non-null entries) and
// models.dev's `reasoning_options` (opencode's models.json). They were compared across all 287
// models both carry: 281 agree. That agreement is why one name can mean one thing on both
// harnesses — a pi preset and its opencode twin sit at the same level by rule, asserted in
// tests/catalog.test.mjs.
//
// The audit found five presets naming a level their model has never had — `max` on Qwen3.8 27B and
// on Nemotron 3 Ultra, `xhigh` on Kimi K3, and an effort at all on MiniMax M3 and Laguna S 2.1.
// None of them errored: pi maps an unknown level to null and sends nothing, and opencode validates
// nothing at all (a deliberately bogus `--variant` was probed and ran). So the run quietly used the
// model's default while the label promised MAX — the failure mode this comment exists to prevent.
// Three models take no effort parameter at all (MiniMax M3, Laguna S 2.1 free, and Codex Images):
// their presets name no level, because a level nothing honours is worse than a blank one.
//
// DeepSeek (V4.1 Flash and V4 Pro 0813, through OpenRouter on pi and opencode) sits at `max`:
// OpenRouter's own model records (GET /api/v1/models, `reasoning.supported_efforts`, read on
// 2026-09-23) list {max, high, low} for both, with `high` the default, and both harnesses take
// `max`. Until then the rows held `high`, the one level two catalogs agreed on. The OpenCode Go
// rows (every model reached a second way through Go) were dropped on 2026-09-24.
//
// The GPT 5.6 trio through OpenCode (sunna/jord/bil) is deliberately NOT at its ceiling: it holds
// the xhigh tier that the same three models occupy on codex and pi, so the trio means the same
// thing on every harness. A tier ladder is a choice; a level the model lacks is a bug.
//
// --- Fable 5.1 (updated 2026-09-10) --------------------------------------
// Native Claude uses claude-fable-5-1; OpenRouter uses anthropic/claude-fable-5.1.
// Pi 0.85.1 now carries low/medium on the OpenRouter model, the user-chosen route.
// Omitted standard thinking-map keys can use provider defaults; explicit null
// marks unsupported levels. Do not mistake an omitted key for a dropped effort.
// Model/effort source and transport evidence: .specs/agent-catalog-redesign/.
//
// --- Gemini 3.8 Flash (2026-09-03) ---------------------------------------
// nike and sif moved from Gemini 3.7 Flash to 3.8. The ceiling did NOT move and neither did the
// price ($0.75/$3.75 per MTok on both), so `high` stands and the labels' "its ceiling" stays true:
// pi's refreshed thinkingLevelMap gives {low, medium, high} non-null and models.dev gives
// reasoning_options {low, medium, high} — the same three, on the same day. `max` is not a level
// this model has, on either catalog, which is why "put it at maximum effort" lands on `high` here.
// Both ids were LIVE-PROBED at that level on the CLI that will run them — `pi -p --model
// openrouter/google/gemini-3.8-flash:high` and `opencode run --model
// openrouter/google/gemini-3.8-flash --variant high` — because a catalog listing proves the id and
// only a run proves the harness.
//
// Muse Spark 1.3 (eos on pi, logi on opencode) went in the same day, and only on the second
// attempt — which is the finding worth keeping. Both catalogs list `meta/muse-spark-1.3` with
// reasoning_options {minimal, low, medium, high, xhigh}, so its ceiling is `xhigh` and not `max`;
// OpenRouter's /api/v1/models carries it; every source said ship it. The probe came back 403 on
// BOTH harnesses: "This model requires you to complete the following before use: 18+ age
// confirmation." An account attestation is invisible to every catalog there is, and a preset
// written on the catalogs alone would have 403'd on every consult. Once the attestation was
// granted the same two probes answered `ok`, and the rows went in — at `xhigh`, the level both
// catalogs give and both CLIs ran. A catalog listing proves the id; only a run proves the account
// can reach it.
// --- GPT 6 Astra (2026-09-05) --------------------------------------------
// The first GPT 6 row in the catalog, on codex only: `gpt-6-astra` answers there, and
// `gpt-6`, `gpt-6-sol` and `gpt-6-pro` are all refused on a ChatGPT login, as is `gpt-5.6-pro`
// even though codex's own history carries that name. The refusal is worth knowing because it is
// USELESS as evidence: "The '<id>' model is not supported when using Codex with a ChatGPT
// account" comes back identically for a deliberately invented id, so it never distinguishes a
// model that does not exist from one this plan cannot reach. Only an id that ANSWERS proves
// anything.
//
// The effort ladder was probed level by level rather than assumed, and codex — unlike opencode —
// really validates: a bogus `model_reasoning_effort` is a 400, which is what makes each probe
// mean something. `minimal` is refused; low, medium, high, xhigh, max and ultra all answer. So
// the model's ceiling is ULTRA and these two rows deliberately sit below it, the way the GPT 5.6
// OpenCode trio does: a tier ladder is a choice, and asteria/astraeus were asked for as xhigh and
// max. Add an ultra row when someone wants the top; the level is there and proven.
export const AGENT_PRESETS = [
  {
    preset: "devin",
    id: "devin",
    name: "Devin",
    label: "Devin configured model",
    description: "Coding and review using the model selected in your Devin settings.",
    kind: "devin",
    model: "default",
  },
  // Lower-effort choices; existing names and higher tiers stay stable.
  {
    preset: "hemera",
    id: "hemera",
    name: "Hemera",
    label: "Codex GPT 5.6 Sol LOW",
    description: "Small code changes and focused reviews.",
    kind: "codex",
    model: "gpt-5.6-sol",
    effort: "low",
  },
  {
    preset: "phaethon",
    id: "phaethon",
    name: "Phaethon",
    label: "Codex GPT 5.6 Sol MEDIUM",
    description: "Implementation, code review and planning.",
    kind: "codex",
    model: "gpt-5.6-sol",
    effort: "medium",
  },
  {
    preset: "leto",
    id: "leto",
    name: "Leto",
    label: "Pi GPT 5.6 Sol LOW",
    description: "Small code changes and focused reviews.",
    kind: "pi",
    model: "openai-codex/gpt-5.6-sol",
    thinking: "low",
  },
  {
    preset: "asterope",
    id: "asterope",
    name: "Asterope",
    label: "Pi GPT 5.6 Sol MEDIUM",
    description: "Implementation, code review and planning.",
    kind: "pi",
    model: "openai-codex/gpt-5.6-sol",
    thinking: "medium",
  },
  {
    preset: "arvakr",
    id: "arvakr",
    name: "Arvakr",
    label: "OpenCode GPT 5.6 Sol LOW",
    description: "Small code changes and focused reviews.",
    kind: "opencode",
    model: "openrouter/openai/gpt-5.6-sol",
    effort: "low",
  },
  {
    preset: "alsvidr",
    id: "alsvidr",
    name: "Alsvidr",
    label: "OpenCode GPT 5.6 Sol MEDIUM",
    description: "Implementation, code review and planning.",
    kind: "opencode",
    model: "openrouter/openai/gpt-5.6-sol",
    effort: "medium",
  },
  {
    preset: "electra",
    id: "electra",
    name: "Electra",
    label: "Codex GPT 6 Astra LOW",
    description: "Small code changes and focused reviews.",
    kind: "codex",
    model: "gpt-6-astra",
    effort: "low",
  },
  {
    preset: "maia",
    id: "maia",
    name: "Maia",
    label: "Codex GPT 6 Astra MEDIUM",
    description: "Implementation, code review and planning.",
    kind: "codex",
    model: "gpt-6-astra",
    effort: "medium",
  },
  {
    preset: "alcyone",
    id: "alcyone",
    name: "Alcyone",
    label: "Pi GPT 6 Astra LOW",
    description: "Small code changes and focused reviews.",
    kind: "pi",
    model: "openai-codex/gpt-6-astra",
    thinking: "low",
  },
  {
    preset: "merope",
    id: "merope",
    name: "Merope",
    label: "Pi GPT 6 Astra MEDIUM",
    description: "Implementation, code review and planning.",
    kind: "pi",
    model: "openai-codex/gpt-6-astra",
    thinking: "medium",
  },
  {
    preset: "dagr",
    id: "dagr",
    name: "Dagr",
    label: "OpenCode GPT 6 Astra LOW (OpenRouter API)",
    description: "Small code changes and focused reviews.",
    kind: "opencode",
    model: "openrouter/openai/gpt-6-astra",
    effort: "low",
  },
  {
    preset: "skirnir",
    id: "skirnir",
    name: "Skirnir",
    label: "OpenCode GPT 6 Astra MEDIUM (OpenRouter API)",
    description: "Implementation, code review and planning.",
    kind: "opencode",
    model: "openrouter/openai/gpt-6-astra",
    effort: "medium",
  },
  {
    preset: "terpsichore",
    id: "terpsichore",
    name: "Terpsichore",
    label: "Claude Code Fable 5.1 LOW",
    description: "Small code changes and focused reviews.",
    kind: "claude-code",
    model: "claude-fable-5-1",
    effort: "low",
  },
  {
    preset: "musaeus",
    id: "musaeus",
    name: "Musaeus",
    label: "Pi Fable 5.1 LOW (OpenRouter API)",
    description: "Small code changes and focused reviews.",
    kind: "pi",
    model: "openrouter/anthropic/claude-fable-5.1",
    thinking: "low",
  },
  {
    preset: "suttung",
    id: "suttung",
    name: "Suttung",
    label: "OpenCode Fable 5.1 LOW (OpenRouter API)",
    description: "Small code changes and focused reviews.",
    kind: "opencode",
    model: "openrouter/anthropic/claude-fable-5.1",
    effort: "low",
  },

  // --- Claude Fable 5.1 — Anthropic's most capable model (priced above Opus).
  // Muse names on claude-code; bard/storyteller names on the other engines.
  {
    preset: "calliope",
    id: "calliope",
    name: "Calliope",
    label: "Claude Code Fable 5.1 MAX",
    description: "Complex debugging, architecture and detailed review.",
    kind: "claude-code",
    model: "claude-fable-5-1",
    effort: "max",
  },
  {
    preset: "clio",
    id: "clio",
    name: "Clio",
    label: "Claude Code Fable 5.1 XHIGH",
    description: "Complex debugging, architecture and detailed review.",
    kind: "claude-code",
    model: "claude-fable-5-1",
    effort: "xhigh",
  },
  {
    preset: "euterpe",
    id: "euterpe",
    name: "Euterpe",
    label: "Claude Code Fable 5.1 HIGH",
    description: "Complex debugging, architecture and detailed review.",
    kind: "claude-code",
    model: "claude-fable-5-1",
    effort: "high",
  },
  {
    preset: "thalia",
    id: "thalia",
    name: "Thalia",
    label: "Claude Code Fable 5.1 MEDIUM",
    description: "Implementation, code review and planning.",
    kind: "claude-code",
    model: "claude-fable-5-1",
    effort: "medium",
  },

  // --- GPT 5.6 celestial trio (Codex) --------------------------------------
  // OpenAI's 2026 family: Sol (flagship), Terra (balanced), Luna (fast/affordable).
  // Codex's 5.6 effort ladder extends past xhigh with "max" and "ultra" (ultra =
  // max reasoning + automatic task delegation; Sol/Terra only). All combos verified live.
  // Sol sits at `max`, one seat below the proven `ultra` ceiling, by the user's decision
  // (2026-09-06) — a tier ladder is a choice, and this one is recorded so the
  // effort-ceilings audit above does not "fix" it back.
  {
    preset: "hyperion",
    id: "hyperion",
    name: "Hyperion",
    label: "Codex GPT 5.6 Sol MAX",
    description: "Feature work, code review and technical planning.",
    kind: "codex",
    model: "gpt-5.6-sol",
    effort: "max",
  },
  {
    preset: "phoebus",
    id: "phoebus",
    name: "Phoebus",
    label: "Codex GPT 5.6 Sol XHIGH",
    description: "Feature work, code review and technical planning.",
    kind: "codex",
    model: "gpt-5.6-sol",
    effort: "xhigh",
  },
  {
    preset: "theia",
    id: "theia",
    name: "Theia",
    label: "Codex GPT 5.6 Sol HIGH",
    description: "Feature work, code review and technical planning.",
    kind: "codex",
    model: "gpt-5.6-sol",
    effort: "high",
  },
  {
    preset: "gaia",
    id: "gaia",
    name: "Gaia",
    label: "Codex GPT 5.6 Terra XHIGH",
    description: "Everyday implementation and tests.",
    kind: "codex",
    model: "gpt-5.6-terra",
    effort: "xhigh",
  },
  {
    preset: "tellus",
    id: "tellus",
    name: "Tellus",
    label: "Codex GPT 5.6 Terra MAX",
    description: "Everyday implementation and tests.",
    kind: "codex",
    model: "gpt-5.6-terra",
    effort: "max",
  },
  {
    preset: "diana",
    id: "diana",
    name: "Diana",
    label: "Codex GPT 5.6 Luna XHIGH",
    description: "Small fixes and focused coding tasks.",
    kind: "codex",
    model: "gpt-5.6-luna",
    effort: "xhigh",
  },
  {
    preset: "cynthia",
    id: "cynthia",
    name: "Cynthia",
    label: "Codex GPT 5.6 Luna MAX",
    description: "Small fixes and focused coding tasks.",
    kind: "codex",
    model: "gpt-5.6-luna",
    effort: "max",
  },

  {
    preset: "astraeus",
    id: "astraeus",
    name: "Astraeus",
    label: "Codex GPT 6 Astra MAX",
    description: "Complex debugging, architecture and detailed review.",
    kind: "codex",
    model: "gpt-6-astra",
    effort: "max",
  },
  {
    preset: "asteria",
    id: "asteria",
    name: "Asteria",
    label: "Codex GPT 6 Astra XHIGH",
    description: "Complex debugging, architecture and detailed review.",
    kind: "codex",
    model: "gpt-6-astra",
    effort: "xhigh",
  },
  // Astra HIGH (asked for on 2026-09-20): the level between medium and xhigh
  // on every road that reaches Astra, complex work without the lead recommendation.
  {
    preset: "celaeno",
    id: "celaeno",
    name: "Celaeno",
    label: "Codex GPT 6 Astra HIGH",
    description: "Complex debugging, architecture and detailed review.",
    kind: "codex",
    model: "gpt-6-astra",
    effort: "high",
  },

  // --- GPT 6 Astra on the other engines that reach it ----------------------
  // Probed 2026-09-06, each id on the CLI that will run it, at both levels.
  // Pi rides the same ChatGPT (Codex) login the codex trio uses — the id there
  // is `openai-codex/gpt-6-astra`, and pi's own catalog is the reason it is not
  // the OpenRouter one: pi's openrouter store carries no gpt-6 row at all,
  // while opencode's does. So the two harnesses reach Astra by different roads,
  // and the model strings differ, which is why no twin rule couples them.
  //
  // Neither road has codex's `ultra`: pi's thinkingLevelMap tops out at max for
  // this model and OpenRouter's catalog lists low…max. `ultra` is a codex-CLI
  // level no preset currently names, since Sol stepped down to `max`.
  {
    preset: "phosphoros",
    id: "phosphoros",
    name: "Phosphoros",
    label: "Pi GPT 6 Astra MAX",
    description: "Complex debugging, architecture and detailed review.",
    kind: "pi",
    model: "openai-codex/gpt-6-astra",
    thinking: "max",
  },
  {
    preset: "hesperos",
    id: "hesperos",
    name: "Hesperos",
    label: "Pi GPT 6 Astra XHIGH",
    description: "Complex debugging, architecture and detailed review.",
    kind: "pi",
    model: "openai-codex/gpt-6-astra",
    thinking: "xhigh",
  },
  {
    preset: "taygete",
    id: "taygete",
    name: "Taygete",
    label: "Pi GPT 6 Astra HIGH",
    description: "Complex debugging, architecture and detailed review.",
    kind: "pi",
    model: "openai-codex/gpt-6-astra",
    thinking: "high",
  },
  {
    preset: "aurvandil",
    id: "aurvandil",
    name: "Aurvandil",
    label: "OpenCode GPT 6 Astra MAX",
    description: "Complex debugging, architecture and detailed review.",
    kind: "opencode",
    model: "openrouter/openai/gpt-6-astra",
    effort: "max",
  },
  {
    preset: "delling",
    id: "delling",
    name: "Delling",
    label: "OpenCode GPT 6 Astra XHIGH",
    description: "Complex debugging, architecture and detailed review.",
    kind: "opencode",
    model: "openrouter/openai/gpt-6-astra",
    effort: "xhigh",
  },
  {
    preset: "vidar",
    id: "vidar",
    name: "Vidar",
    label: "OpenCode GPT 6 Astra HIGH",
    description: "Complex debugging, architecture and detailed review.",
    kind: "opencode",
    model: "openrouter/openai/gpt-6-astra",
    effort: "high",
  },

  // --- GPT 5.6 on the other engines that reach it --------------------------
  // Pi rides the same ChatGPT (Codex) login the codex trio uses — no OpenRouter
  // credits; OpenCode reaches the same three variants through OpenRouter, whose
  // catalog lists openai/gpt-5.6-{sol,terra,luna}. Greek names on pi, Norse on
  // opencode, matching the rest of the catalog.
  {
    preset: "aether",
    id: "aether",
    name: "Aether",
    label: "Pi GPT 5.6 Sol XHIGH",
    description: "Feature work, code review and technical planning.",
    kind: "pi",
    model: "openai-codex/gpt-5.6-sol",
    thinking: "xhigh",
  },
  {
    preset: "aurora",
    id: "aurora",
    name: "Aurora",
    label: "Pi GPT 5.6 Sol HIGH",
    description: "Feature work, code review and technical planning.",
    kind: "pi",
    model: "openai-codex/gpt-5.6-sol",
    thinking: "high",
  },
  {
    preset: "rhea",
    id: "rhea",
    name: "Rhea",
    label: "Pi GPT 5.6 Terra XHIGH",
    description: "Everyday implementation and tests.",
    kind: "pi",
    model: "openai-codex/gpt-5.6-terra",
    thinking: "xhigh",
  },
  {
    preset: "phoebe",
    id: "phoebe",
    name: "Phoebe",
    label: "Pi GPT 5.6 Luna XHIGH",
    description: "Small fixes and focused coding tasks.",
    kind: "pi",
    model: "openai-codex/gpt-5.6-luna",
    thinking: "xhigh",
  },
  {
    preset: "sunna",
    id: "sunna",
    name: "Sunna",
    label: "OpenCode GPT 5.6 Sol XHIGH",
    description: "Feature work, code review and technical planning.",
    kind: "opencode",
    model: "openrouter/openai/gpt-5.6-sol",
    effort: "xhigh",
  },
  {
    preset: "skinfaxi",
    id: "skinfaxi",
    name: "Skinfaxi",
    label: "OpenCode GPT 5.6 Sol HIGH",
    description: "Feature work, code review and technical planning.",
    kind: "opencode",
    model: "openrouter/openai/gpt-5.6-sol",
    effort: "high",
  },
  {
    preset: "jord",
    id: "jord",
    name: "Jord",
    label: "OpenCode GPT 5.6 Terra XHIGH",
    description: "Everyday implementation and tests.",
    kind: "opencode",
    model: "openrouter/openai/gpt-5.6-terra",
    effort: "xhigh",
  },
  {
    preset: "bil",
    id: "bil",
    name: "Bil",
    label: "OpenCode GPT 5.6 Luna XHIGH",
    description: "Small fixes and focused coding tasks.",
    kind: "opencode",
    model: "openrouter/openai/gpt-5.6-luna",
    effort: "xhigh",
  },

  // --- House team: strong default agents per engine --------------------
  {
    preset: "zeus",
    id: "zeus",
    name: "Zeus",
    label: "Claude Code Opus 5.5 MAX",
    description: "Feature work, code review and technical planning.",
    kind: "claude-code",
    model: "claude-opus-5-5",
    effort: "max",
  },
  {
    preset: "apollo",
    id: "apollo",
    name: "Apollo",
    label: "Claude Code Opus 5.5 XHIGH",
    description: "Feature work, code review and technical planning.",
    kind: "claude-code",
    model: "claude-opus-5-5",
    effort: "xhigh",
  },
  {
    preset: "poseidon",
    id: "poseidon",
    name: "Poseidon",
    label: "Claude Code Opus 5.5 HIGH",
    description: "Feature work, code review and technical planning.",
    kind: "claude-code",
    model: "claude-opus-5-5",
    effort: "high",
  },
  {
    preset: "artemis",
    id: "artemis",
    name: "Artemis",
    label: "Claude Code Opus 5.5 MEDIUM",
    description: "Feature work, code review and technical planning.",
    kind: "claude-code",
    model: "claude-opus-5-5",
    effort: "medium",
  },

  // --- Frontier models on the other engines that run them ------------------
  // OpenCode reaches Fable 5.1 through OpenRouter, whose id spells the version with a DOT
  // (anthropic/claude-fable-5.1) where Anthropic's own API spells it with a dash
  // (claude-fable-5-1) — one model, two spellings, and the wrong one is a 404.
  // Pi uses the user-selected OpenRouter API route, with explicit catalog sync.
  {
    preset: "orpheus",
    id: "orpheus",
    name: "Orpheus",
    label: "Pi Fable 5.1 XHIGH (OpenRouter API)",
    description: "Complex code changes and analysis.",
    kind: "pi",
    model: "openrouter/anthropic/claude-fable-5.1",
    thinking: "xhigh",
  },
  {
    preset: "linus",
    id: "linus",
    name: "Linus",
    label: "Pi Fable 5.1 HIGH (OpenRouter API)",
    description: "Complex code changes and analysis.",
    kind: "pi",
    model: "openrouter/anthropic/claude-fable-5.1",
    thinking: "high",
  },
  {
    preset: "erato",
    id: "erato",
    name: "Erato",
    label: "Pi Fable 5.1 MEDIUM (OpenRouter API)",
    description: "Complex code changes and analysis.",
    kind: "pi",
    model: "openrouter/anthropic/claude-fable-5.1",
    thinking: "medium",
  },
  {
    preset: "saga",
    id: "saga",
    name: "Saga",
    label: "OpenCode Fable 5.1 XHIGH",
    description: "Complex debugging, architecture and detailed review.",
    kind: "opencode",
    model: "openrouter/anthropic/claude-fable-5.1",
    effort: "xhigh",
  },
  {
    preset: "gunnlod",
    id: "gunnlod",
    name: "Gunnlod",
    label: "OpenCode Fable 5.1 HIGH",
    description: "Complex debugging, architecture and detailed review.",
    kind: "opencode",
    model: "openrouter/anthropic/claude-fable-5.1",
    effort: "high",
  },
  {
    preset: "kvasir",
    id: "kvasir",
    name: "Kvasir",
    label: "OpenCode Fable 5.1 MEDIUM",
    description: "Implementation, code review and planning.",
    kind: "opencode",
    model: "openrouter/anthropic/claude-fable-5.1",
    effort: "medium",
  },
  // Pi Opus uses the same OpenRouter route, preserving its xhigh/medium tiers.
  {
    preset: "kronos",
    id: "kronos",
    name: "Kronos",
    label: "Pi Opus 5.5 XHIGH (OpenRouter API)",
    description: "Feature work, code review and technical planning.",
    kind: "pi",
    model: "openrouter/anthropic/claude-opus-5.5",
    thinking: "xhigh",
  },
  {
    preset: "iapetus",
    id: "iapetus",
    name: "Iapetus",
    label: "Pi Opus 5.5 HIGH (OpenRouter API)",
    description: "Feature work, code review and technical planning.",
    kind: "pi",
    model: "openrouter/anthropic/claude-opus-5.5",
    thinking: "high",
  },
  {
    preset: "atlas",
    id: "atlas",
    name: "Atlas",
    label: "Pi Opus 5.5 MEDIUM (OpenRouter API)",
    description: "Feature work, code review and technical planning.",
    kind: "pi",
    model: "openrouter/anthropic/claude-opus-5.5",
    thinking: "medium",
  },
  // Opus 5.5 on OpenCode (via OpenRouter), whose id spells the version with a dot, as Fable's.
  {
    preset: "baldr",
    id: "baldr",
    name: "Baldr",
    label: "OpenCode Opus 5.5 XHIGH",
    description: "Feature work, code review and technical planning.",
    kind: "opencode",
    model: "openrouter/anthropic/claude-opus-5.5",
    effort: "xhigh",
  },
  {
    preset: "hodr",
    id: "hodr",
    name: "Hodr",
    label: "OpenCode Opus 5.5 HIGH",
    description: "Feature work, code review and technical planning.",
    kind: "opencode",
    model: "openrouter/anthropic/claude-opus-5.5",
    effort: "high",
  },
  {
    preset: "vali",
    id: "vali",
    name: "Vali",
    label: "OpenCode Opus 5.5 MEDIUM",
    description: "Feature work, code review and technical planning.",
    kind: "opencode",
    model: "openrouter/anthropic/claude-opus-5.5",
    effort: "medium",
  },
  // GPT 5.5 on OpenCode (via OpenRouter).

  // --- Fast/cheap tier: quick gut-checks ----------------------------------
  {
    preset: "hermod",
    id: "hermod",
    name: "Hermod",
    label: "Claude Code Sonnet 5 MAX",
    description: "Everyday implementation and tests.",
    kind: "claude-code",
    model: "claude-sonnet-5",
    effort: "max",
  },
  {
    preset: "nike",
    id: "nike",
    name: "Nike",
    label: "Pi Gemini 3.8 Flash HIGH (fast)",
    description: "Routine coding and second opinions.",
    kind: "pi",
    model: "openrouter/google/gemini-3.8-flash",
    thinking: "high",
  },
  {
    preset: "freya",
    id: "freya",
    name: "Freya",
    label: "OpenCode DeepSeek V4.1 Flash MAX (fast)",
    description: "Routine coding and second opinions.",
    kind: "opencode",
    model: "openrouter/deepseek/deepseek-v4.1-flash",
    effort: "max",
  },
  {
    preset: "zephyros",
    id: "zephyros",
    name: "Zephyros",
    label: "Pi DeepSeek V4.1 Flash MAX (fast)",
    description: "Routine coding and second opinions.",
    kind: "pi",
    model: "openrouter/deepseek/deepseek-v4.1-flash",
    thinking: "max",
  },
  {
    preset: "sif",
    id: "sif",
    name: "Sif",
    label: "OpenCode Gemini 3.8 Flash HIGH (fast)",
    description: "Routine coding and second opinions.",
    kind: "opencode",
    model: "openrouter/google/gemini-3.8-flash",
    effort: "high",
  },

  // --- pi model zoo (Greek names) — popular OpenRouter models via Pi -------
  {
    preset: "hades",
    id: "hades",
    name: "Hades",
    label: "Pi DeepSeek V4 Pro MAX",
    description: "Complex code changes and analysis.",
    kind: "pi",
    model: "openrouter/deepseek/deepseek-v4-pro-0813",
    thinking: "max",
  },
  {
    preset: "ares",
    id: "ares",
    name: "Ares",
    label: "Pi Grok 4.7 XHIGH",
    description: "Complex code changes and analysis.",
    kind: "pi",
    model: "openrouter/x-ai/grok-4.7",
    thinking: "xhigh",
  },
  {
    preset: "hephaestus",
    id: "hephaestus",
    name: "Hephaestus",
    label: "Pi Qwen3.8 Max XHIGH",
    description: "Complex code changes and analysis.",
    kind: "pi",
    model: "openrouter/qwen/qwen3.8-max",
    thinking: "xhigh",
  },
  {
    preset: "athena",
    id: "athena",
    name: "Athena",
    label: "Pi Qwen3.8 27B XHIGH",
    description: "Coding and analysis across longer tasks.",
    kind: "pi",
    model: "openrouter/qwen/qwen3.8-27b",
    thinking: "xhigh",
  },
  {
    preset: "metis",
    id: "metis",
    name: "Metis",
    label: "Pi MiniMax M3",
    description: "Coding and analysis across longer tasks.",
    kind: "pi",
    model: "openrouter/minimax/minimax-m3",
  },
  {
    preset: "prometheus",
    id: "prometheus",
    name: "Prometheus",
    label: "Pi GLM 5.3 MAX",
    description: "Complex code changes and analysis.",
    kind: "pi",
    model: "openrouter/z-ai/glm-5.3",
    thinking: "max",
  },
  {
    preset: "endymion",
    id: "endymion",
    name: "Endymion",
    label: "Pi Kimi K3 MAX",
    description: "Coding and analysis across longer tasks.",
    kind: "pi",
    model: "openrouter/moonshotai/kimi-k3",
    thinking: "max",
  },

  // --- Three OpenRouter models added 2026-08-24, each verified present in
  // `pi --list-models` and `opencode models` before it was written down. Two
  // ride OpenRouter's free tier; the third was stealth/ox-alpha, whose testing
  // period ended 2026-08-27 — the endpoint now 404s and names its own model:
  // ZAI's GLM 5.3 Flash. nyx and nott follow it there rather than keep a name
  // that answers nothing. That id is NEWER than either harness's catalog:
  // neither `pi --list-models` (refreshed) nor `opencode models` carries
  // z-ai/glm-5.3-flash yet, so it was verified another way on 2026-08-27 —
  // present in OpenRouter's own /api/v1/models with reasoning support, and
  // live one-shot probes on both CLIs answered through it as a custom model
  // id. pi says so out loud ("Using custom model id") and still forwards the
  // thinking level: at max the run reports reasoning tokens, at off it reports
  // none. A `~/.pi/agent/models.json` entry (the endymion pattern) is what
  // buys sane token limits until models.dev catches up. All three report
  // thinking support, so all three sit at the ceiling.
  {
    preset: "nyx",
    id: "nyx",
    name: "Nyx",
    label: "Pi GLM 5.3 Flash MAX",
    description: "Routine coding and second opinions.",
    kind: "pi",
    model: "openrouter/z-ai/glm-5.3-flash",
    thinking: "max",
  },
  {
    preset: "oceanus",
    id: "oceanus",
    name: "Oceanus",
    label: "Pi Nemotron 3 Ultra 550B FREE HIGH",
    description: "Coding and analysis across longer tasks.",
    kind: "pi",
    model: "openrouter/nvidia/nemotron-3-ultra-550b-a55b:free",
    thinking: "high",
  },
  {
    preset: "triton",
    id: "triton",
    name: "Triton",
    label: "Pi Laguna S 2.1 FREE",
    description: "Code changes and repository tasks.",
    kind: "pi",
    model: "openrouter/poolside/laguna-s-2.1:free",
  },

  {
    preset: "eos",
    id: "eos",
    name: "Eos",
    label: "Pi Muse Spark 1.3 XHIGH",
    description: "Collaborative coding and task breakdown.",
    kind: "pi",
    model: "openrouter/meta/muse-spark-1.3",
    thinking: "xhigh",
  },

  // --- opencode model zoo (Norse names) — same models via OpenCode --------
  //
  // Same model AND same effort as the pi twin. A name here is a model plus how
  // hard it thinks, so a pair that agreed on the model and not on the level
  // (ares/thor, hades/odin, hephaestus/tyr, zephyros/freya) was two different
  // agents wearing one description — and it showed: an entry with no effort
  // draws a bare harness tag in the roster UI, which reads as a gap because it
  // was one. Filled in 2026-08-27 against OpenRouter's own
  // supported_parameters: each of those four models lists reasoning_effort, so
  // opencode's `--variant` reaches something. opencode validates nothing here
  // (a bogus variant was probed and ran), which is exactly why the catalog
  // must. Two entries below still carry no effort on purpose — each says why.
  {
    preset: "odin",
    id: "odin",
    name: "Odin",
    label: "OpenCode DeepSeek V4 Pro MAX",
    description: "Complex code changes and analysis.",
    kind: "opencode",
    model: "openrouter/deepseek/deepseek-v4-pro-0813",
    effort: "max",
  },
  {
    preset: "thor",
    id: "thor",
    name: "Thor",
    label: "OpenCode Grok 4.7 XHIGH",
    description: "Complex code changes and analysis.",
    kind: "opencode",
    model: "openrouter/x-ai/grok-4.7",
    effort: "xhigh",
  },
  {
    preset: "tyr",
    id: "tyr",
    name: "Tyr",
    label: "OpenCode Qwen3.8 Max XHIGH",
    description: "Complex code changes and analysis.",
    kind: "opencode",
    model: "openrouter/qwen/qwen3.8-max",
    effort: "xhigh",
  },
  {
    preset: "bragi",
    id: "bragi",
    name: "Bragi",
    label: "OpenCode Qwen3.8 27B XHIGH",
    description: "Coding and analysis across longer tasks.",
    kind: "opencode",
    model: "openrouter/qwen/qwen3.8-27b",
    effort: "xhigh",
  },
  // No effort, deliberately — one of the three models in this catalog that take none.
  // See "Effort ceilings" at the top of the file.
  {
    preset: "mimir",
    id: "mimir",
    name: "Mimir",
    label: "OpenCode MiniMax M3",
    description: "Coding and analysis across longer tasks.",
    kind: "opencode",
    model: "openrouter/minimax/minimax-m3",
  },
  {
    preset: "mani",
    id: "mani",
    name: "Mani",
    label: "OpenCode Kimi K3 MAX",
    description: "Coding and analysis across longer tasks.",
    kind: "opencode",
    model: "openrouter/moonshotai/kimi-k3",
    effort: "max",
  },
  {
    preset: "nott",
    id: "nott",
    name: "Nott",
    label: "OpenCode GLM 5.3 Flash MAX",
    description: "Routine coding and second opinions.",
    kind: "opencode",
    model: "openrouter/z-ai/glm-5.3-flash",
    effort: "max",
  },
  {
    preset: "ymir",
    id: "ymir",
    name: "Ymir",
    label: "OpenCode Nemotron 3 Ultra 550B FREE HIGH",
    description: "Coding and analysis across longer tasks.",
    kind: "opencode",
    model: "openrouter/nvidia/nemotron-3-ultra-550b-a55b:free",
    effort: "high",
  },
  {
    preset: "aegir",
    id: "aegir",
    name: "Aegir",
    label: "OpenCode Laguna S 2.1 FREE",
    description: "Code changes and repository tasks.",
    kind: "opencode",
    model: "openrouter/poolside/laguna-s-2.1:free",
  },

  {
    preset: "logi",
    id: "logi",
    name: "Logi",
    label: "OpenCode Muse Spark 1.3 XHIGH",
    description: "Collaborative coding and task breakdown.",
    kind: "opencode",
    model: "openrouter/meta/muse-spark-1.3",
    effort: "xhigh",
  },


  // --- OpenCode Zen (2026-09-06) -------------------------------------------
  // Zen is OpenCode's own pay-as-you-go gateway, and on this account it is
  // almost entirely out of reach: models.dev lists 102 Zen models — Fable 5.1,
  // Opus 5 and GPT 6 Astra among them — while `opencode models` offers 7 and
  // `opencode auth list` holds no Zen credential. The other 95 need Zen billing
  // switched on. That gap IS the finding, and it is the same lesson the Muse
  // Spark 403 taught three days earlier: a catalog listing is not access.
  //
  // Of the seven that are reachable, two are models this catalog already
  // carries, and both were probed here on 2026-09-06. Being free, they are a
  // second road for when OpenRouter's free tier is rate-limited — which is the
  // one thing a free tier does reliably.
  //
  // No pi twins exist and none can: pi has no Zen provider at all (its auth
  // carries openai-codex, openrouter and opencode-go), so these two rows are
  // opencode-only by necessity and the twin rule has nothing to pair them with.
  // No effort on the Nemotron row — Zen's entry for it publishes no reasoning
  // options, where OpenRouter's does, which is why Ymir names `high` and this
  // one names nothing.
  {
    preset: "audhumla",
    id: "audhumla",
    name: "Audhumla",
    label: "OpenCode Zen Nemotron 3 Ultra 550B FREE",
    description: "Coding and analysis across longer tasks.",
    kind: "opencode",
    model: "opencode/nemotron-3-ultra-free",
  },
  {
    preset: "gefjon",
    id: "gefjon",
    name: "Gefjon",
    label: "OpenCode Zen Muse Spark 1.3 Contributor FREE XHIGH",
    description: "Collaborative coding and task breakdown.",
    kind: "opencode",
    model: "opencode/muse-spark-1.3-contributor-free",
    effort: "xhigh",
  },

  // --- Kimi Code (Finnish names, so a kimi agent is recognisable as one at a
  // glance — Greek, Norse and muse names are all spoken for). Added 2026-08-24.
  //
  // K3 only: K2.7 Code and Highspeed retired at the user's request.
  // Kimi Code uses its own configured account. The child-only
  // KIMI_MODEL_THINKING_EFFORT control selects Low/High/Max without a CLI flag
  // or changes to that account's config. Max is an explicit catalog choice.
  {
    preset: "ilmarinen",
    id: "ilmarinen",
    name: "Ilmarinen",
    label: "Kimi K3 MAX",
    description: "Coding and analysis across longer tasks.",
    kind: "kimi",
    model: "moonshot-ai/kimi-k3",
    effort: "max",
  },
  // --- Image generation through the existing Codex login -----------------
  {
    preset: "pygmalion",
    id: "pygmalion",
    name: "Pygmalion",
    label: "Codex Images (via Codex login)",
    description: "Generate illustrations and edit reference images through your existing Codex login.",
    kind: "image",
    // A logical route, not a selectable image model.
    model: "codex-image",
  },
];

// Reviewed 2026-09-10; source notes live in .specs/agent-catalog-redesign.
// Keys describe exact model identities, not callsigns or saved preset provenance.
const MODEL_LABELS = {
  'gpt-6-astra': 'GPT-6 Astra',
  'gpt-5.6-sol': 'GPT-5.6 Sol',
  'gpt-5.6-terra': 'GPT-5.6 Terra',
  'gpt-5.6-luna': 'GPT-5.6 Luna',
  'claude-fable-5.1': 'Claude Fable 5.1',
  'claude-fable-5': 'Claude Fable 5',
  'claude-opus-5.5': 'Claude Opus 5.5',
  'claude-sonnet-5': 'Claude Sonnet 5',
  'gemini-3.8-flash': 'Gemini 3.8 Flash',
  'deepseek-v4.1-flash': 'DeepSeek V4.1 Flash',
  'deepseek-v4-pro-0813': 'DeepSeek V4 Pro (0813)',
  'grok-4.7': 'Grok 4.7',
  'qwen3.8-max': 'Qwen 3.8 Max',
  'qwen3.8-27b': 'Qwen 3.8 27B',
  'minimax-m3': 'MiniMax M3',
  'glm-5.3': 'GLM 5.3',
  'glm-5.3-flash': 'GLM 5.3 Flash',
  'kimi-k3': 'Kimi K3',
  'nemotron-3-ultra-550b-a55b:free': 'Nemotron 3 Ultra (free)',
  'nemotron-3-ultra-free': 'Nemotron 3 Ultra (Zen free)',
  'laguna-s-2.1:free': 'Laguna S 2.1 (free)',
  'muse-spark-1.3': 'Muse Spark 1.3',
}

export const WORK_TIERS = {
  critical: { label: 'Critical work', description: 'Important reviews, architecture, hard problems and important questions. No coding or routine advice.' },
  complex: { label: 'Complex work', description: 'Demanding implementation, investigation, planning and substantial reviews.' },
  standard: { label: 'Standard work', description: 'Feature work, tests, research, planning and ordinary reviews.' },
  light: { label: 'Light work', description: 'Bounded fixes, lookups and routine tasks; verify the model is suitable.' },
};

/** The roles a model suits, in the order the pills show them; a pill launches nothing. */
export function validateWorkTier(value) {
  if (value != null && (typeof value !== 'string' || !Object.hasOwn(WORK_TIERS, value)))
    throw new Error('Work tier must be critical, complex, standard or light');
}

/** Work tiers express the owner's allocation policy, not benchmark or price ranks. */
export function agentProfile(agent) {
  const profile = modelProfile(agent);
  const harness = agent.harness ?? (agent.kind === 'claude-code' ? 'claude' : agent.kind);
  const effort = harness === 'pi' ? agent.thinking ?? agent.effort : agent.effort;
  const known = AGENT_PRESETS.some(p => (p.kind === 'claude-code' ? 'claude' : p.kind) === harness && p.model === agent.model);
  let tier = 'light';
  if (known && /kimi-k3$/.test(profile.modelKey)) tier = 'complex';
  else if (known && (['low', 'medium', 'high', 'xhigh', 'max'].includes(effort) || (harness === 'codex' && effort === 'ultra'))) {
    if (['gpt-6-astra', 'claude-fable-5.1'].includes(profile.modelKey))
      tier = ['max', 'ultra'].includes(effort) ? 'critical' : ['high', 'xhigh'].includes(effort) ? 'complex' : effort === 'medium' ? 'standard' : 'light';
    else if (profile.modelKey === 'claude-opus-5.5')
      tier = effort === 'max' ? 'critical' : effort === 'xhigh' ? 'complex' : effort === 'low' ? 'light' : 'standard';
    else if (profile.modelKey === 'gpt-5.6-sol' && !['low', 'medium'].includes(effort)) tier = 'standard';
  }
  validateWorkTier(agent.workTier);
  profile.workTier = agent.workTier ?? tier;
  return profile;
}

function modelProfile({ harness, kind, model, effort, thinking }) {
  harness ??= kind === "claude-code" ? "claude" : kind
  if (harness === "pi") effort = thinking ?? effort
  if (harness === 'devin') return {
    modelKey: model && model !== 'default' ? model : 'devin-configured',
    modelLabel: model && model !== 'default' ? model : 'Devin configured model',
    routeLabel: 'Devin account',
  }
  if (harness === 'image')
    return {
      modelKey: 'codex-image',
      modelLabel: 'Codex Images',
      routeLabel: 'Codex login',
    }
  const known = AGENT_PRESETS.some((p) => (p.kind === "claude-code" ? "claude" : p.kind) === harness && p.model === model)
  // Strip provider paths only AFTER an exact curated model/harness match.
  const key = known
    ? model
        .split('/')
        .at(-1)
        // Anthropic's own ids spell the version with a dash; the key with the dot, as OpenRouter does.
        .replace(/^claude-(fable|opus)-(\d)-(\d)$/, 'claude-$1-$2.$3')
        // Contributor/free are reviewed pricing and data-use routes for Muse 1.3.
        .replace(/^muse-spark-1\.3-contributor(?:-free)?$/, 'muse-spark-1.3')
    : (model ?? "default")
  const contributor = known && key === 'muse-spark-1.3' && model.includes('-contributor')
  const routeLabel = model?.startsWith('openrouter/')
    ? 'OpenRouter · API'
    : model?.startsWith('opencode-go/')
      ? 'OpenCode Go'
      : model?.startsWith('opencode/')
        ? 'OpenCode Zen'
        : model?.startsWith('openai-codex/')
          ? 'Codex subscription'
          : model?.startsWith('anthropic/')
            ? 'Anthropic · API'
            : ({ claude: 'Claude Code account', codex: 'Codex login', kimi: 'Kimi Code account' }[
                harness
              ] ?? harness)
  return {
    modelKey: key,
    modelLabel: (known && MODEL_LABELS[key]) || model || "Default",
    routeLabel: routeLabel + (contributor ? (model.endsWith('-free') ? ' · Contributor · Free' : ' · Contributor') : ''),
    ...(contributor ? { routeNote: 'Prompts and replies may train Meta models.' } : {}),
  }
}

export const KIMI_EFFORTS = ['low', 'high', 'max'];

export function validateKimiEffort(agent) {
  if ((agent.kind ?? agent.harness) !== 'kimi' || agent.effort == null || agent.effort === '') return;
  if (!KIMI_EFFORTS.includes(agent.effort)) {
    throw new Error('Kimi K3 effort must be low, high or max; leave it blank to use Kimi settings');
  }
}

function getPreset(ref) {
  const id = slugify(stripMention(ref));
  return AGENT_PRESETS.find((preset) => preset.preset === id || preset.id === id || slugify(preset.name) === id) ?? null;
}

// --- Catalog drift -------------------------------------------------------
// A roster entry snapshots its preset's engine fields, so a ConsensFlow update that ships a new
// catalog (Opus 4.8 → Opus 5, say) does not reach agents that were already added. These
// helpers re-resolve that: the fields below are decided entirely by the preset — agentFromPreset
// lets only --name/--id/--cwd/--description through and there is no `agents edit` — so replacing
// them with the catalog's current values is lossless.
// `description` joined the list on 2026-08-27, the maintainer's call, after a live update: nyx moved
// the retired stealth/ox-alpha to z-ai/glm-5.3-flash and the roster — and with it the skill table
// every lead reads — went on saying "Pi Ox Alpha MAX" beside the new model. It was called cosmetic
// while it was only a roster field; it is not, now that the generated skill prints it as the line
// that says WHO an agent is. A label naming a model the agent no longer runs is a wrong answer to
// the only question the table exists to answer.
// It was kept out for two reasons, and both were weighed before it went in. The one that expired:
// two hosts sharing ONE roster worded some descriptions differently (pygmalion's login wording), so
// syncing would never converge — each host re-flagging the other's text forever. The host payloads
// went on 2026-08-23 and nothing but the manager writes a description now. The one that stands:
// `add <preset> --description …` is a real override, and this rewrites it on the next catalog move
// without asking. The escape hatch is provenance, not wording — an agent added with an explicit
// --model or --effort carries no `preset` and is never synced at all.
// Agents with no `preset`, or whose preset has since left the catalog, are left alone.
const PRESET_OWNED_FIELDS = ["kind", "model", "effort", "thinking", "description"];

// The roster's `description` is the preset's one-line LABEL ("Pi GLM 5.3 Flash MAX") — what an add
// writes and what the generated skill prints beside the agent's name. The preset's own
// `description` is the catalog card's paragraph and belongs to the UI, not to a roster row.
function presetOwnedValue(field, preset) {
  if (field === "description") return preset.label ?? preset.description;
  return presetFieldValue(field, preset);
}

function presetFieldValue(field, source) {
  const value = source?.[field];
  if (value === undefined || value === null || value === "") return undefined;
  return value;
}

function presetForAgent(agent) {
  return agent?.preset ? getPreset(agent.preset) : null;
}

export function presetDrift(agent) {
  const preset = presetForAgent(agent);
  if (!preset) return [];
  const changes = [];
  for (const field of PRESET_OWNED_FIELDS) {
    const from = presetFieldValue(field, agent);
    const to = presetOwnedValue(field, preset);
    if (from !== to) changes.push({ field, from, to });
  }
  return changes;
}

export function syncAgentWithPreset(agent) {
  const changes = presetDrift(agent);
  if (changes.length === 0) return { agent, changes };
  const synced = { ...agent };
  for (const { field, to } of changes) {
    if (to === undefined) delete synced[field];
    else synced[field] = to;
  }
  return { agent: synced, changes };
}
