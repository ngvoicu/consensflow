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
the work tier of a row), and a field
the catalog does not know fails the build's own test, not a user.

Image agents use the Codex login; Codex selects the underlying image model.
The image agent's `model` (`codex-image`) is a logical route, not a selectable
image model.

## Effort ceilings (audited 2026-08-27)

Every preset names the HIGHEST level its model actually takes, and no preset
names a level the model does not have. Both facts come from two catalogs that
each publish the per-model list: Pi's `thinkingLevelMap`
(`~/.pi/agent/models-store.json`, non-null entries) and models.dev's
`reasoning_options`. They were compared across all 287 models both carry: 281
agree.

The audit found five presets naming a level their model has never had: `max` on
Qwen3.8 27B and on Nemotron 3 Ultra, `xhigh` on Kimi K3, and an effort at all on
MiniMax M3 and Laguna S 2.1. None of them errored: Pi maps an unknown level to
null and sends nothing. So the run quietly used the model's
default while the label promised MAX: the failure mode this note exists to
prevent. Three models take no effort parameter at all (MiniMax M3, Laguna S 2.1
free, and Codex Images): their presets name no level, because a level nothing
honours is worse than a blank one.

DeepSeek (V4.1 Flash and V4 Pro 0813, through OpenRouter on Pi) sits at `max`:
OpenRouter's own model records (`GET /api/v1/models`,
`reasoning.supported_efforts`, read on 2026-09-23) list {max, high, low} for
both, with `high` the default, and Pi takes `max`. Until then the rows held
`high`, the one level two catalogs agreed on.

## GPT 6.1 Sol, Sonnet 5.5, MiMo V2.6 Pro (2026-09-30)

Every Sol row moved from 5.6 to 6.1 under its old name (Terra and Luna stay
5.6). 6.1 Sol takes low..max on OpenRouter, models.dev and Pi 0.99.1's thinking
map, so no row's level changed; `ultra` was not probed on 6.1. Probed with a real
request: Codex needs 0.159.2 (0.158.0 is refused for a ChatGPT account), Pi ran
it through OpenRouter; Pi's `openai-codex` route waited on a fresh login. Hermod
moved to Sonnet 5.5 (`claude-sonnet-5-5`, answered as itself), still at max. MiMo
V2.6 Pro (selene on Pi) takes reasoning on or off and no level, so its row names
none; it ran through OpenRouter. (models.dev
lists a toggle, Pi has no thinking map and OpenRouter no efforts for it. Probed
2026-09-30.)

Sonnet 5.5 below max (2026-09-30): both levels answered on Claude Code 2.1.286.

Haiku 5.5 (2026-10-08), on Claude Code alone: Devin lists Claude only as Sonnet,
and Pi (OpenRouter) carries Haiku 4.5 and a `~anthropic/claude-haiku-latest`
alias, no 5.5. Claude Code 2.1.294 answers
`claude-haiku-5-5` as itself, in a real window, at each of its five levels
(`npm run live:agent -- --effort low …`). Light work at every level, so one row,
at the ceiling like the other cheap models here (hermod, zephyros). Devin lists it
since (3000.11.205, 2026-10-25), and carries it at every level with the rest of
its list: light work there too.

## Fable 5.1 (updated 2026-09-10)

Native Claude uses `claude-fable-5-1`; OpenRouter uses
`anthropic/claude-fable-5.1`. Pi 0.85.1 now carries low/medium on the OpenRouter
model, the user-chosen route. Omitted standard thinking-map keys can use provider
defaults; explicit null marks unsupported levels. Do not mistake an omitted key
for a dropped effort. The model and effort sources and the transport evidence are
in the agent-catalog-redesign spec, in git history since the repo dropped its
specs (2026-10-02).

OpenRouter's id for Fable 5.1 spells the version with a DOT
(`anthropic/claude-fable-5.1`) where Anthropic's own API spells it with a dash
(`claude-fable-5-1`): one model, two spellings, and the wrong one is a 404. Pi
uses the user-selected OpenRouter API route, with explicit catalog sync. Pi's
Opus uses the same OpenRouter route, preserving its xhigh/medium tiers. Opus 5.5
through OpenRouter spells the version with a dot, as Fable's does. The
Fable 5.1 rows are Anthropic's most capable model (priced above Opus): Muse names
on Claude Code, bard and storyteller names on the other engines.

## Gemini 3.8 Flash (2026-09-03)

nike moved from Gemini 3.7 Flash to 3.8. The ceiling did NOT move and
neither did the price ($0.75/$3.75 per MTok), so `high` stands and the
labels' "its ceiling" stays true: Pi's refreshed `thinkingLevelMap` gives {low,
medium, high} non-null and models.dev gives `reasoning_options` {low, medium,
high}, the same three, on the same day. `max` is not a level this model has, on
either catalog, which is why "put it at maximum effort" lands on `high` here.
The id was LIVE-PROBED at that level on the CLI that runs it
(`pi -p --model openrouter/google/gemini-3.8-flash:high`), because a catalog
listing proves the id and only a run proves the harness.

