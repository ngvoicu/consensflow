# ConsensFlow

A control board for a staff of coding agents. You give a Chief of Staff its
work; the chief hands bounded tasks to workers and asks advisors, a daemon
opens each harness's own window for every task and brings every result back
to the board, and reviewers judge workers' work on an independent model
before you see it.

Everything runs through the harness CLIs you already have installed and logged
in (Claude Code, Codex, OpenCode, Pi, Devin). ConsensFlow stores no
credentials, takes no API key, and writes only inside its own home. Agents
run with full permissions in their windows: the protection is the review, the
chief's acceptance and your approval, not a fence around the run.

## The model

- **A project** is one folder, one board, one Chief of Staff. The chief is the
  only coordinator: it plans, does the work that is its own, and puts everything
  else on the board.
- **Members** are the agents you put on the project's staff, each with one or
  more roles. A *worker* does bounded work. An *advisor* answers the chief's
  question with findings and changes no file; only the chief asks for advice.
  A *reviewer* checks finished work and changes no file. Nothing is reviewed
  on its own: a review is a task the chief puts on the board for a reviewer's
  tier (`cf task add --review --tier complex "Review T-3: …"`), and its
  findings come back as that task's result.
- **Tiers, not names.** The chief names the tier of member a task needs
  (critical, complex, standard, light), never the member. The daemon gives the
  task to a free member of that role and tier with the fewest tasks so far,
  the earliest joined first. No member of that tier on the staff and the task
  is refused; none free and it waits on the board.
- **One task, one window.** A member's task runs in a session of its own,
  named after the member (`diana-amber-pine`): a fresh window that opens with
  the task and closes when the work leaves its hands. Nothing carries over,
  so the chief writes every task for someone who has never seen the project.
  When a follow-up truly needs what a window already knows, the chief continues
  that window with `cf task add --after T-3 "…"`. A session keeps its
  conversation until you delete it: from its lane you open its terminal
  (again on that conversation, if it was closed), close it, or delete the
  session for good. There is no limit on sessions and none of them expires.
- **You talk to the chief in its terminal.** Your work reaches the project
  through the chief: you type to it, and it plans, puts tasks on the board and
  decides on every result. The board has no Accept for you; you read, send
  back, pause, and approve what waits for you.
- **The board is the only channel.** No agent gives another a task by name,
  types into another window, or reads another agent's session files.
  Questions go up: a member asks the chief on the board, and the answer lands in
  the window that asked, through the harness's own question tool where it has
  one. The chief asks you in its own terminal, where you work with it: nothing
  asks you on the board.
- **Review.** A project reviews its workers' finished work, or nothing. The
  reviewer must run a different model than the author; a request for changes
  goes back to the author once, and after a second round the chief decides with
  both reviews in hand.
- **Pause and resume.** The chief (or you) stops a worker's task by its
  number: `cf task pause T-5` interrupts the agent and keeps its window,
  conversation and work; `cf task resume T-5 "…"` sends the words into the
  same window. Something urgent for a window mid-task goes with `cf tell
  T-5 "…"`: the task is paused for it, the agent reads it once interrupted,
  answers as it would any question, and the chief resumes the task with its
  words. A window lost to a restart or a
  crash pauses its task the same way and tells the chief, so nothing is
  redone from scratch. A member
  that runs out of quota mid-task keeps the task with its window when the
  reset is within half an hour or nobody else of its tier is free, and goes
  on by itself at the reset; otherwise the task goes back to the board for
  another member.
- **A plan on the board.** A task may need others first: `cf task add
  --needs T-3,T-4 "…"` waits, blocked, until each is accepted (one the chief
  gives itself waits the same way and comes back to it then), and the
  daemon gives out only unblocked tasks, so a plan of many
  tasks runs in its own
  order with parallel work where the plan allows it. When a result uncovers
  work that must come first, `--before T-9,T-10` puts a new task ahead of
  tasks still on the board. The board is the plan's memory.
- **Human approval required.** With this project setting on, every message
  between two agents (a task, a result after its review, a question, an
  answer) waits in your bay until you pass it on. You may also decline a task
  or an answer with a word to its sender. What you send, what reaches you
  and what ConsensFlow itself notes never wait.

## Install

