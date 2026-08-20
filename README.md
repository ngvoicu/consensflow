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
manages the roster and keeps the skills current — plus cmux's own skills, if
you run the cmux path.

## Install

**The app is the installation.** Download or build `ConsensFlow.app`, drag it
to Applications, open it — that is all. It carries its own Node runtime and
its own copy of ConsensFlow, so nothing needs to be installed first, and
everything else (participants, skills, host integrations, the mode this
machine runs) is done from its window.

```sh
cd app && npm install && npm run build
# → app/src-tauri/target/release/bundle/dmg/ConsensFlow_<version>_aarch64.dmg
```

It is unsigned for now, so the first launch needs right-click → **Open**.

<details>
<summary>Prefer the terminal?</summary>

The same tool is an npm package — this is the developer path, not the one a
new machine needs:

```sh
npm install -g ngvoicu/consensflow    # Node ≥ 20
consensflow use cmux
```
</details>

`cf setup` detects your agents and, in cmux mode, installs the cmux skills
(see below).
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
  recorded as `cmux@<commit>`. They teach agents pane control, so they come
  with `cmux` mode and only that one.

```sh
cf off                # take it all back; participants are kept
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

## One mode at a time

A machine runs exactly one ConsensFlow path, and `consensflow use` chooses it:

```sh
consensflow use cmux     # cmux (pi, cc, codex, opencode) — every agent can consult
consensflow use claude   # only Claude Code consults — with your conversation as context
consensflow use pi       # only pi consults, the same way
consensflow mode         # which one is active, and what it means
```

The modes are named after the three things they are: the Claude Code
integration, the pi extension, and the cmux-wide path that teaches every
agent at once. **cmux's own skills — the ones that let an agent drive panes,
workspaces and browser surfaces — come with `cmux` mode and only that one**;
there they are part of the install, not an option. Consulting through a host
runs a subprocess and never touches a pane, so `claude` and `pi` install none
and take back any they find. If you are offline when you switch to cmux, the
mode still applies and says the cmux skills are pending (`consensflow skills
update` fetches them later).

Switching removes what the previous mode installed, so two paths are never
live at once. The trade is stated every time you switch and every time you
ask: in `claude` mode, **codex and opencode have no ConsensFlow at all** —
that's the price of the deeper path, and `cmux` mode is how you take it
back. ConsensFlow only ever removes what it installed; an integration you
put there another way is reported and left alone.

## Host integrations

Two coding agents can do more than run a one-shot command: they can hand the
participant **your live conversation** as context. That deeper path ships as
a payload per host, and this manager installs it — you never install them
separately:

```sh
consensflow install claude    # wires the Claude Code path through user config
consensflow install pi        # drives pi's own `pi install`
consensflow install all
consensflow hosts             # what's installed where, at which commit
consensflow uninstall claude  # removes exactly what it wrote
```

`install claude` puts the payload in `~/.config/consensflow/hosts/claude`
(never inside Claude Code's own directories) and wires it up through
documented user config: a skill, a `/consensflow` command, and hook entries
merged into `settings.json` — your other settings and hooks are left alone,
and the file is backed up before each write. `install pi` shells out to pi's
supported CLI rather than editing pi's state by hand.

Both payloads live in this repo (`hosts/claude`, `hosts/pi`) and share one
engine (`hosts/lib`), so there is nothing to install from anywhere else and
nothing to keep in sync.

Every host reads the same participants file, so they always agree on who
your participants are — and they don't stack up on one agent. Where a host
already ships its own ConsensFlow (Claude Code via the plugin, pi via the
extension), `consensflow-cmux` leaves it alone rather than adding a second
skill with the same name, and retires any copy it installed earlier. Its own
skill goes where nothing else provides one — codex and opencode, plus any
host without the native integration. `--all` overrides if you want ours
everywhere.

## What is inside the app

It is a window around the same editor, never a second implementation. The
bundle carries an official Node build (a Tauri sidecar) and ConsensFlow's own
sources, and on launch it runs *its own* copy — `node cf.mjs ui --json
--no-open` — then points the window at it. Nothing on the machine is
consulted, which is what makes the app self-sufficient: a .app opened from
Finder inherits almost no PATH, so depending on an installed CLI was fragile
anyway. The editor exits when the app does, because the app holds a pipe to
its stdin.

About 148 MB installed, 48 MB as a dmg — nearly all of it the Node runtime
that makes it standalone.

The icon was generated by **pygmalion**, the image participant on the
roster, given the site's logo as reference — the roster designing its own
app.

## The engine, and what a consult looks like

The host integrations share one engine (`hosts/lib`) — there is a single copy
now, not one per host. What it guarantees:

- **Streaming is foreground and non-optional.** A participant's thinking,
  tool calls and answer stream into your session as they happen; runs are
  never sent to the background, because a consult you cannot watch is a
  consult you cannot trust. pi surfaces the same stream through its
  `onUpdate` channel.
- **A durable backstop.** Every run also lands in `transcript.md` inside the
  workspace's ConsensFlow directory, so a closed pane or a lost scrollback
  never loses the answer. `transcript-events.js` normalizes each engine's
  event shapes into one vocabulary.
- **The roster is shared.** Every host reads the same
  `~/.consensflow/participants.json`, so participants defined once are
  available everywhere.

## History

`consensflow-cc` (the Claude Code plugin) and `consensflow-pi` (the pi
extension) were separate repositories until 2026-08-20. They are archived:
their payloads live here under `hosts/`, sharing one engine, and this app
installs them. Nothing is maintained outside this repo.

## State

The roster: `~/.consensflow/participants.json` (shared with cc/pi). The
manifest: `~/.config/consensflow/skills-manifest.json` (override with
`CONSENSFLOW_HOME`). Leave completely with `cf skills uninstall &&
npm uninstall -g consensflow`.
