# ConsensFlow

**A desktop app that runs a team of AI coding agents on your project.**

You talk to one agent, the **Chief of Staff**. It plans the work and puts
tasks on a board; the other agents on your staff pick them up, each in a
terminal of its own, and every result comes back to the board. You see it
all in one window: the board on the left, the agents' terminals on the right.

ConsensFlow works through the coding agents you already use (Claude Code,
Codex, Pi, Devin and Muse Code), with your own logins. It stores no
passwords and needs no API keys.

## What it does

- **One chief, a whole staff.** Give the chief your goal in its terminal. It
  breaks the work into tasks, does what is its own, and hands the rest to the
  staff you picked for the project.
- **Nine roles.** The chief gives each member work for its role:
  - **Worker:** a task; completed work and evidence come back.
  - **Frontend designer:** a screen's purpose; its design and implementation
    come back.
  - **Architect:** a design problem; documents, contracts and a code frame
    come back.
  - **Researcher:** a question; findings and where they were found come back.
  - **Advisor:** a question; findings and recommendations come back.
  - **Reviewer:** finished work to check; findings and evidence come back.
  - **Tester:** work to run and check; test results and evidence come back.
  - **Integrator:** accepted work; the merged, checked and landed work comes
    back.
  - **Image designer (on Codex):** a drawing order; the saved image comes back.

  Researchers, advisors and reviewers change no project files; testers change
  no product code.
- **Do with your own hands.** Turn this on in New project, Project settings or
  the chief's pane, and the chief does your work itself, using the board only
  when you ask. Work already on the board continues. The chief can read the
  board's guide in parts with `cf guide`.
- **A board you can read.** The dark board groups the staff by role. Each task
  shows who has it, where it stands, and how long it has been in that state.
  Open a task to read its brief, result and messages, and see what the agent
  did. History and the standalone Harnesses page follow your system's light
  or dark appearance.
- **Each task in its own window.** A task opens a fresh terminal of its
  agent's harness, and the window closes when the work is done. Its
  conversation is kept, so a follow-up continues where it stopped.
- **Work by tier.** For worker, advisor, reviewer and tester tasks, the chief
  asks for a tier (critical, complex, standard or light). Each task gets a
  new window of a member of that role and tier. If nobody of that role holds
  the tier, the nearest one with a member is used. The other roles need no tier.
- **Backups and Vacation.** Give a member an optional backup on any harness
  or model. A new task always goes to a member first. A task its member cannot
  run (out of quota, its login or billing refused, its model moved or busy
  past its tries, its harness not opening, or in need of an edit) goes to the
  member's backup, whether it was new or already given: a task in progress
  starts over on the backup in a fresh window, and finishes there even after
  the member is back. A task given by name, a follow-up, the chief's own and a
  paused task stay where they are. The chief and image designer have no
  backup. Send a member or backup on Vacation from the board or Project
  settings to stop new tasks; work already in a window finishes there. A
  member on Vacation is not covered by its backup.
- **Plans with an order.** A task can wait for others to be accepted first,
  so a plan runs in its own order, in parallel where it can. The chief can
  add a plan and its dependencies in one command with
  `cf task add --plan plan.md`.
- **Tell, pause and resume.** A tell lets a member keep working and reaches
  its window at the next rest. A pause stops the task and can ask a question;
  resume sends it on in the same window.
- **Meet with the chief.** Choose **Start meeting** on the chief's window.
  The staff's results and questions wait on the board while you talk, and no
  wake-up calls arrive. **Stop meeting** lets the held messages reach the chief
  one at a time.
- **Notes without interruption.** ConsensFlow types notes that need the
  chief's attention into its window. Other status notes are available to the
  chief in `cf inbox`.
- **Resume after a quit or update.** Resume a project to bring its windows
  back on their conversations. A member cut mid-turn is told to go on. If a
  member's window cannot come back, its task is paused with the reason.
- **Quota-aware.** A member works up to 100% of its quota and waits when its
  harness says it is out, keeping its work in its window until the reset. If
  the account's quota is renewed early, **Try again now** brings it back; for
  harnesses with a shared login, it also brings back chiefs and members across
  projects.
- **Your approval, if you want it.** Turn on human approval for a project,
  and every message between agents waits for you to pass it on.
- **Switch the chief anytime.** Move the chief to another model or harness.
  The new chief starts with a handoff: what is on the board, your last words,
  and the history of the chiefs before it.
- **Read specs.** The board's **Specs** button opens the project's specs folder
  as a tree, with its registry and Markdown, including Mermaid diagrams. HTML
  specs open in your browser from a copy. The button appears when specs are on
  or the folder holds a file.
- **Search History.** Open **History** on a window to search the participant's
  stored conversations, including a chief's history across chief switches.
