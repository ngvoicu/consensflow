# ConsensFlow

A native terminal workspace for a lead, its workers, and an optional project manager.

Keep a roster of named agents such as `zeus` and `hyperion`, then ask your lead
to consult them. ConsensFlow opens worker conversations in its own panes and
delivers completed results back to their lead. Each session can also have a PM
in a separate window for research, planning and explanations.

The application supplies `consensflow-lead` or `consensflow-pm` only to the
corresponding role it launches. Workers and ordinary external terminals receive
neither skill. Your harness profiles and native terminal appearance are preserved.

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
cd app && npm install
npm run build -- --config '{"bundle":{"createUpdaterArtifacts":false}}'
# → app/src-tauri/target/release/bundle/dmg/ConsensFlow_<version>_aarch64.dmg
```

The app is ad-hoc code-signed, but not Apple-notarized, so macOS blocks the first launch. Right-click → **Open**
still works on older systems; since Sequoia, Gatekeeper no longer offers it for
unnotarized apps — open it once, let it be refused, then **System Settings →
Privacy & Security → Open Anyway**.

Use **ConsensFlow → Check for Updates…** in the macOS menu for in-app updates.
The app checks quietly after startup and every six hours.
Choose the stable or alpha channel, read the release notes, then download the
update. Installation and restart happen only when you choose them, after closing
all panes across every session. Save or submit native drafts before closing:
ConsensFlow does not infer draft contents or stop working agents automatically.

Update archives have a separate cryptographic signature and include the app,
Node, `cf`, role skills and integration code together. Harness detection, installed
versions, official release checks and observed integration status appear in
**Agents**. Native harness versions never gate launching or reading results.
Older builds require one manual DMG installation to acquire the updater. Channel feeds must be published before
online discovery works; see [update release preparation](docs/updates.md).

Nothing is seeded. Open the app, pick from the ready-made list — `zeus`,
`hyperion`, `athena`, `endymion` … — or define your own with any model string
its harness accepts. The app refreshes its private lead roster context when agents change.
No separate skill installation or update is needed.

The roster lives at `~/.consensflow/agents.json` and is shared by everything
that reads it.

## Who can consult

Claude Code, Codex, Pi and OpenCode can be leads or PMs. Kimi is available as a
worker. A lead can delegate and continue worker conversations; a PM can send to
or read from its own lead only when you request it. Several sessions can work
at once, including sessions sharing the same project folder.

Pi uses a bundled extension loaded only into its ConsensFlow process. It is
prepared automatically if Pi is installed; an installation error is shown with
a retry action. No global Pi extension or settings are changed.

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
Sessions can be renamed or deleted from the sidebar, and several can keep
working at once. Grid view shows at most six panes across two visible rows,
with at most three columns; additional rows scroll vertically.

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
cf results [conversation|@agent]       # completed worker results, with status and preview
cf read <name> [--answer <id>]         # one completed result, first part
cf read <delivery> [--part N]          # follow-up parts use the delivery id
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
the lead runs and reads in full before anything else. Under `manual` there is
no automatic delivery: the lead reads results only when you explicitly ask.
The lead follows the same rule in either mode and does not need to query the
setting: wait for delivery or your request. Delegation or an ongoing task does
not authorize fetching results. Completing the parts of an already delivered
result needs no new request. When asked, `cf read <name>` reads a known
conversation's completed result directly, without a preliminary listing.
`cf results` is for finding a conversation or selecting among requested results.
One request covers those results and their parts, not future polling. Waiting a question
out and polling in a loop are both wrong: an answer the lead has not read is
not a decision you have made, and a policy you set is never changed behind
your back.

The app never reads the pane's screen, because screen text is a picture of an
answer, not an answer. `cf results` shows the lead every completed worker
result — id, status and preview — and `cf read <name>` prints one whole, part
by part, from the app's delivery records. The completions themselves are
recognised from the harness's **own session store** —
codex's rollout file, claude's session jsonl, pi's, opencode's — **read-only,
never written**. `cf say` still exists for typing turns through our own
machinery, and every pane runs with the same environment guards: billing keys
stripped, control variables stripped.

Replies use native input routes for leads and workers. Codex queues messages
by thread ID; OpenCode receives them through its local server. Supported
Claude Code versions 2.1.263 and 2.1.265 use their built-in local peer inbox,
with no development channel or plugin. Unsent input is preserved. Pi waits
until its native editor is empty, then continues automatically. The app keeps
each TUI's original palette, layout and text styling.

When terminal input cannot be verified as clear, incoming messages remain held.
Manual result reads stay available when you request them, and reply policy remains
yours to set. Start a new ConsensFlow session for a new collaboration.

New Codex leads open without a prompt and wait for the human's first message.
Their launch identifier travels in native session metadata; no prompt is
injected. Once Codex persists that session, ConsensFlow binds it by the exact identifier; a recent conversation
in the same folder is never a fallback. Explicit Resume uses the bound session.

New OpenCode sessions use an empty session created through OpenCode's native
API and open its exact ID in the ordinary TUI. They wait for the first human
message; worker tasks are submitted once through the native API after their
TUI server starts, using the selected model. This requires no ConsensFlow
plugin in OpenCode and changes no harness binaries or global settings.

## Outside an app pane

There is no consult outside the app. Without `CONSENSFLOW_APP` — a plain
terminal, a script, a test — `cf run`, `cf say`, `cf attach` and `cf read`
refuse and name the app; nothing streams, nothing queues. The lead's `cf run`
returns after the app opens the pane; task startup continues there. `cf sessions`
shows recorded startup/admission status, and the answer arrives in the lead's
pane later. The app keeps conversation bindings and delivery records under
`~/.consensflow/workspaces/<key>/`. Native harness histories are the source
for complete results even after scrollback is gone. Launch coordination
files may live in the project's `.consensflow/` directory.

Two things are worth being explicit about:

**Agents run with full permissions.** `--dangerously-skip-permissions` for
claude, `--dangerously-bypass-approvals-and-sandbox` for codex, `--auto` for
opencode. An agent is a helper you hand a task to: it reads and writes files and
reaches the network. There is no knob, and that is deliberate — **the protection
is the approval gate on *keeping* its work, not a fence around the run.** The
lead skill keeps worker suggestions within the user's existing authorization.

**Nothing rides along.** An agent sees the brief, the task, and whatever you
hand it with `--handoff-file`. No conversation is stashed or attached
automatically.

## App-private role skills

`consensflow-lead` is generated from the roster in ConsensFlow's private data
folder. The app supplies it only when launching a lead. Native global skill
folders are never written, refreshed, or cleaned by this installer.
The old global `consensflow` skill must be removed manually.

```sh
cf skills status      # inspect owned files
cf doctor             # harnesses, agents, skills, runtime
```

Role skills ship with each application release. There is no separate skill update.
Claude Code, Codex, OpenCode and Pi receive the full assigned role instructions
as native startup context when a lead or PM launches or resumes. No manual skill
invocation is needed; existing native instructions and settings are preserved.
Use **Add project manager** beside a session to choose a PM harness. The PM opens
in its own maximized window; it shares the project folder and communicates with
its lead only through explicit `cf lead send --message-file <file>` and
`cf lead read` requests. It cannot create or control workers.

## Leaving

```sh
cf off            # remove the private installation; keep agents and runs
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
| Role documents | `~/.consensflow/roles/` |
| Private Pi integration | `~/.consensflow/extensions/pi/` |
| Owned-file manifest | `~/.consensflow/skills-manifest.json` |

A leftover `mode.json` from the old three-mode era is ignored; `cf doctor`
reports it once as removable. One root; `CONSENSFLOW_HOME` moves all of it. The `cf`/`consensflow` launchers are placed on PATH. Role documents and Pi
integration files stay under this private root; native global skill folders are
not changed.

## Development

```sh
npm test        # node --test
npm run check   # biome + tests
```

Tests spawn no live agent CLIs and reach no network: harness CLIs are stub
scripts on a fake PATH, git is a shim copying a fixture tree, and every test
runs against a throwaway home.

Specs live in `.specs/`.