Muse Spark 1.3 (eos on Pi) went in the same day, and only on
the second attempt, which is the finding worth keeping. Both catalogs list
`meta/muse-spark-1.3` with `reasoning_options` {minimal, low, medium, high,
xhigh}, so its ceiling is `xhigh` and not `max`; OpenRouter's `/api/v1/models`
carries it; every source said ship it. The probe came back 403: "This model
requires you to complete the following before use: 18+ age confirmation." An
account attestation is invisible to every catalog there is, and a preset written
on the catalogs alone would have 403'd on every consult. Once the attestation was
granted the same probe answered `ok`, and the row went in, at `xhigh`, the level
both catalogs give and Pi ran. A catalog
listing proves the id; only a run proves the account can reach it.

## GPT 6 Astra (2026-09-05)

The first GPT 6 row in the catalog, on Codex only: `gpt-6-astra` answers there,
and `gpt-6`, `gpt-6-sol` and `gpt-6-pro` are all refused on a ChatGPT login, as is
`gpt-5.6-pro` even though Codex's own history carries that name. The refusal is
worth knowing because it is USELESS as evidence: "The '<id>' model is not
supported when using Codex with a ChatGPT account" comes back identically for a
deliberately invented id, so it never distinguishes a model that does not exist
from one this plan cannot reach. Only an id that ANSWERS proves anything.

The effort ladder was probed level by level rather than assumed, and Codex
really validates: a bogus `model_reasoning_effort` is a 400,
which is what makes each probe mean something. `minimal` is refused; low,
medium, high, xhigh, max and ultra all answer. So the model's ceiling is ULTRA
and the two Codex rows (asteria and astraeus) deliberately sit below it: a tier
ladder is a choice, and they were asked for as
xhigh and max. Add an ultra row when someone wants the top; the level is there
and proven. Astra HIGH (asked for on 2026-09-20) is the level between medium and
xhigh on every road that reaches Astra, complex work without the chief
recommendation.

GPT 6 Astra on Pi. Probed 2026-09-06, on the CLI that will run it, at both
levels. Pi rides the same ChatGPT (Codex) login the Codex trio uses: the id there
is `openai-codex/gpt-6-astra`, and Pi's own catalog is the reason it is not the
OpenRouter one: Pi's OpenRouter store carries no gpt-6 row at all. Pi's road has
no Codex `ultra`: Pi's `thinkingLevelMap` tops out at max for this model. `ultra` is a Codex CLI level
no preset currently names, since Sol stepped down to `max`.

## The GPT celestial trio (Codex): Sol 6.1, Terra and Luna 5.6

OpenAI's 2026 family: Sol (flagship), Terra (balanced), Luna (fast and
affordable). Codex's 5.6 effort ladder extends past xhigh with "max" and "ultra"
(ultra = max reasoning + automatic task delegation; Sol and Terra only). All
combinations were verified live. Sol sits at `max`, one seat below the proven
`ultra` ceiling, by the owner's decision (2026-09-06): a tier ladder is a choice,
and this one is recorded so that the effort-ceilings audit above does not "fix" it
back (a test holds that no preset names `ultra`).

On Pi, the trio rides the same ChatGPT (Codex) login the Codex trio uses (no
OpenRouter credits), under Greek names, matching the rest of the catalog.

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

## Every model Devin lists (2026-10-25)

The owner: "add from devin all models it has". Devin 3000.11.205 lists 44 families
(`devin models list --format json`, and the same 44 rows in its `/model`
dialog, walked in a pty); `crates/cf-catalog/tests/catalog/devin-models.json`
keeps the ids and labels, and the test beside it holds that every family is a
row at each of its levels from low to max, and every model with no level a row
by its plain id (Adaptive, GLM 5.2, SWE-1.6, SWE-1.7, Opus 4.6, Sonnet 4.6, Kimi
K2.6 and K2.7): 151 rows. Left out, as the owner decided: the 575 Fusion pairs
(a lead model and a sidekick), the `none` and `minimal` levels below low, and
the dialog's toggles (fast mode `-fast`, `-priority` on GPT, `-lightning` on
SWE-2, a 1M context `-1m`, thinking `-thinking`), which a row cannot name: the
launch joins a family and its effort, nothing more. The new rows carry Egyptian
names, gods first and then kings and queens, and say their work in the words of
their tier.

Devin spells versions with dashes; the profile reads them as the catalog
spells them (`gemini-3-8-flash` is `gemini-3.8-flash`, `claude-5-fable` is
`claude-fable-5`), so a model Pi carries too shares its card and its tier.
DeepSeek V4.1 Flash on Devin is the `1017` snapshot, kept apart from
OpenRouter's, as other snapshots are.

The tiers of the models the table did not know were proposed by the chief and
decided by the owner (2026-10-25): Fable 5 and Opus 5 take Sonnet 5.5's ladder,
Opus 4.8, Sonnet 5 and Muse Wonder take GPT-6.1 Sol's, GPT-6 Sol is standard
work at max only, Gemini 4 Argon is complex at high, standard at medium and
light at low; SWE-2 stays light at every level, and so does every other model
Devin lists. Opus 5.5 at medium is standard work now, on every harness.

