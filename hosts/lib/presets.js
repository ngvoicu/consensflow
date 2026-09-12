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
// The one disagreement that touches this catalog is DeepSeek V4 (all variants): pi says
// {high, xhigh}, models.dev says {low, high, max}. Probing could not settle it — pi returns
// reasoning tokens at both of its levels, and opencode reports zero reasoning on this model at
// every variant, including the `high` both catalogs confirm. So zephyros/hades/freya/odin sit at
// `high`, the only level both sources agree exists. Raise it when one of them is proven right.
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
    label: "Claude Code Opus 5 MAX",
    description: "Feature work, code review and technical planning.",
    kind: "claude-code",
    model: "claude-opus-5",
    effort: "max",
  },
  {
    preset: "apollo",
    id: "apollo",
    name: "Apollo",
    label: "Claude Code Opus 5 XHIGH",
    description: "Feature work, code review and technical planning.",
    kind: "claude-code",
    model: "claude-opus-5",
    effort: "xhigh",
  },
  {
    preset: "artemis",
    id: "artemis",
    name: "Artemis",
    label: "Claude Code Opus 5 MEDIUM",
    description: "Feature work, code review and technical planning.",
    kind: "claude-code",
    model: "claude-opus-5",
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
    skillsPolicy: "default",
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
    skillsPolicy: "default",
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
    skillsPolicy: "default",
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
    label: "Pi Opus 5 XHIGH (OpenRouter API)",
    description: "Feature work, code review and technical planning.",
    kind: "pi",
    model: "openrouter/anthropic/claude-opus-5",
    thinking: "xhigh",
    skillsPolicy: "default",
  },
  {
    preset: "atlas",
    id: "atlas",
    name: "Atlas",
    label: "Pi Opus 5 MEDIUM (OpenRouter API)",
    description: "Feature work, code review and technical planning.",
    kind: "pi",
    model: "openrouter/anthropic/claude-opus-5",
    thinking: "medium",
    skillsPolicy: "default",
  },
  // Opus 5 on OpenCode (via OpenRouter). Unlike the 4.8 generation there is no dotted id:
  // it is plainly anthropic/claude-opus-5. Kept at the xhigh/medium tiers the 4.8 pair used.
  {
    preset: "baldr",
    id: "baldr",
    name: "Baldr",
    label: "OpenCode Opus 5 XHIGH",
    description: "Feature work, code review and technical planning.",
    kind: "opencode",
    model: "openrouter/anthropic/claude-opus-5",
    effort: "xhigh",
  },
  {
    preset: "vali",
    id: "vali",
    name: "Vali",
    label: "OpenCode Opus 5 MEDIUM",
    description: "Feature work, code review and technical planning.",
    kind: "opencode",
    model: "openrouter/anthropic/claude-opus-5",
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
    skillsPolicy: "default",
  },
  {
    preset: "freya",
    id: "freya",
    name: "Freya",
    label: "OpenCode DeepSeek V4 Flash HIGH (fast)",
    description: "Routine coding and second opinions.",
    kind: "opencode",
    model: "openrouter/deepseek/deepseek-v4-flash-0731",
    effort: "high",
  },
  {
    preset: "zephyros",
    id: "zephyros",
    name: "Zephyros",
    label: "Pi DeepSeek V4 Flash HIGH (fast)",
    description: "Routine coding and second opinions.",
    kind: "pi",
    model: "openrouter/deepseek/deepseek-v4-flash-0731",
    thinking: "high",
    skillsPolicy: "default",
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
    label: "Pi DeepSeek V4 Pro",
    description: "Complex code changes and analysis.",
    kind: "pi",
    model: "openrouter/deepseek/deepseek-v4-pro-0813",
    thinking: "high",
    skillsPolicy: "default",
  },
  {
    preset: "ares",
    id: "ares",
    name: "Ares",
    label: "Pi Grok 4.6 XHIGH",
    description: "Complex code changes and analysis.",
    kind: "pi",
    model: "openrouter/x-ai/grok-4.6",
    thinking: "xhigh",
    skillsPolicy: "default",
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
    skillsPolicy: "default",
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
    skillsPolicy: "default",
  },
  {
    preset: "metis",
    id: "metis",
    name: "Metis",
    label: "Pi MiniMax M3",
    description: "Coding and analysis across longer tasks.",
    kind: "pi",
    model: "openrouter/minimax/minimax-m3",
    skillsPolicy: "default",
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
    skillsPolicy: "default",
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
    skillsPolicy: "default",
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
    skillsPolicy: "default",
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
    label: "OpenCode DeepSeek V4 Pro HIGH",
    description: "Complex code changes and analysis.",
    kind: "opencode",
    model: "openrouter/deepseek/deepseek-v4-pro-0813",
    effort: "high",
  },
  {
    preset: "thor",
    id: "thor",
    name: "Thor",
    label: "OpenCode Grok 4.6 XHIGH",
    description: "Complex code changes and analysis.",
    kind: "opencode",
    model: "openrouter/x-ai/grok-4.6",
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

  // --- OpenCode Go route (2026-09-06) --------------------------------------
  // Ten models this catalog already carries, reached a second way. OpenCode Go
  // is a $10/month subscription metered in DOLLARS OF USAGE rather than tokens
  // ($12 per 5 hours, $30 per week, $60 per month), so these rows are the cheap
  // road and their OpenRouter twins stay as the fallback for when Go is
  // throttled or down. Nothing was retired to make room: same model, two roads,
  // two names.
  //
  // The id is identical on both harnesses — `opencode-go/<model>`, byte for
  // byte — which is what lets a pi row and its opencode twin sit at the same
  // level by rule, the pairing tests/catalog.test.mjs asserts.
  //
  // All twenty rows were probed on the CLI that will run them, at the level
  // each one names: `pi -p --model opencode-go/<id> --thinking <level>` and
  // `opencode run --model opencode-go/<id> --variant <level>`. What that proves
  // is bounded, and the bound is the point — opencode validates no variant at
  // all and pi maps an unknown level to null and sends nothing, so a passing
  // probe proves the ID AND THE ACCOUNT, never the level. The levels come from
  // the two catalogs agreeing: pi's thinkingLevelMap (~/.pi/agent/models-store.json)
  // and models.dev's reasoning_options, which agree on all ten models here.
  //
  // Two deliberate departures from "name the ceiling":
  //   * gpt-5.6-luna could take `max` on this road — both catalogs list it —
  //     and holds `xhigh` anyway, the tier the whole GPT 5.6 family holds on
  //     every harness. A ladder is a choice, and that one is already made.
  //   * minimax-m3 names no level at all: pi lists none, and models.dev gives
  //     it a reasoning TOGGLE with no effort values. A level nothing honours is
  //     worse than a blank one.
  //
  // DeepSeek runs the other way, and it is worth recording because the
  // OpenRouter rows sit LOWER: zephyros/hades/freya/odin hold `high` because pi
  // and models.dev disagreed about that road. On Go the two agree — {low, high,
  // max} for Flash, {high, max} for Pro — so these rows take `max`, and both
  // were probed there on both harnesses. Same model, different road, different
  // evidence, different ceiling.
  //
  // PRIVACY, because it differs row by row. Go's own model table says "Not
  // used" for training on every model here except Muse Spark: the `-contributor`
  // tier costs $0.10/$0.20 per MTok against $1.25/$4.25 for the standard model
  // precisely because you grant permission to use your prompts and completions
  // to train future Meta models. Grok 4.6 and GPT 5.6 Luna keep 30 days of logs
  // for abuse monitoring; the rest keep none. Urania and Odrerir say so in their
  // own descriptions — a row that spends your privacy should not read like one
  // that does not.
  {
    preset: "boreas",
    id: "boreas",
    name: "Boreas",
    label: "Pi DeepSeek V4 Flash MAX (OpenCode Go)",
    description: "Routine coding and second opinions.",
    kind: "pi",
    model: "opencode-go/deepseek-v4-flash",
    thinking: "max",
  },
  {
    preset: "nereus",
    id: "nereus",
    name: "Nereus",
    label: "Pi DeepSeek V4 Pro MAX (OpenCode Go)",
    description: "Complex code changes and analysis.",
    kind: "pi",
    model: "opencode-go/deepseek-v4-pro",
    thinking: "max",
  },
  {
    preset: "eris",
    id: "eris",
    name: "Eris",
    label: "Pi Grok 4.6 XHIGH (OpenCode Go)",
    description: "Complex code changes and analysis.",
    kind: "pi",
    model: "opencode-go/grok-4.6",
    thinking: "xhigh",
  },
  {
    preset: "coeus",
    id: "coeus",
    name: "Coeus",
    label: "Pi Qwen3.8 Max XHIGH (OpenCode Go)",
    description: "Complex code changes and analysis.",
    kind: "pi",
    model: "opencode-go/qwen3.8-max",
    thinking: "xhigh",
  },
  // No thinking level, deliberately — the model takes a reasoning toggle and no
  // levels, on both catalogs. See the section note above.
  {
    preset: "kairos",
    id: "kairos",
    name: "Kairos",
    label: "Pi MiniMax M3 (OpenCode Go)",
    description: "Coding and analysis across longer tasks.",
    kind: "pi",
    model: "opencode-go/minimax-m3",
  },
  {
    preset: "hecate",
    id: "hecate",
    name: "Hecate",
    label: "Pi GLM 5.3 MAX (OpenCode Go)",
    description: "Complex code changes and analysis.",
    kind: "pi",
    model: "opencode-go/glm-5.3",
    thinking: "max",
  },
  {
    preset: "hermes",
    id: "hermes",
    name: "Hermes",
    label: "Pi GLM 5.3 Flash MAX (OpenCode Go)",
    description: "Routine coding and second opinions.",
    kind: "pi",
    model: "opencode-go/glm-5.3-flash",
    thinking: "max",
  },
  {
    preset: "mnemosyne",
    id: "mnemosyne",
    name: "Mnemosyne",
    label: "Pi Kimi K3 MAX (OpenCode Go)",
    description: "Coding and analysis across longer tasks.",
    kind: "pi",
    model: "opencode-go/kimi-k3",
    thinking: "max",
  },
  {
    preset: "urania",
    id: "urania",
    name: "Urania",
    label: "Pi Muse Spark 1.3 Contributor XHIGH (OpenCode Go)",
    description: "Collaborative coding and task breakdown.",
    kind: "pi",
    model: "opencode-go/muse-spark-1.3-contributor",
    thinking: "xhigh",
  },
  {
    preset: "selene",
    id: "selene",
    name: "Selene",
    label: "Pi GPT 5.6 Luna XHIGH (OpenCode Go)",
    description: "Small fixes and focused coding tasks.",
    kind: "pi",
    model: "opencode-go/gpt-5.6-luna",
    thinking: "xhigh",
  },
  {
    preset: "dvalin",
    id: "dvalin",
    name: "Dvalin",
    label: "OpenCode Go DeepSeek V4 Flash MAX",
    description: "Routine coding and second opinions.",
    kind: "opencode",
    model: "opencode-go/deepseek-v4-flash",
    effort: "max",
  },
  {
    preset: "durin",
    id: "durin",
    name: "Durin",
    label: "OpenCode Go DeepSeek V4 Pro MAX",
    description: "Complex code changes and analysis.",
    kind: "opencode",
    model: "opencode-go/deepseek-v4-pro",
    effort: "max",
  },
  {
    preset: "loki",
    id: "loki",
    name: "Loki",
    label: "OpenCode Go Grok 4.6 XHIGH",
    description: "Complex code changes and analysis.",
    kind: "opencode",
    model: "opencode-go/grok-4.6",
    effort: "xhigh",
  },
  {
    preset: "alviss",
    id: "alviss",
    name: "Alviss",
    label: "OpenCode Go Qwen3.8 Max XHIGH",
    description: "Complex code changes and analysis.",
    kind: "opencode",
    model: "opencode-go/qwen3.8-max",
    effort: "xhigh",
  },
  // No effort, deliberately — same reason as Kairos above.
  {
    preset: "andvari",
    id: "andvari",
    name: "Andvari",
    label: "OpenCode Go MiniMax M3",
    description: "Coding and analysis across longer tasks.",
    kind: "opencode",
    model: "opencode-go/minimax-m3",
  },
  {
    preset: "brokkr",
    id: "brokkr",
    name: "Brokkr",
    label: "OpenCode Go GLM 5.3 MAX",
    description: "Complex code changes and analysis.",
    kind: "opencode",
    model: "opencode-go/glm-5.3",
    effort: "max",
  },
  {
    preset: "sindri",
    id: "sindri",
    name: "Sindri",
    label: "OpenCode Go GLM 5.3 Flash MAX",
    description: "Routine coding and second opinions.",
    kind: "opencode",
    model: "opencode-go/glm-5.3-flash",
    effort: "max",
  },
  {
    preset: "regin",
    id: "regin",
    name: "Regin",
    label: "OpenCode Go Kimi K3 MAX",
    description: "Coding and analysis across longer tasks.",
    kind: "opencode",
    model: "opencode-go/kimi-k3",
    effort: "max",
  },
  {
    preset: "odrerir",
    id: "odrerir",
    name: "Odrerir",
    label: "OpenCode Go Muse Spark 1.3 Contributor XHIGH",
    description: "Collaborative coding and task breakdown.",
    kind: "opencode",
    model: "opencode-go/muse-spark-1.3-contributor",
    effort: "xhigh",
  },
  {
    preset: "hjuki",
    id: "hjuki",
    name: "Hjuki",
    label: "OpenCode Go GPT 5.6 Luna XHIGH",
    description: "Small fixes and focused coding tasks.",
    kind: "opencode",
    model: "opencode-go/gpt-5.6-luna",
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
  'claude-opus-5': 'Claude Opus 5',
  'claude-sonnet-5': 'Claude Sonnet 5',
  'gemini-3.8-flash': 'Gemini 3.8 Flash',
  'deepseek-v4-flash-0731': 'DeepSeek V4 Flash (0731)',
  'deepseek-v4-pro-0813': 'DeepSeek V4 Pro (0813)',
  'deepseek-v4-flash': 'DeepSeek V4 Flash',
  'deepseek-v4-pro': 'DeepSeek V4 Pro',
  'grok-4.6': 'Grok 4.6',
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

export function agentProfile({ harness, kind, model, effort, thinking }) {
  harness ??= kind === "claude-code" ? "claude" : kind
  if (harness === "pi") effort = thinking ?? effort
  if (harness === 'image')
    return {
      modelKey: 'codex-image',
      modelLabel: 'Codex Images',
      routeLabel: 'Codex login',
      categories: ['images'],
      goodFor: 'Generate illustrations and edit reference images.',
    }
  const known = AGENT_PRESETS.some((p) => (p.kind === "claude-code" ? "claude" : p.kind) === harness && p.model === model)
  // Strip provider paths only AFTER an exact curated model/harness match.
  const key = known
    ? model
        .split('/')
        .at(-1)
        .replace(/^claude-fable-5-1$/, 'claude-fable-5.1')
        // Contributor/free are reviewed pricing and data-use routes for Muse 1.3.
        .replace(/^muse-spark-1\.3-contributor(?:-free)?$/, 'muse-spark-1.3')
    : (model ?? "default")
  const categories = ['claude', 'codex', 'pi', 'opencode', 'kimi'].includes(harness)
    ? ['coding']
    : []
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
  let goodFor = categories.length
    ? 'Use your chosen model for coding tasks.'
    : 'Use your custom harness and model.'
  if (known) {
    if (['gpt-6-astra', 'claude-fable-5.1'].includes(key) || (key === 'gpt-5.6-sol' && ['low', 'medium'].includes(effort))) {
      goodFor =
        effort === 'low'
          ? 'Small code changes and focused reviews.'
          : effort === 'medium'
            ? 'Implementation, code review and planning.'
            : 'Complex debugging, architecture and detailed review.'
    } else if (/sol|opus/.test(key)) goodFor = 'Feature work, code review and technical planning.'
    else if (/terra|sonnet/.test(key)) goodFor = 'Everyday implementation and tests.'
    else if (/luna/.test(key)) goodFor = 'Small fixes and focused coding tasks.'
    else if (/flash/.test(key)) goodFor = 'Routine coding and second opinions.'
    else if (/laguna/.test(key)) goodFor = 'Code changes and repository tasks.'
    else if (/muse/.test(key)) goodFor = 'Collaborative coding and task breakdown.'
    else if (/27b|minimax|kimi|nemotron/.test(key))
      goodFor = 'Coding and analysis across longer tasks.'
    else goodFor = 'Complex code changes and analysis.'
    const supportedEffort =
      (harness === 'kimi' ? KIMI_EFFORTS : ['low', 'medium', 'high', 'xhigh', 'max']).includes(effort) ||
      (harness === 'codex' && effort === 'ultra')
    if (supportedEffort) {
      const roleModel = ['gpt-6-astra', 'claude-fable-5.1', 'gpt-5.6-sol', 'claude-opus-5'].includes(key)
      if (roleModel && ['xhigh', 'max', 'ultra'].includes(effort)) categories.push('lead', 'pm')
      if (effort !== 'low') categories.push('reviewer')
    }
  }
  return {
    modelKey: key,
    modelLabel: (known && MODEL_LABELS[key]) || model || "Default",
    routeLabel: routeLabel + (contributor ? (model.endsWith('-free') ? ' · Contributor · Free' : ' · Contributor') : ''),
    ...(contributor ? { routeNote: 'Prompts and replies may train Meta models.' } : {}),
    categories,
    goodFor,
  }
}

export const KIMI_EFFORTS = ['low', 'high', 'max'];

export function validateKimiEffort(agent) {
  if ((agent.kind ?? agent.harness) !== 'kimi' || agent.effort == null || agent.effort === '') return;
  if (!KIMI_EFFORTS.includes(agent.effort)) {
    throw new Error('Kimi K3 effort must be low, high or max; leave it blank to use Kimi settings');
  }
}

export function getPreset(ref) {
  const id = slugify(stripMention(ref));
  return AGENT_PRESETS.find((preset) => preset.preset === id || preset.id === id || slugify(preset.name) === id) ?? null;
}

export function listPresetIds() {
  return AGENT_PRESETS.map((preset) => preset.preset);
}

export function agentFromPreset(ref, overrides = {}) {
  const preset = getPreset(ref);
  if (!preset) return null;
  const nameOverride = stringOverride(overrides.name);
  const idOverride = stringOverride(overrides.id);
  const name = nameOverride ?? preset.name;
  // Keep the preset's canonical id; only derive a new id when the caller renames (--name) or sets
  // an explicit id.
  const id = slugify(idOverride ?? nameOverride ?? preset.id);
  const agent = {
    ...preset,
    // The label, not the catalog card's paragraph: a roster row's description is the
    // one-liner the skill table prints, and sync now keeps it current — a row created
    // with the paragraph would drift the moment it was written.
    description: preset.label ?? preset.description,
    ...allowedOverrides(overrides),
    preset: preset.preset,
    id,
    name,
    kind: preset.kind,
    model: preset.model,
    effort: preset.effort,
    thinking: preset.thinking,
    skillsPolicy: preset.skillsPolicy,
  };
  delete agent.label;
  return agent;
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
export const PRESET_OWNED_FIELDS = ["kind", "model", "effort", "thinking", "skillsPolicy", "description"];

// The roster's `description` is the preset's one-line LABEL ("Pi GLM 5.3 Flash MAX") — what an add
// writes and what the generated skill prints beside the agent's name. The preset's own
// `description` is the catalog card's paragraph and belongs to the UI, not to a roster row.
function presetOwnedValue(field, preset) {
  if (field === "description") return preset.label ?? preset.description;
  return presetFieldValue(field, preset);
}

// normalizeAgent() fills these in on save, so compare against the same defaults or every
// non-pi agent reports a phantom skillsPolicy change.
const PRESET_FIELD_DEFAULTS = { skillsPolicy: "default" };

function presetFieldValue(field, source) {
  const value = source?.[field];
  if (value === undefined || value === null || value === "") return PRESET_FIELD_DEFAULTS[field];
  return value;
}

export function presetForAgent(agent) {
  return agent?.preset ? getPreset(agent.preset) : null;
}

// True when the entry names a preset the catalog no longer carries (e.g. the GPT 5.5 presets
// retired in 1.7.0). Those stay pinned to what they were created with — sync never touches them.
export function isOrphanedPreset(agent) {
  return Boolean(agent?.preset) && !getPreset(agent.preset);
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

export function driftedAgents(agents) {
  return (agents ?? []).filter((agent) => presetDrift(agent).length > 0);
}

function stringOverride(value) {
  if (value === undefined || value === null || value === true) return undefined;
  const trimmed = String(value).trim();
  return trimmed || undefined;
}

function allowedOverrides(overrides) {
  const result = {};
  for (const key of ["cwd", "description"]) {
    if (overrides[key] !== undefined) result[key] = overrides[key];
  }
  return result;
}

export function formatPresetLine(preset) {
  const effort = preset.effort ? ` effort=${preset.effort}` : preset.thinking ? ` thinking=${preset.thinking}` : "";
  const skills = preset.kind === "pi" ? ` skills=${preset.skillsPolicy ?? "default"}` : "";
  return `- ${preset.preset} → @${preset.id} (${preset.name}): ${preset.label} [${preset.kind} model=${preset.model}${effort}${skills}]`;
}

export function formatPresets() {
  return ["# ConsensFlow agent presets", "", ...AGENT_PRESETS.map(formatPresetLine), "", "Add one with `/consensflow:agents add <preset>`, or `/consensflow:agents add all`."].join("\n");
}