- **The project menu.** Right-click a project in the sidebar to rename it, add
  a private one-line description, or open its Project settings or Specs.

## Supported agents

ConsensFlow runs each agent through its harness's own command-line tool.
Install the ones you want to use, and sign in to each once in a terminal:

| Harness     | Install                                          |
| ----------- | ------------------------------------------------ |
| Claude Code | `npm install -g @anthropic-ai/claude-code`, or Anthropic's installer |
| Codex       | `npm install -g @openai/codex`, or its own installer |
| Pi          | `npm install -g @earendil-works/pi-coding-agent`, or its own installer |
| Devin       | Devin's own CLI installer (version 3000.10.21 or later) |
| Muse Code   | Meta's installer (version 1.4.13 or later) |

You don't need all five: one is enough to start. **Settings → Harnesses**
shows which are installed, their versions, and whether ConsensFlow is set up
for each. ConsensFlow prepares what it needs for a harness by itself, the
first time one of its windows opens.

In New project, Project settings and Switch chief, pick a harness, its model
and, where offered, an effort. The lists are read from the installed harnesses
when the dialog opens, and members are named by their model.

**Settings → Specs** says whether a chief plans big work as specs in every
project that doesn't say otherwise, in which folder and with which variant of
Specmint, and shows the Specmint version ConsensFlow brings and what each
variant is. A project's own Specs settings win, and what it leaves out follows
what is set here.

## Install

Download the latest version from
[Releases](https://github.com/ngvoicu/consensflow/releases).

**Mac** (Apple silicon, macOS 13.5 or later)

Download the `.dmg`, open it, and drag ConsensFlow to Applications. The app
is signed and notarized by Apple, so it opens like any other app.

**Windows** (Windows 10 or 11, 64-bit)

- **Installer:** run `ConsensFlow_<version>_x64-setup.exe`.
- **Portable:** `ConsensFlow_<version>_x64-portable.exe` is a single file you
  can run from anywhere, with nothing to install.

The first time you run it, SmartScreen asks, because the app is not signed
yet: choose **More info → Run anyway**.

**From a terminal** (releases from 3.0.7-alpha.1 on)

```sh
curl -fsSL https://consensflow.ngvoicu.dev/install.sh | sh          # macOS
```

```powershell
irm https://consensflow.ngvoicu.dev/install.ps1 | iex               # Windows
```

Each installs the latest release (`sh -s -- --alpha`, or
`$env:CONSENSFLOW_CHANNEL = 'alpha'` first, for the latest alpha; `--help` says
the rest). It installs nothing until the release's `SHA256SUMS` carries the
signature of ConsensFlow's update key for that release and the download matches
it, and on a Mac until the app's Developer ID signature and notarization hold.
An installed ConsensFlow is upgraded where it is, once it has quit; a newer one
is kept, unless `--version` names the release. The scripts are `install/` in
this repository.

Everything ConsensFlow needs comes with it. The only other things to install
are the harnesses you want to use.

## Getting started

1. Install at least one harness and sign in to it.
2. Open ConsensFlow and choose **New project**: pick the project's folder,
   the chief's harness and model, and the staff. You can also choose backups,
   own hands, human approval, wake-up calls and specs. Change them later in
   **Project settings**.

   Project settings also lets you set **Compaction** for members: a size from
   250,000 to 1,000,000 tokens, or the harness's own point. A new project
   starts at 300,000. **Prompt suggestion** controls Claude Code's next-prompt
   guess for the chief and members; **Advisor tool** controls Claude Code's
   own advisor tool for members. Both are off by default; when on, Claude
   Code follows its own settings.
3. Tell the chief what you want, in its terminal.
4. Watch the board. Open a task to read it, or open a member's terminal from
   its row to see it work.

## Updates

On a Mac, ConsensFlow checks for updates by itself, and you can check from
**ConsensFlow → Check for Updates…**. Updates install in the app.

On Windows, Check for Updates tells you when a new version is out and opens
its download page.

## Your data

- ConsensFlow keeps its data in `~/.consensflow` (on Windows,
  `%USERPROFILE%\.consensflow`).
- It never sees your passwords: each agent uses its own harness's login.
- Agents work with full permissions in their windows, as they would if you
  ran them yourself. Your protection is the review, the chief's acceptance
  and, if you turn it on, your approval. Use ConsensFlow on projects kept in
  version control.
- Every agent, the chief and its staff alike, starts with its harness's MCP
  servers and connectors (your browser, your accounts, your IDE), skills,
  settings and customizations, as it would if you ran it yourself. The staff
  use them with full permissions while you aren't watching, so keep in a
  harness only what you would let any agent use.

## License

[MIT](LICENSE)
