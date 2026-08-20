# consensflow-cmux

Named AI participants for every coding agent on your machine.

You create a roster — say `zeus` (Claude Opus, max effort) or `hyperion`
(GPT 5.6, ultra) — and ConsensFlow generates one **skill** from it and
installs that skill into every coding agent you have: Claude Code, codex, pi
and opencode all read the same Agent Skills format. From then on, any agent
in any project understands "ask hyperion whether this migration is safe": it
runs hyperion's exact one-shot command, waits, and reports the answer,
attributed.

**No accounts, no API keys.** Participants run through the agent CLIs you
already have installed and logged in — your Claude subscription, your ChatGPT
login, whatever providers you configured in pi or opencode. ConsensFlow
stores no credentials and asks for none. (The generated commands even strip
stray `ANTHROPIC_API_KEY`/`OPENAI_API_KEY` shell variables for the one
command they run, so a leftover export can't silently switch a subscription
login to per-token API billing.)

There is no daemon and no database — the skill *is* the product. ConsensFlow
manages the roster and keeps the skills current, including cmux's own skills.

## Install

Node ≥ 20:

```sh
npm install -g ngvoicu/consensflow-cmux
cf setup
```

`cf setup` detects your agents and installs the cmux skills (see below).
Participants are yours to create — nothing is seeded:

```sh
cf ui
```

opens a minimal browser editor (Ctrl-C to stop). Each tool comes with a
**list of ready-made participants** — one click adds `zeus` (Claude Opus 5 at
max effort), `hyperion` (GPT 5.6 Sol at ultra), `endymion` (Kimi K3, 1M
context), `prometheus` (GLM 5.3) … — or define a custom one with any model
string its runtime accepts. **The skill installs itself into every agent the
moment the first participant exists, and rewrites itself on every add, edit
or remove after that.** No install step, no sync step.

The CLI does the same job if you prefer it:

```sh
cf catalog                  # the ready-made participants, per tool
cf participant add zeus     # a catalog name is enough — model and effort come with it
cf participant add nemo --runtime codex --model gpt-5.6-terra --effort xhigh   # or roll your own
cf participant list | edit | remove …
```

Model identifiers are passed through verbatim, so anything your CLI accepts
works — the catalog is a convenience, never a constraint.

**The roster is shared.** It lives at `~/.consensflow/participants.json` —
the very file the `consensflow-cc` plugin and `consensflow-pi` extension
already use. One roster, three consumers: edit it here (UI or CLI) and cc
and pi see the change on their next run; if you already used either, `cf
setup` finds your participants and installs the skill immediately — nothing
to import. v3 preserves every field it doesn't understand, so cc/pi-specific
settings survive round-trips. And it heals: if cc or pi change the roster
behind v3's back, the next `cf` invocation notices (a stored roster hash)
and regenerates the installed skill — and the skill itself tells agents to
run `cf skills update` when a name is missing from its table.

## The skills ConsensFlow manages

Two sources, one owner:

- **`consensflow`** — generated from your roster: a table of exact commands,
  one per participant, with its model and effort baked in. Participants run
  in the agent's working directory (so they read project files themselves)
  with their own CLI's default permissions — ConsensFlow never generates a
  permission-bypass flag.
- **cmux's skills** — fetched by shallow-cloning
  [`manaflow-ai/cmux`](https://github.com/manaflow-ai/cmux) (its `skills/`
  tree: core, workspace, browser, settings, …) and installed the same way,
  recorded as `cmux@<commit>`. They teach agents pane control; skip with
  `cf setup --no-cmux`.

```sh
cf skills status      # every file ConsensFlow owns: ok, drifted, missing
cf skills update      # regenerate ours; re-fetch cmux's at the latest commit
cf skills uninstall   # remove exactly what the manifest owns, nothing else
cf doctor
```

`cf ui` does this part too: it shows which agents were found (and which
already have their own ConsensFlow), how many files are installed and at
which cmux commit, and offers **Install / update skills** and **Remove
installed skills**. Those are named operations — the page has no endpoint
that runs a command you type, by design.

Ownership is a hash manifest (`~/.config/consensflow/skills-manifest.json`).
A file you edited by hand is **drifted**: ConsensFlow refuses to overwrite or
delete it without `--force`, and it never touches files it didn't write.

## Which ConsensFlow?

- **This repo (`consensflow-cmux`)** — the one to install: works with all
  four agents, one npm command; also manages cmux's skills.
- [`consensflow-cc`](https://github.com/ngvoicu/consensflow-cc) — a Claude
  Code **plugin** (installed through Claude Code, not npm) that additionally
  hands your *current conversation* to the participant as context.
- [`consensflow-pi`](https://github.com/ngvoicu/consensflow-pi) — the same
  idea as a **pi extension** (installed through pi, not npm).

All three share the same participants file, so they always agree on who
your participants are — and they don't stack up on one agent. Where a host
already ships its own ConsensFlow (Claude Code via the plugin, pi via the
extension), `consensflow-cmux` leaves it alone rather than adding a second
skill with the same name, and retires any copy it installed earlier. Its own
skill goes where nothing else provides one — codex and opencode, plus any
host without the native integration. `--all` overrides if you want ours
everywhere.

## State

The roster: `~/.consensflow/participants.json` (shared with cc/pi). The
manifest: `~/.config/consensflow/skills-manifest.json` (override with
`CONSENSFLOW_HOME`). Leave completely with `cf skills uninstall &&
npm uninstall -g consensflow-cmux`.
