# The catalog's data

`presets.json` is the catalog's only source: the ready-made agents of every
harness (`presets`, in the order the roster and the pickers list them) and the
label of each model (`modelLabels`). It is edited by hand, directly. Until the
Node daemon was deleted (step 4 of the Rust rewrite) it was generated from
Node's presets module by the recorder of the catalog; both are gone, and
nothing writes this file but the person editing it.

JSON has no comments, so what the JavaScript said beside the rows is here. The
tests in `crates/cf-catalog/tests/catalog/` hold the rules below that a test
can hold (an effort a harness takes, one named wherever a harness has levels,
each OpenCode row at its Pi twin's level, the work tier of a row), and a field
the catalog does not know fails the build's own test, not a user.

Image agents use the Codex login; Codex selects the underlying image model.
The image agent's `model` (`codex-image`) is a logical route, not a selectable
image model.

## Effort ceilings (audited 2026-08-27)

Every preset names the HIGHEST level its model actually takes, and no preset
names a level the model does not have. Both facts come from the harnesses' own
catalogs, which each publish the per-model list: Pi's `thinkingLevelMap`
(`~/.pi/agent/models-store.json`, non-null entries) and models.dev's
`reasoning_options` (OpenCode's `models.json`). They were compared across all
287 models both carry: 281 agree. That agreement is why one name can mean one
thing on both harnesses: a Pi preset and its OpenCode twin sit at the same level
by rule, asserted in `crates/cf-catalog/tests/catalog/`.

The audit found five presets naming a level their model has never had: `max` on
Qwen3.8 27B and on Nemotron 3 Ultra, `xhigh` on Kimi K3, and an effort at all on
MiniMax M3 and Laguna S 2.1. None of them errored: Pi maps an unknown level to
null and sends nothing, and OpenCode validates nothing at all (a deliberately
bogus `--variant` was probed and ran). So the run quietly used the model's
default while the label promised MAX: the failure mode this note exists to
prevent. Three models take no effort parameter at all (MiniMax M3, Laguna S 2.1
free, and Codex Images): their presets name no level, because a level nothing
honours is worse than a blank one.

DeepSeek (V4.1 Flash and V4 Pro 0813, through OpenRouter on Pi and OpenCode)
sits at `max`: OpenRouter's own model records (`GET /api/v1/models`,
`reasoning.supported_efforts`, read on 2026-09-23) list {max, high, low} for
both, with `high` the default, and both harnesses take `max`. Until then the
rows held `high`, the one level two catalogs agreed on. The OpenCode Go rows
(every model reached a second way through Go) were dropped on 2026-09-24.

The GPT trio through OpenCode (sunna, jord and bil) is deliberately NOT at its
ceiling: it holds the xhigh tier that the same three models occupy on Codex and
Pi, so the trio means the same thing on every harness. A tier ladder is a
choice; a level the model lacks is a bug.

## GPT 6.1 Sol, Sonnet 5.5, MiMo V2.6 Pro (2026-09-30)

Every Sol row moved from 5.6 to 6.1 under its old name (Terra and Luna stay
5.6). 6.1 Sol takes low..max on OpenRouter, models.dev and Pi 0.99.1's thinking
map, so no row's level changed; `ultra` was not probed on 6.1. Probed with a real
request: Codex needs 0.159.2 (0.158.0 is refused for a ChatGPT account), Pi and
OpenCode ran it through OpenRouter; Pi's `openai-codex` route waited on a fresh
login. Hermod moved to Sonnet 5.5 (`claude-sonnet-5-5`, answered as itself),
still at max. MiMo V2.6 Pro (selene on Pi, idun on OpenCode) takes reasoning on
or off and no level, so its rows name none; both ran through OpenRouter. (models.dev
lists a toggle, Pi has no thinking map and OpenRouter no efforts for it. Probed
2026-09-30.)

Sonnet 5.5 below max (2026-09-30): both levels answered on Claude Code 2.1.286.

Haiku 5.5 (2026-10-08), on Claude Code alone: Devin lists Claude only as Sonnet,
and Pi and OpenCode (OpenRouter) carry Haiku 4.5 and a
`~anthropic/claude-haiku-latest` alias, no 5.5. Claude Code 2.1.294 answers
`claude-haiku-5-5` as itself, in a real window, at each of its five levels
(`npm run live:agent -- --effort low …`). Light work at every level, so one row,
at the ceiling like the other cheap models here (hermod, freya).

## Fable 5.1 (updated 2026-09-10)

Native Claude uses `claude-fable-5-1`; OpenRouter uses
`anthropic/claude-fable-5.1`. Pi 0.85.1 now carries low/medium on the OpenRouter
model, the user-chosen route. Omitted standard thinking-map keys can use provider
defaults; explicit null marks unsupported levels. Do not mistake an omitted key
for a dropped effort. The model and effort sources and the transport evidence are
in the agent-catalog-redesign spec, in git history since the repo dropped its
specs (2026-10-02).

OpenCode reaches Fable 5.1 through OpenRouter, whose id spells the version with a
DOT (`anthropic/claude-fable-5.1`) where Anthropic's own API spells it with a dash
(`claude-fable-5-1`): one model, two spellings, and the wrong one is a 404. Pi
uses the user-selected OpenRouter API route, with explicit catalog sync. Pi's
Opus uses the same OpenRouter route, preserving its xhigh/medium tiers. Opus 5.5
on OpenCode (via OpenRouter) spells the version with a dot, as Fable's does. The
Fable 5.1 rows are Anthropic's most capable model (priced above Opus): Muse names
on Claude Code, bard and storyteller names on the other engines.

## Gemini 3.8 Flash (2026-09-03)

nike and sif moved from Gemini 3.7 Flash to 3.8. The ceiling did NOT move and
neither did the price ($0.75/$3.75 per MTok on both), so `high` stands and the
labels' "its ceiling" stays true: Pi's refreshed `thinkingLevelMap` gives {low,
medium, high} non-null and models.dev gives `reasoning_options` {low, medium,
high}, the same three, on the same day. `max` is not a level this model has, on
either catalog, which is why "put it at maximum effort" lands on `high` here.
Both ids were LIVE-PROBED at that level on the CLI that will run them
(`pi -p --model openrouter/google/gemini-3.8-flash:high` and
`opencode run --model openrouter/google/gemini-3.8-flash --variant high`),
because a catalog listing proves the id and only a run proves the harness.

Muse Spark 1.3 (eos on Pi, logi on OpenCode) went in the same day, and only on
the second attempt, which is the finding worth keeping. Both catalogs list
`meta/muse-spark-1.3` with `reasoning_options` {minimal, low, medium, high,
xhigh}, so its ceiling is `xhigh` and not `max`; OpenRouter's `/api/v1/models`
carries it; every source said ship it. The probe came back 403 on BOTH
harnesses: "This model requires you to complete the following before use: 18+
age confirmation." An account attestation is invisible to every catalog there is,
and a preset written on the catalogs alone would have 403'd on every consult.
Once the attestation was granted the same two probes answered `ok`, and the rows
went in, at `xhigh`, the level both catalogs give and both CLIs ran. A catalog
listing proves the id; only a run proves the account can reach it. The
contributor and free routes of Muse 1.3 are reviewed pricing and data-use routes
(their label says "Prompts and replies may train Meta models.").

## GPT 6 Astra (2026-09-05)

The first GPT 6 row in the catalog, on Codex only: `gpt-6-astra` answers there,
and `gpt-6`, `gpt-6-sol` and `gpt-6-pro` are all refused on a ChatGPT login, as is
`gpt-5.6-pro` even though Codex's own history carries that name. The refusal is
worth knowing because it is USELESS as evidence: "The '<id>' model is not
supported when using Codex with a ChatGPT account" comes back identically for a
deliberately invented id, so it never distinguishes a model that does not exist
from one this plan cannot reach. Only an id that ANSWERS proves anything.

The effort ladder was probed level by level rather than assumed, and Codex,
unlike OpenCode, really validates: a bogus `model_reasoning_effort` is a 400,
which is what makes each probe mean something. `minimal` is refused; low,
medium, high, xhigh, max and ultra all answer. So the model's ceiling is ULTRA
and the two Codex rows (asteria and astraeus) deliberately sit below it, the way
the GPT OpenCode trio does: a tier ladder is a choice, and they were asked for as
xhigh and max. Add an ultra row when someone wants the top; the level is there
and proven. Astra HIGH (asked for on 2026-09-20) is the level between medium and
xhigh on every road that reaches Astra, complex work without the chief
recommendation.

GPT 6 Astra on the other engines that reach it. Probed 2026-09-06, each id on the
CLI that will run it, at both levels. Pi rides the same ChatGPT (Codex) login the
Codex trio uses: the id there is `openai-codex/gpt-6-astra`, and Pi's own catalog
is the reason it is not the OpenRouter one: Pi's OpenRouter store carries no gpt-6
row at all, while OpenCode's does. So the two harnesses reach Astra by different
roads, and the model strings differ, which is why no twin rule couples them.
Neither road has Codex's `ultra`: Pi's `thinkingLevelMap` tops out at max for
this model and OpenRouter's catalog lists low..max. `ultra` is a Codex CLI level
no preset currently names, since Sol stepped down to `max`.

## The GPT celestial trio (Codex): Sol 6.1, Terra and Luna 5.6

OpenAI's 2026 family: Sol (flagship), Terra (balanced), Luna (fast and
affordable). Codex's 5.6 effort ladder extends past xhigh with "max" and "ultra"
(ultra = max reasoning + automatic task delegation; Sol and Terra only). All
combinations were verified live. Sol sits at `max`, one seat below the proven
`ultra` ceiling, by the owner's decision (2026-09-06): a tier ladder is a choice,
and this one is recorded so that the effort-ceilings audit above does not "fix" it
back (a test holds that no preset names `ultra`).

