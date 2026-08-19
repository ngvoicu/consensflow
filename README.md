# ConsensFlow v3

Named AI participants for every coding agent on your machine.

You keep a roster — `zeus` (Claude Opus, max effort), `hyperion` (GPT 5.6 Sol,
ultra), `endymion` (Kimi K3, 1M context)… — and ConsensFlow generates one
**skill** from it and installs that skill into every coding agent you have:
Claude Code, codex, pi and opencode all read the same Agent Skills format.
From then on, any agent in any project understands "ask hyperion whether this
migration is safe": it runs hyperion's exact one-shot command, waits, and
reports the answer, attributed.

There is no daemon, no database, no delegation engine — the skill *is* the
product. ConsensFlow manages the roster and keeps the skills current
(including cmux's own skills, which teach agents pane control).

## Install

```sh
npm install -g <git-url-or-checkout-path>   # Node ≥ 20; installs `cf` and `consensflow`
cf setup
```

(It's a Node tool, so npm — not pipx — is the installer.)

`cf setup` is idempotent and does everything:

1. Imports your ConsensFlow v1 roster (`~/.consensflow/participants.json`)
   if the v3 roster is empty — efforts included.
2. Generates the consensflow skill from the roster and installs it into every
   detected agent (`~/.claude/skills`, `~/.codex/skills`,
   `~/.config/opencode/skills`, `~/.pi/agent/skills`).
3. Fetches and installs cmux's own skills (`--no-cmux` to skip).

Open a fresh agent session anywhere and say "ask zeus …".

## Managing the roster

```sh
cf ui                                        # minimal browser editor, Ctrl-C to stop
cf participant add freya --runtime codex --model gpt-5.6-terra --effort xhigh
cf participant list
cf participant edit freya --model gpt-5.6-sol
cf participant remove freya
cf participant import-v1                     # re-import from v1 any time
```

Every change regenerates the installed skill everywhere, immediately.

## Managing the skills

```sh
cf skills status      # every file ConsensFlow owns: ok, drifted, missing
cf skills update      # regenerate ours; re-fetch cmux's if installed
cf skills uninstall   # remove exactly what the manifest owns, nothing else
cf doctor
```

Ownership is a hash manifest (`~/.config/consensflow/skills-manifest.json`).
A file you edited by hand is **drifted**: ConsensFlow refuses to overwrite or
delete it without `--force`, and it never touches files it didn't write.

## How the skill works

The generated `SKILL.md` carries a table of exact commands, one per
participant — model, effort and permission baked in:

```
env -u OPENAI_API_KEY codex exec --skip-git-repo-check -m gpt-5.6-sol -c model_reasoning_effort="ultra" "<question>"
```

The `env -u …_API_KEY` guards keep subscription logins from silently
switching to API-key billing. `--dangerously-*` flags appear only on
participants you explicitly stored as `full-auto`. Participants run in the
agent's working directory, so they read project files themselves.

The prose was live-proven before the generator existed: fresh claude and
codex sessions each consulted a participant correctly on the first try.

## State

Everything lives in `~/.config/consensflow/` (override `CONSENSFLOW_HOME`):
`participants.json` and `skills-manifest.json`. Uninstall completely with
`cf skills uninstall && npm uninstall -g consensflow` and delete that folder.
