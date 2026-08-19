# ConsensFlow

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
npm install -g ngvoicu/consensflow
cf setup
```

`cf setup` detects your agents and installs the cmux skills (see below).
Participants are yours to create — nothing is seeded:

```sh
cf ui
```

opens a minimal browser editor (Ctrl-C to stop). Add a participant — name,
runtime, model, optional effort — and **the skill installs itself into every
agent the moment the first participant exists, and rewrites itself on every
add, edit or remove after that.** No install step, no sync step.

The CLI does the same job if you prefer it:

```sh
cf participant add zeus --runtime claude --model claude-opus-5 --effort max
cf participant list | edit | remove …
```

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
  one per participant, with model, effort and permission baked in.
  `--dangerously-*` flags appear only on participants you explicitly stored
  as `full-auto`. Participants run in the agent's working directory, so they
  read project files themselves.
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

Ownership is a hash manifest (`~/.config/consensflow/skills-manifest.json`).
A file you edited by hand is **drifted**: ConsensFlow refuses to overwrite or
delete it without `--force`, and it never touches files it didn't write.

## Which ConsensFlow?

- **This repo (`consensflow`)** — the one to install: works with all four
  agents, one npm command.
- [`consensflow-cc`](https://github.com/ngvoicu/consensflow-cc) — a Claude
  Code **plugin** (installed through Claude Code, not npm) that additionally
  hands your *current conversation* to the participant as context.
- [`consensflow-pi`](https://github.com/ngvoicu/consensflow-pi) — the same
  idea as a **pi extension** (installed through pi, not npm).

All three share the same participants file, so they always agree on who
your participants are.

## State

The roster: `~/.consensflow/participants.json` (shared with cc/pi). The
manifest: `~/.config/consensflow/skills-manifest.json` (override with
`CONSENSFLOW_HOME`). Leave completely with `cf skills uninstall &&
npm uninstall -g consensflow`.