On the other engines, Pi rides the same ChatGPT (Codex) login the Codex trio
uses (no OpenRouter credits); OpenCode reaches the same three variants through
OpenRouter, whose catalog lists `openai/gpt-6.1-sol` and
`openai/gpt-5.6-{terra,luna}`. Greek names on Pi, Norse on OpenCode, matching the
rest of the catalog. GPT 5.5 on OpenCode goes through OpenRouter.

## Devin (2026-10-01 and 2026-10-02)

Devin's flagship models carry Egyptian names. Devin lists 54 model families;
these are the ladders Claude Code and Codex carry, at the same levels, and
Devin's own SWE-2. Devin writes the level into the model id
(`claude-opus-5-5-max`): a row names the family and the effort, and the launch
joins them (`window_args` in `cf-harness`). Each family answered "Upgrade to Pro
to access this model" on a free plan (probed one level each with `devin -p`). The
rows carry no note about it: Devin says so itself.

Devin's SWE-1.6 (2026-10-02): Devin lists it with no levels, under three ids:
`swe-1-6`, `swe-1-6-fast` and `swe-1-6-slow`. The owner asked for the plain one
and the slow one; the slow one is what a free Devin plan runs (proven
2026-09-19, when plain `swe-1-6` answered "Upgrade to Pro" there).

