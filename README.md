# ConsensFlow

Named AI agents your coding agent can consult, in every harness on your machine.

You keep a roster — `zeus` is Claude Opus at max effort, `hyperion` is GPT 5.6
Sol at ultra. ConsensFlow generates **one skill** from it and installs that skill
into Claude Code, codex, pi and opencode, which all read the same Agent Skills
format. From then on you say *"ask hyperion whether this migration is safe"* and
your coding agent does the rest: it runs hyperion, waits, and reports the answer
attributed. You never type a command.

**No accounts, no API keys.** Agents run through the harness CLIs you already
have installed and logged in — your Claude subscription, your ChatGPT login,
whatever you configured in pi or opencode. ConsensFlow stores no credentials and
asks for none. The generated commands strip stray `ANTHROPIC_API_KEY` /
`OPENAI_API_KEY` for the one command they run, so a leftover export cannot
silently move a subscription login onto per-token API billing.

There is no daemon and no database. The skill is the product.

## Install

**The app is the installation.** Build it, drag it to Applications, open it.
It carries its own Node runtime and its own copy of ConsensFlow, so nothing has
to be installed first, and everything else — agents, which harnesses consult,
the skills — happens in its window.

```sh
cd app && npm install && npm run build
# → app/src-tauri/target/release/bundle/dmg/ConsensFlow_<version>_aarch64.dmg
```

Unsigned for now, so the first launch needs right-click → **Open**.

Nothing is seeded. Open the app, pick from the ready-made list — `zeus`,
`hyperion`, `athena`, `endymion` … — or define your own with any model string
its harness accepts. **The skill installs itself the moment your first agent
exists, and rewrites itself on every change after.** No install step, no sync
step.

The roster lives at `~/.consensflow/agents.json` and is shared by everything
that reads it.

## Who can consult

A machine runs one path, and choosing it is the install:

```sh
cf use cmux     # every harness (claude, codex, pi, opencode), plus cmux's pane skills
cf use claude   # only Claude Code
cf use pi       # only pi
cf mode         # which one is active, and what it costs
```

A mode is a **scope**, not a different product: all three install the same
generated skill and differ only in who receives it. Switching takes the skill
back from whoever is no longer in scope, so two paths are never live at once. A
harness that ships its own ConsensFlow is left alone rather than given a second
skill with the same name.

ConsensFlow ships exactly one skill — its own. It used to clone cmux's whole
skills tree into every harness too; that era is over, and switching modes now
takes those leftovers back. `cf doctor` names any harness that is in scope but
carrying no skill.

## A consult IS the agent's window

In cmux mode, in a terminal, `cf run` does not print an answer and exit. It
opens the harness's **own interface** — claude's, pi's, opencode's — on a named
conversation, seeded with your task, and stays. You watch the agent work in its
real window; you type follow-ups straight into it; your coding agent follows
along by reading the harness's own session. codex is the one exception: it has
no way to pre-set an interactive session id, so it streams its first answer,
then the same pane becomes `codex resume` on that session.

```
$ cf run @hyperion "is the retry path sound?"
conversation: silver-waves (new)
read it back with: cf catchup silver-waves
…the pane is now hyperion's own window…
```

One agent can hold several conversations at once, which is why they have names:
*"ask ares in bubble-sky about the migration"*. Each gets its own pane.

**A conversation belongs to the session that started it.** Open a new coding
session and its first consult starts a fresh conversation — it never picks up
what the last one left in that directory, however recent. Ones somebody else
started stay reachable by name, which is what `--session` is for.

```sh
cf mint                                # a fresh conversation name, before anything exists under it
cf run @name "<task>"                  # opens (or reopens) THIS session's conversation with it
cf run @name "<task>" --new            # a fresh conversation, its own window
cf run @name "<task>" --new --session <name>  # a fresh one, under the name you minted
cf run @name "<task>" --session <name> # a specific existing one, by name
cf sessions                            # what is alive in this folder
cf catchup <name> [--wait]             # what was said; --wait sits out the next answer
cf attach <name>                       # reopen a conversation's window later, anywhere
cf last <name>                         # the last answer a streamed run left
```

**The harness owns the session; ConsensFlow only remembers which one.** That is
the whole mechanism — no daemon, no database, no long-lived child. Session ids
live in `<config>/workspaces/<key>/threads.json`. claude opens on a uuid we
mint, pi on the conversation's name, opencode tells its store and we read it
there, codex hands its id back on the streamed first turn.

## How your coding agent follows along

