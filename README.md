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
versions and official release checks appear in **Harnesses**. Native harness
versions never gate launching or reading results.
Older builds require one manual DMG installation to acquire the updater. Channel feeds must be published before
online discovery works; see [update release preparation](docs/updates.md).

Nothing is seeded. Open **Agent library** in the app and pick from the ready-made list — `zeus`,
`hyperion`, `athena`, `endymion` … — or define your own with any model string
its harness accepts. The app refreshes its private lead roster context when agents change.
No separate skill installation or update is needed.

**Your agents** and **Agent library** are separate top-level screens, each with independent search, category filters
and grouping/sorting controls with the same options. Both open with **Model and reasoning**
cards by default; **Clear filters** restores this view. **None** and **Harness** remain
available. Shared cards show common tags, descriptions and scores once, with individual harness/provider
choices and Add/Remove or Edit/Remove controls beneath. This applies to every repeated
model/reasoning combination; differing agent metadata stays on its own row. Added presets stay visible
as Already added, with an adjacent Remove button that removes the saved copy while
keeping the library choice available. Default sorting uses model family and tier,
then Ultra → Max → Xhigh → High → Medium → Low (followed by Minimal, Off and defaults).
Claude families appear as Fable → Opus → Sonnet → Haiku; GPT as Astra → Sol → Terra → Luna.
Claude, GPT and Gemini stay together first, followed by the other model families.
Each agent shows its categories as individual pills, separately from its provider route.
Muse Spark 1.3 Contributor and Contributor Free share the Muse 1.3 model card;
this grouping follows the provider's description of Contributor as a pricing/data-use
[tier](https://openrouter.ai/meta/muse-spark-1.3-contributor). Contributor and Free
remain on their provider rows, including the disclosed use of prompts/replies for
[Meta model training](https://opencode.ai/docs/go/#privacy). Execution model IDs
and separate Add/Remove identities are preserved. Gemini 3.1 Pro Preview is retired.
Lead and PM recommendations require Fable, Opus, Astra or Sol at Xhigh or above
where supported. Reviewer / second opinion recommendations start at Medium for
known coding models. These tags do not change how roles are launched. Sol Low and
Medium are available on Codex, Pi and OpenCode, using their existing provider routes. Pygmalion uses Codex Images through your existing Codex login.
[OpenAI currently documents GPT Image 2](https://learn.chatgpt.com/docs/image-generation)
for built-in Codex image generation; this route does not select GPT Image 2.5.

Kimi Code is K3-only; K2.7 Code and Highspeed have been removed from the library.
Ilmarinen selects **Max**. K3 also supports **Low** and **High** through Edit or
custom-agent creation. These settings reach the Kimi process through its
[supported environment control](https://www.kimi.com/code/docs/en/kimi-code-cli/configuration/env-vars.html),
without changing native configuration or credentials. An unset effort reads
**Kimi setting** and follows Kimi's configuration. Existing saved copies keep
their selection until you use **Update** or **Edit**.

The roster lives at `~/.consensflow/agents.json`. Each saved agent includes its
configuration, description and the model, provider route, categories and Good for
details shown in the app, plus available benchmark scores and their provenance.
Editing an agent refreshes those details for its actual model and reasoning effort. The file is
created when you add an agent; an empty installation seeds nothing.

Artificial Analysis scores appear as pills, with explanations and tested settings
under **Benchmark details**. **Sort by** offers metrics with available scores;
missing scores sort last, and hallucination rate sorts lowest first. Grouping
stays independent. Scores describe AA’s test configuration, not measured
ConsensFlow harness performance. When AA publishes a model-level result without a
reasoning level, its scores carry **AA reasoning level not specified**. Named
reasoning settings and ambiguous model snapshots never borrow another tested
configuration. Codex Images has no claimed underlying model score.

For the optional AA integration, store your own key in
`$CONSENSFLOW_HOME/artificial-analysis-key` (default
`~/.consensflow/artificial-analysis-key`) with file permissions `0600`.
Keep it outside the repository and app bundle. The local backend calls AA and
caches scores daily in `artificial-analysis-cache.json`; credentials never enter
the browser or `agents.json`. Failed refreshes retain dated cached scores and
respect quota retry times. Free access provides Intelligence, Coding and Agentic
indexes. Higher access enables supported individual benchmarks, including
Terminal-Bench v4.0, hallucinations/knowledge accuracy, instruction following,
long-context reasoning and professional work. See
[AA API documentation](https://artificialanalysis.ai/data-api/docs) for current
access and attribution requirements. No key or live AA dataset is distributed
with ConsensFlow.

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

Every completed worker reply or advisor finding is kept in a private result inbox.
The selected Lead or PM conversation fetches complete numbered parts when ready.
The same receipt rules apply to Claude Code, Codex, Pi and OpenCode: only full
content in the native conversation confirms receipt. A transport response or
opening Results in the app does not mark a result received. Unconfirmed writes
are retained and are never automatically replayed.

The Results view shows every reply and its full text, including replies from
previous native conversations. Each worker/advisor and its coordinator shows
an unconfirmed count. Manual policy keeps results available for explicit reading
or collection. Native session changes retire the previous receiver while keeping
its receipt evidence. ConsensFlow uses small process-local native integrations,
prepared under its private home; it does not change global harness settings or
write bookkeeping into project folders.

PM and Lead have separate grids within one project session. Advisors return
research, review and existing-test findings to their owning PM; only the PM
incorporates that advice into specifications. Switching grids keeps both groups
running and preserves each group's navigation and terminal output.

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

## Inside the app

**Agents** manages the worker roster. **Harnesses**, beside it, shows installed
coding tools, versions and available updates, plus a retry action if Pi setup
fails. Harness checks run when that screen is first opened or refreshed.

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
reports it once as removable. One root; `CONSENSFLOW_HOME` moves all of it. The `cf`/`consensflow` launchers live under `~/.consensflow/bin`; add that directory to your shell PATH to use them outside the app. App panes receive the bundled CLI automatically. Role documents and Pi
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
