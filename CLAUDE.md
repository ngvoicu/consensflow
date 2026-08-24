# CLAUDE.md

Byte-identical to `AGENTS.md` — keep both in sync (workspace convention).

## What this is

ConsensFlow (the manager, npm `consensflow`): skills-first. A zero-dependency Node ESM npm package
(`cf`/`consensflow` bins, no build step) that manages a roster of named AI
agents and generates/installs **one skill** teaching every harness
(claude, codex, pi, opencode, kimi — all read the same Agent Skills `SKILL.md`
format) how to consult them via exact CLI commands — one-shot in the host
modes, named resumable conversations in each agent's own window in `cmux`
mode. ConsensFlow ships exactly ONE skill — its own; the cmux-skills cloning
era is over and `syncCmuxSkills` only takes its leftovers back. There is
deliberately **no delegation engine** — no daemon, no SQLite, no panes, no state machine (the
v2 engine that had one was retired and deleted 2026-08-19).

## Consulting agents while working here

This machine runs `cmux` mode: a consult opens the agent's OWN window in its
own pane. When you consult an agent, go through the **consensflow skill** —
open a pane per conversation as it says, send follow-ups into it as plain
text, read with `cf catchup <name> --wait`, and never run `cf run` in your own
pane (in a terminal it would replace it with the agent's window). The
`cf run @name "<task>"` lines below document the product; they are not a
license to skip the skill.

## Source map

| Path | Owns |
|---|---|
| `bin/cf.mjs` | All verbs: setup, use, run, mint, chat, attach, catchup, sessions, last, mode, off, reset, agent …, skills …, ui, doctor. `reset` refuses without `--yes` and prints what it would destroy — the refusal is the preview. Setup never seeds agents; a machine that ran cc/pi already has the shared roster, so setup installs the skill straight from it. Every roster mutation installs-or-regenerates the skill, for whoever the mode puts in scope (`skillTargets` → `scopeTargets`) — the first add installs it, the rest regenerate it |
| `src/roster.js` | Roster = the SHARED v1 file `~/.consensflow/agents.json` (cc + pi read/write it too): v1-schema-faithful mapping (kind↔runtime, thinking/effort↔effort, toolsPolicy↔permission), unknown fields preserved, unsupported kinds listed+marked, never dropped. **One root, one meaning**: roster, state and workspaces all live in `~/.consensflow`, or under `CONSENSFLOW_HOME` when it is set. The state used to sit under XDG while the roster sat here, and the variable meant a different directory to each half — a machine set up that way is moved into the one root on first run (`migrateStateRoot`) |
| `src/catalog.js` | A **view over `hosts/lib/presets.js`** — one catalog, not two (they disagreed on five names until 2026-08-21) — plus each CLI's real effort levels. All 57 presets are offered, grouped by harness (claude 8, codex 4, pi 21, opencode 20, kimi 3, image 1). `cf agent add <name>` resolves through it and records `preset` on the row, which is what makes `cf agent sync` and the UI's Update button possible |
| `src/skill.js` | SKILL.md generation — the prose IS the product. One command for every agent (`cf run @name "<task>"`), so the table says who each agent is rather than what to type. The front-matter `description` is mode-aware, because it is the only part a lead reads before deciding whether to open the rest |
| `src/harnesses.js` | Harness detection (CLI on PATH) + per-harness skills dir (honours `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `XDG_CONFIG_HOME`, `KIMI_CODE_HOME`). Five harnesses since 2026-08-24: kimi's CLI is `kimi` but its home is `.kimi-code` |
| `src/terminal.js` | The `cf`/`consensflow` launcher on PATH, in **every** mode — the skill teaches `cf run`, so it is part of the path, not a cmux extra. It is also the only installed file that names a runtime absolutely, which is what `terminalRuntime` reads for `cf doctor` |
| `src/manifest.js` + `src/install.js` | Hash-manifest ownership: install/update/status/uninstall; drift is sacred |
| `src/host-payloads.js` | **Take-back only, never installs.** Everything the payload era left on a machine: the recorded claude files, the `/consensflow` command (by its front-matter marker), the world-readable `settings.json.consensflow.bak`, the `<config>/hosts` payloads, `hosts.json`, and a pi extension removed through pi's own CLI. Claude Code's `settings.json` is never written — a hook an older version left is *reported* by `cf doctor` |
| `src/sync.js` | The roster→skill path: `skillTargets` (delegates to `scopeTargets`, so one rule decides scope), `refreshInstalledSkill` on every mutation, `healSkillIfStale` when another tool edits the roster behind us, and `skillGaps` — a POSITIVE check of who should carry the skill against who does, because `installSkill` refuses an unowned path and records nothing, which once left opencode silently consulting nothing while everything else looked healthy |
| `src/mode.js` | The one-path-per-machine invariant. A mode is a **scope over one generated skill** — who gets it — plus whether cmux's pane skills come along. `syncGeneratedSkill` is the single install path for all three |
| `src/ui.js` | Ephemeral loopback roster editor (random bearer token, no daemon): roster CRUD, catalog quick-adds, the mode switcher (`POST /api/mode`), and the skills panel (`GET /api/system`, `POST /api/skills/install`, `POST /api/skills/uninstall` — uninstall needs `confirm:true`), and the two danger buttons: `POST /api/off` and `POST /api/reset`, both gated on `confirm:true`. Named operations only: never an endpoint that executes a supplied command |
| `skill/SKILL.md` | The hand-written v0 the generator's template mirrors. Reference only — nothing installs it; `src/skill.js` is what reaches a machine |

## The engine (`hosts/lib`)

| Path | Owns |
|---|---|
| `runners.js` | `buildRunnerInvocation` (per-harness argv, one-shot vs conversation), `runAgent` (spawn, stream, capture the session id, write the run dir), `extractSessionId`, `interactiveStart`/`interactiveResume` (the harness's OWN window, fresh or resumed, optionally seeded with a first message), and `childEnv` — the one rule for what an agent process may see (billing keys and `CMUX_SOCKET*` stripped, child marker set), applied to windows and one-shots alike |
| `packets.js` | The packet. No persona, ever. `continuing` drops the scene-setting for a follow-up; `conversational` invites the agent to ask rather than guess |
| `threads.js` | Named conversations per workspace (`threads.json`), the name generator — `ilmarinen-quartz-valley`: the agent, then two everyday words, so a pane tab says whose window it is and "ask ares in ares-bubble-sky" cannot read as two agents — and `leadId` — who a conversation belongs to, read from the environment and never from `process.env` directly |
| `harness-transcript.js` | Reads each harness's OWN session store so window turns are visible to the lead. Read-only; empty rather than throwing when a layout moves. Also `discoverOpencodeSession` — the id opencode minted, found by directory and birth time, because it is the one harness that neither takes an id nor prints one |
| `state.js` | The one root, workspace keys, run dirs, `writeJsonAtomic` (shared, one copy) |
| `transcript.js` + `transcript-events.js` | Normalising four engines' event shapes into one vocabulary |
| `presets.js` | The 57 catalog presets — the single source `src/catalog.js` is a view over |
| `image.js` + `image-run.js` | The image path: gpt-image-2, drawn by ASKING codex rather than imitating it. Calling the responses endpoint ourselves is refused now — a ChatGPT login is never handed the image tool — but codex's own sessions still are, so `runImageAgent` is one `codex exec` with an instruction and a path. The file on disk is the proof: a run that wrote none is `ok: false` with the reason recorded, because an empty run directory used to look exactly like work in progress |
| `handoff.js` · `workflows.js` · `utils.js` · `codex-auth.js` | Transcript serialisation, the run entry point, ids/parsing, the ChatGPT token |

## The desktop app

`app/` (Tauri v2, Rust) is a window around `cf ui`, never a second editor.
Load-bearing facts, each learned by running the built bundle:

- **The app is self-contained**: `app/scripts/prepare-sidecar.mjs` downloads an
  official Node build (pinned) into `src-tauri/binaries/node-<triple>` and
  copies `bin/ src/ hosts/ skill/` into `src-tauri/resources/cli`. The Rust
  side runs that pair and consults nothing on the machine. Never copy the
  system Node: package-manager builds link machine-local dylibs.
- macOS ATS blocks cleartext http in WKWebView → `bundle.macOS.exceptionDomain
  = "localhost"` and the URL must say `localhost`, not `127.0.0.1`.
- Build the window AT the address (`WebviewUrl::External`); navigating after
  the fact fails silently.
- A .app launched from Finder has a minimal PATH — `locate_cli()` tries PATH,
  known install locations, then the login shell.
- The editor is tied to the app's lifetime by a pipe on its stdin
  (`isPipe` accepts a socketpair, which is what Node's `'pipe'` is on macOS);
  `cf ui` run by hand keeps a terminal/dev-null stdin and is unaffected.

## The merged layout

`hosts/lib` is THE engine, and now the only thing under `hosts/`. It was two
hand-synced copies in two repos until 2026-08-20, then one copy shared by two
payloads (`hosts/claude`, `hosts/pi`) until 2026-08-23, when the payloads went
and `src/` began importing it directly. Nothing is copied anywhere at install
time, so there is no `${CONSENSFLOW_HOST_ROOT}` to rewrite and no second CLI to
keep in step — the manager is the only caller.

## Load-bearing rules

- **One mode per machine** (`src/mode.js`): `claude` | `pi` | `cmux` (named after the three products; `standalone` is accepted as the old name for `cmux`),
  recorded in `<config>/mode.json`. A mode is a **scope**, not an integration:
  every mode installs the same generated skill through the same
  `syncGeneratedSkill`, and differs only in which harnesses receive it. Anyone
  out of scope gives it back, so two paths can never be live at once.
  `cf skills install` follows the same scope; it refuses only when no mode has
  been chosen, which is how ConsensFlow used to appear in harnesses nobody had
  picked. No mode installs cmux's skills any more: the cloning era shipped ~300
  files of cmux-development docs into four skills budgets for the sake of
  three pane commands, and the generated skill now quotes everything a lead
  needs (four commands). `syncCmuxSkills` is take-back only — it retires
  cmux-sourced manifest files and the checkout cache, in every mode, never
  touches the network, and honours drift like any owned file. Every entry
  point states the cost of a host mode (codex and opencode get nothing).

- **A conversation is the harness's, we only remember which one.** In cmux
  mode a consult resumes the harness's own session (`--resume`, `exec resume`,
  `--session-id`, `--session`) instead of the one-shot flags, keyed by a named
  conversation per (workspace, agent) in `<config>/workspaces/<key>/threads.json`.
  Three states, not two: no session object is a one-shot and REFUSES a session;
  an object with no id is the first run and must still SAVE one (passing
  `--ephemeral` there captured an id for a session never written); an id
  resumes. `codex exec resume` takes no `-C` — it filters by the process's own
  cwd. `cf chat` is the same machinery behind a prompt: one typed line, one
  turn, no daemon. Nothing pushes: a turn the user types is visible to the lead
  only when it looks (`cf sessions`, `cf last`). `cf attach` reopens a
  conversation's window later, anywhere
  (`interactiveResume` — `codex resume`, not `codex exec resume`; the first is
  the interface, the second the one-shot), and its `--print` form carries the
  billing guard as prose because the line runs in someone else's shell. A
  follow-up in a live conversation sends only the new message (`continuing`) —
  the agent already has the workspace and the how-to-work from turn one, and
  repeating them buried the question. And because the reply now reaches the
  same agent, a conversational packet tells it to ASK when the task is
  ambiguous instead of guessing, which a one-shot could never do: there, a
  question reached a stranger who had never heard it. `cf catchup` closes the
  last gap: turns the user takes in an attached window leave no run of ours, so
  `hosts/lib/harness-transcript.js` READS each harness's own session store
  (codex rollout jsonl, claude's file named for the session, pi's uuid-suffixed
  jsonl, opencode's message+part pair) — never writes, and returns an empty
  list rather than throwing when a harness has moved its files. It strips the
  environment's own injected `<…>` blocks and unwraps our packet to the
  question inside, because a transcript should show what a person would say was
  said. The screen is never read: it is a picture, not the conversation.

- **A conversation belongs to the lead that started it.** Continuing only
  helps when the one continuing is the one who was there. "The agent's most
  recent conversation in this directory" was everybody's, so a brand-new
  Claude Code session asking for a joke became turn 4 of an unrelated one
  (live, 2026-08-24). Every row now records a `lead`, and a consult takes only
  its own. `leadId(env)` reads it from the environment: the harness's own
  session id first (`CLAUDE_CODE_SESSION_ID`), then the window
  (`CMUX_SURFACE_ID`, `ITERM_SESSION_ID`, `TERM_SESSION_ID`) — the harness id
  wins because a new session in the SAME pane is a new lead and the pane id
  would not have moved. A lead it cannot name is `null`, which matches no row:
  unidentified is nobody, never everybody, so being wrong costs one fresh
  conversation instead of handing over a stranger's. Rows written before this
  carry no `lead` and are never adopted — reachable by name, owned by nobody.
  **Spawning is scoped, joining is not**: `cf chat`, `cf last`, `cf catchup`
  and bare `cf attach` fall back to the agent's most recent conversation
  whoever started it, because the user typing in an agent's pane is a different
  lead by every measure we have and refusing them their own conversation would
  be absurd. `--session <name>` is never scoped either: explicit is the user
  saying which one they mean. `resolveConversation` is the single rule — `cf
  run` carried a second copy of it that never learned anything, which is how
  one path could change and the other not.

- **Every threaded run says which conversation it is in**, not only the run
  that starts one. The name is what `cf last`, `cf catchup` and finding the
  pane again all need, and `(continuing, turn N)` is what lets a lead notice
  the subject has moved far enough to want `--new`. `--json` stays clean.

- **In a terminal in cmux mode, the consult IS the agent's window.** `cf run`
  does not stream and exit there: it opens the harness's own interface on the
  conversation, seeded with the packet (bare message on later turns), and
  stays — attach is the default and only behaviour, so the `--attach` flag is
  gone. claude opens on a uuid we mint (`--session-id`), pi on the
  conversation's name, and opencode and codex each open cold on a seeded
  prompt and have their id read back out of their own store afterwards
  (`discoverOpencodeSession`, `discoverCodexSession` — codex's comes from the
  rollout FILENAME, because a forked session's metadata names a different
  one). kimi is the only harness that cannot be seeded at
  all — `-p` is documented non-interactive and a positional prompt comes back
  as `unknown command` (both probed) — so it streams its first turn and the
  pane becomes its window after, the shape codex used to have. Opening the
  window EMPTY and typing the task in was built and then removed the same day:
  `cmux send` reaches a TUI as a PASTE, and in a paste a newline is a newline,
  so Enter never submits. The text sits in the input box, no answer arrives,
  and the delivery check — waiting for a reply that cannot come — pastes
  another copy on every retry. Six overlapping questions in a pane that looks
  alive: the typed-bootstrap minefield in its exact original form, which is
  why nothing here types at a TUI. Because kimi prints its id LAST, a run that
  dies mid-work takes the id with it: `cf run` falls back to
  `discoverKimiSession` whenever a kimi run ends without one, so a
  conversation survives a rate limit or a closed pane with its work intact.
  All three discoveries read the harness's own store and return null rather
  than throwing.

- **Two harnesses stop to ask before a window opens, and they need opposite
  treatment.** kimi's prompt gates ONE thing and says so: the project's own MCP
  servers, declared in `.kimi-code/mcp.json` or `.mcp.json`. A directory
  declaring none grants nothing by being trusted, so ConsensFlow answers that
  formality itself — `kimiTrustDirectory` writes kimi's own shape into kimi's
  own store at 0600, and the key is `wd_<basename>_<sha256(path)[0..12]>`,
  verified against a key kimi generated for the same directory. A directory
  that DOES declare a server is asking to run a command its author chose, so
  that answer stays the user's. codex is the reverse: its window is seeded
  through argv, so the prompt costs nothing — it waits with the question in
  hand — but its trust lives in `config.toml`, the USER'S file, alongside their
  model, approvals and MCP servers. `codexTrustsDirectory` therefore only
  READS (a deliberately narrow parse: `[projects."<path>"]` headers and the
  `trust_level` under each, matching exactly or by ancestor), and an untrusted
  directory gets one line telling the lead why nothing may come back until
  somebody answers in the pane. Writing it would be the settings.json rule
  broken for our own convenience.

- **A lead reads what is new, not everything.** `cf catchup <name> --unread`
  is only what has been said since THIS lead last looked — a turn count per
  lead, kept as `seen` on the conversation row; plain `cf catchup` still shows
  the whole thing with a line marking where that memory stopped, and every
  read (plain, `--json`, `--wait`) moves the mark, because reading is seeing.
  A lead we cannot name gets no mark — a shared `null` key would merge every
  anonymous shell into one reader — so `--unread` shows it everything, the
  harmless direction for a verb that only reads. Both row writers spread the
  existing record: they do not own `seen`, and rebuilding a row from a literal
  wiped it on the next run. Without this the lead had no way to ask "what did
  he say while I was not looking", so asked to READ a conversation it SENT
  another question and invented a third round instead of finding the second
  (live, 2026-08-24) — which is also why the skill's rules now separate
  reading a conversation from adding to one, and require a look before both.

- **Kimi Code is the fifth harness, and the second that cannot open cold.**
  It meets every requirement (`.specs/kimi-harness/research-01.md`, all
  live-probed): skills at `$KIMI_CODE_HOME/skills`, one-shot `kimi -p …
  --output-format stream-json`, resume `-S <id>`, window `kimi -S <id>`,
  store `sessions/wd_*/<id>/agents/main/wire.jsonl`. Three things are unlike
  the other four and each is a finding, not a gap: **the packet rides in
  argv** (no `--prompt-file`, and `-p -` is a literal dash, so `stdinMode` is
  `none`); **full permissions are IMPLIED by `-p`** — `--auto` and `--yolo`
  are both REFUSED alongside it, so the missing danger flag is the correct
  shape; and **`dropEnv` is empty** because kimi authenticates from its own
  `config.toml` and has no env var whose presence switches billing. `-S`
  resumes but cannot mint, so kimi takes the codex path: stream turn one, read
  the id off `session.resume_hint` (which arrives at the END of the stream),
  then the pane becomes the window. It has no effort flag at all —
  `default_effort` is per-model in that same config file, which is the user's
  and holds their API key, so `EFFORTS.kimi` is empty and a row's effort is
  ignored rather than invented. Its transcript reader joins `content.part`
  events **per `turnId`**: one answer is many parts, and one turn per part
  would inflate the count `--unread` and `--wait` both key on.

- **`cf catchup --wait` survives both of its races.** A fast agent can answer
  BEFORE `--wait` starts (this hung live, twice): a conversation ending in an
  assistant turn is a standing answer, returned. A fast lead can start
  `--wait` before its just-sent question reaches the store: a short grace
  (`CONSENSFLOW_WAIT_GRACE_MS`, default 4s) watches for a newer user turn
  first, and only then settles for the standing exchange. It always prints
  from the last user turn, so a stale answer is recognisable by its question.

- **An image agent draws and stops.** It holds no conversation and opens no
  window, so `--session`, `--new` and `--thread` are REFUSED there rather than
  accepted and ignored — a lead passed one, was told a conversation name, and
  hunted for a conversation that never existed (live, 2026-08-24). And a
  generation that produced no file fails the command: `ok: false`, the reason
  in `result.json`, codex's own words in `codex.log`, exit 1. The old path
  left an empty directory and no record at all, which is why the same lead
  reported "it is running" about a run that had already died with a 400 in
  its face.

- **One spawn verb, three modes.** `cf run @name "<task>"` builds the packet,
  applies the billing guards and streams the run, whichever harness is behind
  the name — and the manager is the only caller of the `hosts/lib` engine now.
  Image agents included: `image` is a harness like the rest, and its run lives
  in `hosts/lib/image-run.js` so no caller carries a second copy.

- **Nobody assigns an agent a persona.** The packet has no "Who you are" and
  never calls a run a coding session; `--brief` is the lead's own words about
  what it wants from THIS spawn, and when there is none, nothing stands in for
  it. Per-spawn rather than stored on the roster, because the same agent is a
  reviewer in one call and a researcher in the next.

- **The handoff is the lead's to give — everywhere, the same way.** Nothing
  stashes a session and nothing attaches one: an agent sees the brief, the
  task, and whatever came in on `--handoff-file`. Claude Code's hooks and pi's
  input watcher existed only to stash, and were deleted 2026-08-22 because
  that was the last behavioural difference between the three modes. Installing
  or uninstalling still takes back hooks an older version wrote.

- **`cf` on PATH is part of every mode**, not an optional extra: the generated
  skill teaches `cf run @name` whoever is in scope, so `applyMode` installs the
  launcher first, in all three. `off` removes ours, by the marker it writes.
  (This doc claimed for a while that a host mode took it back — the code never
  did, and the skill would have named a command the machine did not have.)

- **No harness intercepts a mention.** pi used to swallow `@zeus …` and run the
  agent itself; the lead composes every run now, in all three modes.

- **One surface, everywhere: the skill.** No harness has a private path into
  ConsensFlow. pi's tools went first, then its input watcher, then (2026-08-23)
  the slash commands — pi registered five `/consensflow:*`, Claude Code
  installed one `/consensflow`, and codex and opencode had none, so the same
  request took a different shape depending on where it was made. What is left
  is the skill every harness reads and `cf run @name` that every harness runs.
  Driving it by hand is the CLI's job and the roster UI's. Then (same day) the
  host payloads themselves: `claude` and `pi` were integrations with a CLI and
  a hand-written skill each, justified while a host could hand an agent the
  live conversation — and nothing has stashed one since 2026-08-22. What was
  left was a second copy of the skill that could not name the roster, which is
  what makes a harness reach for it. `src/host-payloads.js` takes all of it
  back; nothing installs it again.

- **Nothing the app installs assumes Node on PATH.** One installed file still
  names a runtime, and only one: the launcher, absolutely (`src/terminal.js`),
  which from the app is its own bundled Node. `terminalRuntime` reads it back,
  so `cf doctor` says MISSING when whatever provided it has moved — rather than
  letting every `cf` the skill teaches fail one at a time.

- **Claude Code's `settings.json` is never written.** It is the user's file,
  and the rule that protects a skill they edited protects it too. A hook an
  older version left there is *reported* by `cf doctor` (`staleClaudeHooks`)
  and removed by the user. ConsensFlow used to rewrite the file on every
  install and drop a world-readable `.consensflow.bak` beside a file Claude
  Code deliberately keeps at 0600 — nothing ever read that copy or cleaned it
  up, so the backup is now something we take back, not something we make.

- **`off` and `reset` are different promises.** `off` removes every file
  ConsensFlow installed and stops there: the roster survives, because agents
  are the user's and the file is shared with anything else that reads it, and
  so does `workspaces/`, where each run's packet, transcript and generated
  image lives. `resetEverything` is the one operation that takes those too —
  the whole config root, drifted skill files included — so it counts what it
  is about to destroy. `resetPreview` is the one place those counts come from,
  so `cf reset`'s refusal and the page's dialog quote the same two numbers —
  and `cf reset` refuses without `--yes`, printing them while nothing has been
  touched. Drift is not honoured there on purpose: the rule exists so an
  install never clobbers an edit by accident, and a reset is not an accident.
  It also clears the desktop app's own data — the `dev.ngvoicu.consensflow`
  directories the OS gives the bundle, which live outside the root because the
  OS decides where they go and which nothing else creates. The `.app` itself is
  deliberately not in that set: an application deleting its own bundle mid-run
  is a bad idea, and on macOS removing an application is a Finder gesture, so
  both surfaces say so instead of doing it.

- **Modules never read `process.env`** — the environment is an explicit
  argument everywhere. That is the whole test-isolation story
  (`tests/helpers.mjs` builds throwaway homes).
- **Drift is sacred.** A manifest-owned file whose hash changed was edited by
  the user: refuse without `--force`, never clobber. A file not in the
  manifest is never touched, installed over, or deleted.
- **A harness with its own ConsensFlow keeps it.** `detectHarnesses` marks `native` (cc plugin cache / pi extension checkout) and `syncGeneratedSkill` leaves those out of scope in every mode. Without this, claude and pi see two same-named skills with the same trigger, competing for one skills budget.
- **Full permissions, no permission concept.** There is still no knob, tier or
  policy to choose — but the default is now everything, not the CLI's own
  default: `--dangerously-skip-permissions` (claude),
  `--dangerously-bypass-approvals-and-sandbox` (codex, replacing the
  workspace-write sandbox), `--auto` (opencode), nothing for pi, whose tools
  are on already. A agent is a helper you hand a task to, so it writes
  anywhere and reaches the network; the approval gate on *keeping* its work is
  what protects the user, not a fence around the run. Asserted in
  `tests/skill.test.mjs` and both engine suites so no refactor re-fences it.
- **The generated commands carry `env -u ANTHROPIC_API_KEY` / `-u
  OPENAI_API_KEY`** — v1's dropEnv as prose; prevents subscription logins
  silently switching to API billing.
- **Every roster mutation regenerates the installed skill** (CLI and UI both
  call `refreshInstalledSkill`); only where the manifest says it is installed.
- **Pane control belongs to cmux**, never to us — the v2 lesson
  (typed-bootstrap verification is a minefield). Our skill says *what* to run
  and *quotes* the four cmux commands a lead needs (`new-pane`, `send`,
  `rename-tab`, `list-pane-surfaces`), because a lead without them spent a
  minute dumping `cmux --help` before it could ask a question. Quoting is not
  implementing — we drive no pane, and the skill says outright that the
  commands are cmux's and `cmux --help` is the authority if they move. The
  same section forbids reading the answer back off the screen: `cf catchup`
  exists so nobody has to. Pane control is also a different product from
  consulting, which is why only `cmux` mode carries any of this: a Claude
  Code install runs its agents as subprocesses and never touches a pane.
- **Tests spawn no live agent CLIs and no network** — agent CLIs are stub
  scripts on a fake PATH; git is a PATH shim copying a fixture tree.

## Commands

```sh
npm test          # node --test — what the skill SAYS, no live CLI, no network
npm run check     # biome + tests
npm run eval      # what a lead DOES with it — real lead, real skill, stubbed cf/cmux
```

`evals/` is the answer to a question the unit tests cannot ask. Three
behavioural failures in one day (2026-08-24) were each answered with more
prose and none was measured, so the eval runs a REAL lead against the REAL
installed skill with `cf` and `cmux` replaced by recording stubs, and asserts
which commands it chose. It spends real tokens and is deliberately outside
`npm test`. Scenarios are the failures themselves; a check that passes 4/5 is
a failing check. Baseline 2026-08-24 (claude): 13/13, and 3/3 on repeat for
both read-versus-write scenarios.

## Spec

`.specs/consensflow-v3-skills-first/` — the forge record, including the
skill-first inversion (v0 prose live-proven on claude and codex before any
factory code) and the live-verified one-shot command forms per engine.