Kimi K3.1 Pro is not on Devin's list; Pi reaches it through OpenRouter at low,
high and max (its thinking map has no other level), complex at max, standard at
high and light at low. Gemini 4 Argon is on Pi too, through OpenRouter, at its
three levels. Pi's one-shots got OpenRouter's 402 (Pi asks for the model's whole
942k output tokens, beyond the account's credits) and a hang, the account's and
Pi's, not the rows'.

Probed again on 2026-11-01 (T-612; Pi 1.4.0 in a throwaway home holding only the
`openrouter` entry of the owner's `auth.json`, one probe at a time, each a one-shot
`pi --print --no-extensions --model <id> "Reply with ok"`, about $0.001 each):
- **The 402 is Argon's, not Kimi's.** `openrouter/google/gemini-4-argon` answered
  `402: This request requires more credits, or fewer max_tokens. You requested up to
  942219 tokens, but can only afford 739015` in 1.1 s: Pi 1.4.0's catalog gives it
  943.7K output tokens (`pi --list-models gemini-4`). `openrouter/moonshotai/kimi-k3`
  has 131.1K there and answered `ok` in 2.4 s; `kimi-k3.1-pro` is not in Pi 1.4.0's
  catalog ("Using custom model id", Pi's default of 16K) and answered `ok` in 3.5 s.
  The 942k that T-553 saw on K3.1 Pro was Argon's. Whether the 402 comes is the
  account's: OpenRouter refuses a request whose `max_tokens` at the model's price is
  more than the credits left (739,015 tokens of Argon on that day), so another balance
  or another day answers.
- **The hang did not come back.** Argon answered the 402 at once, and `ok` in 2.9 s
  with the override below; nothing waited.
- **Pi's one lever is a file.** There is no flag, variable or setting for the output
  limit (`pi --help`, `docs/environment-variables.md`, `docs/settings.md`); the model's
  `maxTokens` is set in `models.json` of Pi's agent folder, as a model of the
  provider's own or a `modelOverrides` entry
  (`{"providers":{"openrouter":{"modelOverrides":{"google/gemini-4-argon":{"maxTokens":32768}}}}}`,
  which turned the 402 into `ok` and the catalog's 943.7K into 32.8K). The owner's own
  `models.json` already does it for Kimi K3. A ConsensFlow window runs on the person's
  Pi folder as a terminal Pi does, so their override reaches it; ConsensFlow would have
  to write the person's `models.json` or give Pi a folder of its own to pass one, and it
  does neither. A 402 that Pi reports is read as the billing refusal it is (`Quota::
  Unavailable`, "its billing was refused", with Pi's words, MUS-23), so the window is
  marked out for the human and not retried.

Khnum (Devin SWE-1.6 Slow) went: Devin no longer lists `swe-1-6-slow`. The data
keeps it under `retired`, with why, so a project whose staff or chief still
names it says so in plain words (the board's row, the staff in Project settings,
the chief's card and Switch chief, a task taken back, the chief's note) and asks
the human to pick another agent, rather than to define it again on a model Devin
no longer has.

## The Pi model zoo (Greek names)

Popular OpenRouter models via Pi. A name here is a model plus how hard it thinks:
an entry with no effort draws a bare harness tag in the roster UI, which reads as
a gap. The efforts of ares, hades, hephaestus and zephyros were filled in
2026-08-27 against OpenRouter's own `supported_parameters`: each of those four
models lists `reasoning_effort`. Two entries still carry no effort on purpose;
each of them is one of the three models in the catalog that take none (see the
effort ceilings).

Three OpenRouter models added 2026-08-24, each verified present in
`pi --list-models` before it was written down. Two ride
OpenRouter's free tier; the third was stealth/ox-alpha, whose testing period
ended 2026-08-27: the endpoint now 404s and names its own model: ZAI's GLM 5.3
Flash. nyx follows it there rather than keep a name that answers nothing.
That id is NEWER than Pi's catalog: `pi --list-models` (refreshed) does not carry
`z-ai/glm-5.3-flash` yet, so it was verified another way on 2026-08-27: present
in OpenRouter's own `/api/v1/models` with reasoning support, and a live one-shot
probe on Pi answered through it as a custom model id. Pi says so out loud ("Using custom
model id") and still forwards the thinking level: at max the run reports
reasoning tokens, at off it reports none. A `~/.pi/agent/models.json` entry (the
endymion pattern) is what buys sane token limits until models.dev catches up. All
three report thinking support, so all three sit at the ceiling.

Lower-effort choices were added without renaming: existing names and higher
tiers stay stable.

## Model labels

`modelLabels` was reviewed 2026-09-10 (source notes: the agent-catalog-redesign
spec, in git history). Its keys describe exact model identities, not callsigns
or saved preset provenance. A model key is the model's id with its provider path
stripped, after an exact match with a preset of the harness: Anthropic's own ids
spell the version with a dash, the key with the dot, as OpenRouter does,
and Devin spells GPT versions with dashes too (`gpt-6-1-sol`).
