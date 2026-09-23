# ConsensFlow

A control board for a team of coding agents. You give a lead its work; the
lead hands bounded tasks to workers and asks advisors, a daemon opens each
harness's own window for every task and brings every result back to the
board, and reviewers judge workers' work on an independent model before you
see it.

Everything runs through the harness CLIs you already have installed and logged
in (Claude Code, Codex, OpenCode, Pi, Devin, Kimi Code). ConsensFlow stores no
credentials, takes no API key, and writes only inside its own home.

## The model

- **A project** is one folder, one board, one lead. The lead is the only
  coordinator: it plans, does the work that is its own, and puts everything
  else on the board.
- **Members** are the agents you put on the project's team, each with one or
  more roles. A *worker* does bounded work. An *advisor* answers the lead's
  question with findings and changes no file; only the lead asks for advice.
  A *reviewer* checks finished work and changes no file. Nothing is reviewed
  on its own: a review is a task the lead puts on the board for a reviewer's
  tier (`cf task add --review --tier complex "Review T-3: …"`), and its
  findings come back as that task's result.
- **Tiers, not names.** The lead names the tier of member a task needs
  (critical, complex, standard, light), never the member. The daemon gives the
  task to a free member of that role and tier with the fewest tasks so far,
  the earliest joined first. No member of that tier on the team and the task
  is refused; none free and it waits on the board.
- **One task, one window.** A member's task runs in a session of its own,
  named after the member (`diana-amber-pine`): a fresh window that opens with
  the task and closes when the work leaves its hands. Nothing carries over,
  so the lead writes every task for someone who has never seen the project.
  When a follow-up truly needs what a window already knows, the lead continues
  that window with `cf task add --after T-3 "…"`. A session keeps its
  conversation until you delete it: from its lane you open its terminal
  (again on that conversation, if it was closed), close it, or delete the
  session for good. There is no limit on sessions and none of them expires.
- **You talk to the lead in its terminal.** Your work reaches the project
  through the lead: you type to it, and it plans, puts tasks on the board and
  decides on every result. The board has no Accept for you; you read, send
  back, pause, and approve what waits for you.
- **The board is the only channel.** No agent gives another a task by name,
  types into another window, or reads another agent's session files.
  Questions go up: a member asks the lead, the lead asks you. You answer on the
  board, and the answer lands in the window that asked, through the harness's
  own question tool where it has one.
- **Review.** A project reviews its workers' finished work, or nothing. The
  reviewer must run a different model than the author; a request for changes
  goes back to the author once, and after a second round the lead decides with
  both reviews in hand.
- **Pause and resume.** The lead (or you) stops a worker's task by its
  number: `cf task pause T-5` interrupts the agent and keeps its window,
  conversation and work; `cf task resume T-5 "…"` sends the words into the
  same window. A window lost to a restart or a crash pauses its task the
  same way and tells the lead, so nothing is redone from scratch.
- **A plan on the board.** A task may need others first: `cf task add
  --needs T-3,T-4 "…"` waits, blocked, until each is accepted, and the daemon
  gives out only unblocked tasks, so a plan of many tasks runs in its own
  order with parallel work where the plan allows it. When a result uncovers
  work that must come first, `--before T-9,T-10` puts a new task ahead of
  tasks still on the board. The board is the plan's memory.
- **Human approval required.** With this project setting on, every message
  between two agents (a task, a result after its review, a question, an
  answer) waits in your bay until you pass it on. You may also decline a task
  or an answer with a word to its sender, send a result back with a
  follow-up, or answer a question yourself. What you send, what reaches you
  and what ConsensFlow itself notes never wait.

## Install

Download the app, drag it to Applications, open it. It carries its own Node
runtime and its own copy of ConsensFlow. Nothing else to install: the harness
integrations it needs are prepared under its home the first time a window of
that harness opens, and updated the same way.

The development build is **ConsensFlow Candidate**, installed beside the
release with its own home, so a build under test never touches your live
projects.

## Inside the app

- **The board.** One lane per participant: you, the lead, each member and its
  live sessions under it. A task moves from the backlog through queued and
  working to done; you open any card to read its brief, its result, its thread
  and what its window wrote, and to pause it, reassign it (back to the board
  for another member of its tier) or cancel it. The lead's window is docked beside the board; a strip holds every
  live window.
- **New project.** A folder, the lead's harness, the team (the last project's
  ticked already) and whether human approval is required.
- **Team.** Which saved agents this project may use, each with its roles and
  tier, plus the approval setting. The daemon assigns
  work only within the team.
- **Agents** (Settings). The catalog and the agents you saved, as one list by
  model. Each model's card says its work tier and its benchmark scores once;
  a saved
  catalog entry takes its row's place with its harness, tier, route and an
  editor for model, effort and tier. A saved agent follows its catalog entry:
  when a release moves the entry to a newer model, the agent and every team
  it is on move with it, until you edit its model or effort, which makes it
  your own. Define your own with any model string its harness accepts.
- **Harnesses** (Settings). Which harness CLIs are installed, their versions
  and whether ConsensFlow's integration with each is in place.

## Inside a window

`cf` is on the PATH of every window ConsensFlow opens, and the board is its
only subject:

    cf task add --tier <critical|complex|standard|light> "…"   work for a worker
    cf task add --advice --tier <tier> "…"                     a question for an advisor (the lead)
    cf task add --after T-3 "…"                                continue the window that did T-3
    cf task add --self "…"                                     work the lead does itself
    cf task list | get T-3 | done T-3 "…" | review T-3 | accept | reopen | cancel
    cf inbox [read m-12] · cf ask "…" [--human] · cf answer m-12 "…" · cf team · cf whoami

Every role's window opens with its role text: what it does, what it never
does, and these commands. Agents run with full permissions in their windows;
the protection is the review, the lead's acceptance and your approval, not a fence
around the run.

## Developing

    npm run check          lint and the Node suite
    npm run test:integration   the daemon against the real pane host with fake agents
    npm --prefix app run test:ui   the board and the Agents screen in a browser
    npm run smoke          the packaged app
    npm run bench:core     the live bench on real harnesses (opt-in, cheap models)
    npm run candidate      build, smoke-test and install ConsensFlow Candidate

Requirements, decisions and status live in the `consensflow-sme` brain; the
repo's `.specs/` folder tracks each piece of work.