## The Pi model zoo (Greek names) and the OpenCode zoo (Norse names)

Popular OpenRouter models via Pi, and the same models via OpenCode. Same model
AND same effort as the Pi twin. A name here is a model plus how hard it thinks, so
a pair that agreed on the model and not on the level (ares/thor, hades/odin,
hephaestus/tyr, zephyros/freya) was two different agents wearing one
description, and it showed: an entry with no effort draws a bare harness tag in
the roster UI, which reads as a gap because it was one. Filled in 2026-08-27
against OpenRouter's own `supported_parameters`: each of those four models lists
`reasoning_effort`, so OpenCode's `--variant` reaches something. OpenCode
validates nothing here (a bogus variant was probed and ran), which is exactly why
the catalog must. Two entries still carry no effort on purpose; each of them is
one of the three models in the catalog that take none (see the effort ceilings).

Three OpenRouter models added 2026-08-24, each verified present in
`pi --list-models` and `opencode models` before it was written down. Two ride
OpenRouter's free tier; the third was stealth/ox-alpha, whose testing period
ended 2026-08-27: the endpoint now 404s and names its own model: ZAI's GLM 5.3
Flash. nyx and nott follow it there rather than keep a name that answers nothing.
That id is NEWER than either harness's catalog: neither `pi --list-models`
(refreshed) nor `opencode models` carries `z-ai/glm-5.3-flash` yet, so it was
verified another way on 2026-08-27: present in OpenRouter's own
`/api/v1/models` with reasoning support, and live one-shot probes on both CLIs
answered through it as a custom model id. Pi says so out loud ("Using custom
model id") and still forwards the thinking level: at max the run reports
reasoning tokens, at off it reports none. A `~/.pi/agent/models.json` entry (the
endymion pattern) is what buys sane token limits until models.dev catches up. All
three report thinking support, so all three sit at the ceiling.

Lower-effort choices were added without renaming: existing names and higher
tiers stay stable.

## OpenCode Zen (2026-09-06)

Zen is OpenCode's own pay-as-you-go gateway, and on this account it is almost
entirely out of reach: models.dev lists 102 Zen models (Fable 5.1, Opus 5 and
GPT 6 Astra among them) while `opencode models` offers 7 and `opencode auth list`
holds no Zen credential. The other 95 need Zen billing switched on. That gap IS
the finding, and it is the same lesson the Muse Spark 403 taught three days
earlier: a catalog listing is not access.

Of the seven that are reachable, two are models this catalog already carries, and
both were probed on 2026-09-06. Being free, they are a second road for when
OpenRouter's free tier is rate-limited, which is the one thing a free tier does
reliably.

No Pi twins exist and none can: Pi has no Zen provider at all (its auth carries
`openai-codex`, `openrouter` and `opencode-go`), so these two rows are
OpenCode-only by necessity and the twin rule has nothing to pair them with. No
effort on the Nemotron row: Zen's entry for it publishes no reasoning options,
where OpenRouter's does, which is why Ymir names `high` and this one names
nothing.

## Model labels

`modelLabels` was reviewed 2026-09-10 (source notes: the agent-catalog-redesign
spec, in git history). Its keys describe exact model identities, not callsigns
or saved preset provenance. A model key is the model's id with its provider path
stripped, after an exact match with a preset of the harness: Anthropic's own ids
spell the version with a dash, the key with the dot, as OpenRouter does;
Devin spells GPT versions with dashes too (`gpt-6-1-sol`); and Muse Spark's
contributor and free routes are one model.