Window turns leave no stream for the lead to parse — and it never reads the
pane's screen, because screen text is a picture of an answer, not an answer.
Instead `cf catchup` reads the harness's **own session store** — codex's
rollout file, claude's session jsonl, pi's, opencode's — **read-only, never
written** — and returns nothing rather than failing if a harness has moved its
files. `cf catchup <name> --wait` blocks until the agent's next answer lands
and prints only what is new: that is the lead's way to wait out a thinking
agent. `cf chat` still exists for typing turns through our own machinery, and
every window runs with the same environment guards a one-shot does: billing
keys stripped, cmux control stripped.

## When there is no terminal

A pipe cannot host a TUI. When a program runs the consult — your coding
agent's tool call, a test, `--json` — it streams instead, and streaming is
**foreground and non-optional**: thinking, tool calls and the answer arrive
as they happen, runs are never sent to the background, and there is no flag
to quiet it — only `--json`, which asks for machine-readable output instead.
The run writes its artifacts under
`~/.consensflow/workspaces/<key>/runs/<id>/` — `packet.md`, `transcript.md`,
`result.json`. Nothing is written inside your project. Host modes work this
way for every consult. `transcript-events.js` normalises four engines' event
shapes into one vocabulary, and `transcript.md` is the durable backstop so a
lost scrollback never costs the answer.

Two things are worth being explicit about:

**Agents run with full permissions.** `--dangerously-skip-permissions` for
claude, `--dangerously-bypass-approvals-and-sandbox` for codex, `--auto` for
opencode. An agent is a helper you hand a task to: it reads and writes files and
reaches the network. There is no knob, and that is deliberate — **the protection
is the approval gate on *keeping* its work, not a fence around the run.** The
skill tells your coding agent never to apply or keep an agent's changes without
asking you first.

**Nothing rides along.** An agent sees the brief, the task, and whatever you
hand it with `--handoff-file`. No conversation is stashed or attached
automatically, in any mode.

## The one skill ConsensFlow manages

**`consensflow`** — generated from your roster. Its description names your
actual agents, which is what makes a harness reach for it when you say a name.
In cmux mode it also quotes the four cmux pane commands a lead needs, so
nothing else has to be installed to open, address and find panes.

```sh
cf skills status      # every file ConsensFlow owns: ok, drifted, missing
cf skills update      # regenerate ours; retire anything the cmux-cloning era left
cf doctor             # harnesses, agents, skills, runtime
```

Ownership is a hash manifest (`~/.consensflow/skills-manifest.json`). A file you
edited by hand is **drifted**: ConsensFlow refuses to overwrite or delete it
without `--force`, and never touches a file it did not write. Claude Code's
`settings.json` is never written at all — a hook an older version left there is
reported by `cf doctor` for you to remove.

## Leaving

```sh
cf off            # remove every file it installed. Your agents and runs are kept
cf reset --yes    # the clean slate: those too, and the app's own caches
```

`cf reset` prints what it will destroy and refuses without `--yes`. It does not
delete `ConsensFlow.app` — removing an application is a Finder gesture — and it
never touches a harness's own session store.

## Inside the app

A window around the same roster editor, never a second implementation. The
bundle carries an official Node build and ConsensFlow's own sources, and on
launch runs *its own* copy and points the window at it. Nothing on the machine
is consulted, which is what makes it self-sufficient: an app opened from Finder
inherits almost no PATH.

About 148 MB installed, 48 MB as a dmg — nearly all of it the Node runtime.

The icon was generated by **pygmalion**, the image agent on the roster, from the
site's logo: the roster designing its own app.

## State

| | |
|---|---|
| Roster | `~/.consensflow/agents.json` |
| Conversations | `~/.consensflow/workspaces/<key>/threads.json` |
| Run artifacts | `~/.consensflow/workspaces/<key>/runs/<id>/` |
| Skill manifest | `~/.consensflow/skills-manifest.json` |
| Mode | `~/.consensflow/mode.json` |

One root; `CONSENSFLOW_HOME` moves all of it. The only things written outside
are the generated skill in each harness's own skills directory and the
`cf`/`consensflow` launcher on PATH.

## Development

```sh
npm test        # node --test
npm run check   # biome + tests
```

Tests spawn no live agent CLIs and reach no network: harness CLIs are stub
scripts on a fake PATH, git is a shim copying a fixture tree, and every test
runs against a throwaway home.

Specs live in `.specs/`. `CLAUDE.md` (byte-identical to `AGENTS.md`) holds the
load-bearing rules and the reasons behind them.