Download it from [Releases](https://github.com/ngvoicu/consensflow/releases):
on a Mac, open the DMG and drag ConsensFlow to Applications; on Windows, run
the installer, or the portable `ConsensFlow_<version>_x64-portable.exe`, one
exe you run from anywhere. Its first start unpacks Node and the CLI into
`%LOCALAPPDATA%\dev.ngvoicu.consensflow\runtime`; its data stays in
`%USERPROFILE%\.consensflow`, as the installed app's does.
It carries its own Node runtime and its own copy of ConsensFlow. Nothing else to install: the harness
integrations it needs are prepared under its home the first time a window of
that harness opens, and updated the same way.

The app is not signed with an Apple or Microsoft certificate yet, so its first
open asks. On a Mac, macOS blocks it once: allow it in System Settings →
Privacy & Security → Open Anyway. On Windows, SmartScreen asks: choose More
info → Run anyway.

The development build is **ConsensFlow Candidate**, installed beside the
release with its own home, so a build under test never touches your live
projects.

## Inside the app

- **The board.** One lane per participant: you, the chief, each member and its
  live sessions under it. A task moves from the backlog through queued and
  working to done; you open any card to read its brief, its result, its thread
  and what its window wrote, and to pause it, reassign it (back to the board
  for another member of its tier) or cancel it. The chief's window is docked beside the board; a strip holds every
  live window.
- **New project.** A folder, the saved agent the lead runs on (its harness,
  model and effort come with it), the staff (the last project's ticked
  already) and whether human approval is required.
- **Switch lead.** On the chief's row: the lead goes on in a new window on
  another saved agent, with its harness, model and effort. A lead always runs
  on a saved agent, never on a harness's own default model; a lead that was
  started on its harness's default before keeps running on it until you
  switch it. A lead at work finishes its turn first, or is cut off if you say
  so; it can first be asked to write down where things stand. Its first
  message hands it the lead: what waits on the board, your last words, and
  `cf history`, which pages through what you and the leads before it said.
  The staff keeps working, and what was on its way to the lead goes to the
  new one.
- **Staff.** Which agents this project may use, each with its roles and what
  it runs (model, harness, effort, tier), plus the approval setting. The
  rows read by role, then by work tier with the most critical first; the
  pick list offers every agent by work tier the same way. The daemon
  assigns work only within the staff. A member whose agent is gone (a
  release dropped the catalog entry, or you removed one of your own) runs
  on no default: its row says so, it gets no work, and what it held goes
  back to the board; define the agent again or remove the member.
- **Agents** (Settings). One roster: every catalog agent and every agent
  you define, as one list by model. Each model's card says its work tier
  once; each agent's row says its harness, effort, tier and route. A
  catalog agent is exactly what the catalog ships: when a
  release moves its entry to a newer model, the agent and every staff it is
  on move with it, and it cannot be edited or removed. Your own agents carry
  Edit and Remove; define one with any model string its harness accepts and
  the settings you want, under a name the catalog does not have. A checkbox
  keeps Claude and OpenAI models to their own harnesses: they are hidden on
  Pi and OpenCode, here and in the staff dialogs; a member already on one
  still runs.
- **Harnesses** (Settings). Which harness CLIs are installed, their versions
  and whether ConsensFlow's integration with each is in place.

## Developing

    npm run check          lint and the Node suite
    npm run build:bridge   the headless pane host the integration suite drives (a test helper no app ships)
    npm run test:integration   the daemon against the real pane host with fake agents
    npm --prefix app run test:ui   the board and the Agents screen in a browser
    npm run smoke          the packaged app
    npm run load           the daemon under load: several projects, waves of tasks, the page polling
    npm run bench:core     the live bench on real harnesses (opt-in, cheap models)
    npm run candidate      build, smoke-test and install ConsensFlow Candidate

The daemon keeps its own log at `~/.consensflow/daemon.log` (one `.1` kept
past 5 MB): when it started, why it stopped, a pass that failed or ran long,
an error nobody caught, and every ten minutes that it is alive and how big it
is. The app asks it to stop before quitting, so a start with no stop after it
means something outside the app killed it.

Requirements, decisions, status and the release runbook live in the
`consensflow-sme` brain. A release is one tag: set the version in
`package.json`, `package-lock.json`, `app/src-tauri/Cargo.toml`,
`app/src-tauri/Cargo.lock` and `app/src-tauri/tauri.conf.json`, then push an
annotated `v<version>` tag to GitHub, its message the release notes;
`.github/workflows/release.yml` builds, checks and publishes the DMG, the
signed update bundle, the Windows installer and the portable exe.
