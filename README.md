# ConsensFlow

Named AI agents your coding agent can consult, in every harness on your machine.

You keep a roster — `zeus` is Claude Opus at max effort, `hyperion` is GPT 5.6
Sol at max. ConsensFlow generates **one skill** from it and installs that skill
into Claude Code, codex, pi, opencode and Kimi Code, which all read the same Agent Skills
format. From then on you say *"ask hyperion whether this migration is safe"* and
your coding agent does the rest: it asks from its pane in the app, reports what
is running, and reads the answer when it arrives, attributed. You never type a command.

**No accounts, no API keys.** Agents run through the harness CLIs you already
have installed and logged in — your Claude subscription, your ChatGPT login,
whatever you configured in pi or opencode. ConsensFlow stores no credentials and
asks for none. The generated commands strip stray `ANTHROPIC_API_KEY` /
`OPENAI_API_KEY` for the one command they run, so a leftover export cannot
silently move a subscription login onto per-token API billing.

There is no daemon and no database outside the app. The app owns the panes and
the delivery queue; the harnesses' own session stores stay read-only.

macOS first: the app is built, tested and installed on macOS. Windows and
Linux packaging follow in a later spec; every choice stays compatible with them.

## Install

**The app is the installation.** Download it, drag it to Applications, open it.
It carries its own Node runtime and its own copy of ConsensFlow, so nothing has
to be installed first, and everything else — agents, which harnesses consult,
the skills — happens in its window.

Latest build: **[releases](https://github.com/ngvoicu/consensflow/releases)**
— `ConsensFlow_<version>_aarch64.dmg`, Apple silicon. (Not `/latest`: every
build so far is a prerelease, and GitHub's `latest` skips those.)

Or build your own, which is the same artifact:

```sh
cd app && npm install && npm run build
# → app/src-tauri/target/release/bundle/dmg/ConsensFlow_<version>_aarch64.dmg
```

Unsigned either way, so macOS blocks the first launch. Right-click → **Open**
still works on older systems; since Sequoia, Gatekeeper no longer offers it for
unnotarized apps — open it once, let it be refused, then **System Settings →
Privacy & Security → Open Anyway**.

Nothing is seeded. Open the app, pick from the ready-made list — `zeus`,
`hyperion`, `athena`, `endymion` … — or define your own with any model string
its harness accepts. **The skill installs itself the moment your first agent
exists, and rewrites itself on every change after.** No install step, no sync
step.

The roster lives at `~/.consensflow/agents.json` and is shared by everything
that reads it.

## Who can consult

ConsensFlow has one shape: the app owns the panes (standalone). There are no
modes and no `cf use` — the one generated skill is installed into every
detected harness without a native ConsensFlow. A harness that ships its own
ConsensFlow is left alone rather than given a second skill with the same name.

ConsensFlow ships exactly one skill — its own. `cf doctor` names any harness
that is missing it, and reports a leftover `mode.json` from the old three-mode
era as removable.

## A consult lives in the app

The app is the terminal: it opens every pane itself — a tab is one directory,
one lead and a policy, with worker and shell panes beside it — and lays them
out in a fixed progression. Several tabs may share a directory, each with its
own lead; nothing is shared between them. `cf run` from a lead's pane never opens a window there; it
follows today's continuation rule and the app prints the name it minted:

```
$ cf run @hyperion "is the retry path sound?"
conversation: silver-waves (new)
```

One agent can hold several conversations at once, which is why they have names
— and the name carries the agent, so a row of panes says whose each one is:
*"ask ares in ares-bubble-sky about the migration"*. A sidebar lists every
session ever opened, live or closed, and a closed one resumes from there.

**A conversation belongs to the session that started it.** Open a new coding
session and its first consult starts a fresh conversation — it never picks up
what the last one left in that directory, however recent. Ones somebody else
started stay reachable by name, which is what `--session` is for.

```sh
cf run @name "<task>"                  # continues this session's conversation with it
cf run @name "<task>" --new            # a fresh conversation
cf run @name "<task>" --session <name> # a specific existing one, by name
cf say <name> "<words>"                # a follow-up in the same conversation
cf attach <name>                       # reopen a conversation later, anywhere
cf read <delivery> [--part N]          # a delivered file, in full, part by part
cf catchup <name> --unread             # what has been said since you last looked
```

**The harness owns the session; ConsensFlow only remembers which one.** That is
the whole mechanism — no long-lived child outside the app. The app binds a
lead or worker to its native session with launch-unique evidence, and an
ambiguous binding stays visibly unbound rather than driving delivery.

## How your coding agent follows along

The lead sends and returns, never waits. After a consult or a follow-up it
reports what is running and in which conversation, then takes your next
message. Under `auto` every completed worker answer arrives in the lead's pane
whole — inline when it fits, else as a `cf read <id>` line whose every part
the lead runs and reads in full before anything else. Under `manual` the human
says when the lead reads, with `cf catchup <name> --unread`. Waiting a question
out and polling in a loop are both wrong: an answer the lead has not read is
not a decision you have made, and a policy you set is never changed behind
your back.

The app never reads the pane's screen, because screen text is a picture of an
answer, not an answer. `cf catchup <name> --unread` gives the lead exactly what
has been said since its last look, from the harness's **own session store** —
codex's rollout file, claude's session jsonl, pi's, opencode's — **read-only,
never written**. `cf say` still exists for typing turns through our own
machinery, and every pane runs with the same environment guards: billing keys
stripped, control variables stripped.

## Outside an app pane

There is no consult outside the app. Without `CONSENSFLOW_APP` — a plain
terminal, a script, a test — `cf run`, `cf say`, `cf attach` and `cf read`
refuse and name the app; nothing streams, nothing queues. The lead's `cf run`
returns as soon as the app accepts the task, and the answer arrives in its
pane later. Worker runs keep their artifacts under
`~/.consensflow/workspaces/<key>/runs/<id>/` — `packet.md`, `transcript.md`,
`result.json` — and `transcript.md` is the durable backstop so a lost
scrollback never costs the answer. Nothing is written inside your project.

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
automatically.

## The one skill ConsensFlow manages

**`consensflow`** — generated from your roster. Its description names your
actual agents, which is what makes a harness reach for it when you say a name.

```sh
cf skills status      # every file ConsensFlow owns: ok, drifted, missing
cf skills update      # regenerate ours
cf doctor             # harnesses, agents, skills, runtime
```

Ownership is a hash manifest (`~/.consensflow/skills-manifest.json`). A file you
edited by hand is **drifted**: files ConsensFlow owns regenerate from the
roster when the app opens, so hand edits to owned files do not survive — while
a file it never owned is never touched. Claude Code's
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

The app is the terminal, not a window around the roster. It hosts every pane
itself — a PTY per pane, drawn by the page — and the **Agents** button opens
the roster editor as a full-window modal, closed with its close button or Escape.
The bundle carries an official Node build and ConsensFlow's own sources, and on
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

A leftover `mode.json` from the old three-mode era is ignored; `cf doctor`
reports it once as removable. One root; `CONSENSFLOW_HOME` moves all of it. The only things written outside
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

Specs live in `.specs/`.
