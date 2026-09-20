---
id: candidate-feedback-3
title: Third hands-on round — one Agents screen, no PM, three member roles, tiers instead of tags
status: completed
created: 2026-09-20
updated: 2026-09-20
priority: critical
tags: [agents, roles, tiers, board, simplification]
---

# Third hands-on round: fewer concepts

Gabriel, on the Candidate built from `87c45f5`, 2026-09-20 afternoon:

- "Somehow it's not ok to have the roster and the agent library, it's a
  duplication."
- "We should remove the PM concept and all its code and features; and the
  roles will be worker; reviewer (for worker); advisor (only the lead can get
  advice)."
- "Each agent will not have tags, will have only severity like critical
  work, complex tasks and so on; we remove the concept of tags."

## Decisions and scope

- **One Agents screen.** The catalog and the saved agents are one list,
  grouped by model: a catalog entry not yet saved offers Add; once saved it
  shows as the saved agent, editable, in the same place. The Settings menu
  offers Agents and Harnesses.
- **No PM.** The lead is the only coordinator. The human gives the lead its
  work; the lead hands work to workers, gets advice from advisors, and the
  review policy sends workers' finished work to reviewers. Everything that
  existed only for the PM goes: its role text, `pm.add`, its lane, its
  section in the team dialog, its harness choice, `teamOf`, the PM pool of
  advisors.
- **Three member roles**, all under the lead: `worker` does bounded work;
  `reviewer` reviews workers' finished work when the policy asks; `advisor`
  answers the lead's questions and plans, changing no file. The lead asks an
  advisor with `cf task add --advice --tier <tier> "…"`.
- **Tiers instead of tags.** An agent carries a work tier (critical, complex,
  standard, light) and nothing else about what it is good for. A task names a
  tier; the daemon picks a free member of that tier with the fewest tasks so
  far. Tags go from the roster, the catalog presets, the team, the board, the
  commands (`--tags`), the role texts and the ledger (a migration drops the
  columns).

## Phases

### Phase A: The ledger, dispatcher, API and commands [done]

- [x] [TEST-CF3-01] Ledger: no PM (`addPm` gone, coordinators are the lead
  alone), no tags (a migration drops `participant.tags` and `task.tags`; a
  home written with them upgrades), advisors' tasks come from the lead
  (`pool: 'advisor'`); the dispatcher ranks by fewest tasks then join order.
- [x] [IMPL-CF3-02] Satisfies TEST-CF3-01; `cf task add --advice`; `cf team`
  and the API without tags; role texts for lead, worker, reviewer, advisor
  (no PM, no tags, advice to the lead only).

### Phase B: Agents and the board [done]

- [x] [TEST-CF3-03] Agents screen: one list of catalog entries and saved
  agents by model; a saved agent shows its name, harness, tier and route,
  editable tier and effort; no tags anywhere; presets carry no tags.
- [x] [TEST-CF3-04] Board: no PM lane, no tags in the team dialog, the New
  project dialog, the rows or the composer; the composer offers "advice from
  a <tier> advisor".
- [x] [IMPL-CF3-05] Satisfies both; the bench steers by tier (one tier per
  bench worker) instead of tags; Candidate rebuilt.

## TDD log

- 2026-09-20 afternoon, both phases in one gated commit. A fourth migration deletes
  a home's PM rows (their tasks and messages go with them by cascade) and
  drops both `tags` columns; the upgrade test writes a version-1 home with a
  PM, a PM-requested task and tags and checks nothing dangles
  (`PRAGMA foreign_key_check`). `#reviewDue`: workers' work under `members`,
  the lead's too under `all`, advice never. `#rank`: fewest taken, then join
  order; the dispatcher tests that steered by tag use a one-worker team or a
  distinct tier. `cf task add --advice --tier <t>`; `POST /api/tasks` takes
  `advice: true`. `roleInstructions` appends the coordinating text to the
  lead alone, with a team table of name, roles and tier built from the
  project's participants (no roster read). The Agents screen is one list
  (`/library` gone); the browser spec rewritten for one screen in two tabs
  with a Show filter. Node 819/819, integration 8/8, Rust 108, clippy clean,
  page 79/79.
- Assumptions to confirm with Gabriel: installing this build deletes any PM
  and its tasks from Candidate projects; the catalog's category pills stay
  (only tags go); advice is never reviewed; the lead's old "never write
  specifications" line is dropped with the PM and not replaced.
- Advisor review found a gap: the ledger's tier check at task creation read
  the primary `role` column, so a member saved as worker+advisor did not
  count as an advisor and `cf task add --advice` was refused; `#members` now
  reads the roles set (and skips sessions). Tests: a worker-first advisor's
  advice opens with the advisor text and is never reviewed (ledger and
  dispatcher); a member counts for every role it holds.
- Live bench on the cut (`npm run bench:core -- opencode pi`, one tier for
  both workers, Devin reviewing): first run 18/22, the four review checks
  failing because the bench watched the first worker's lane while the daemon
  had given the review's work to Pi, the worker with fewer tasks (the
  question step runs only for harnesses with a question tool, so the round
  was not balanced). The daemon was right; the bench now follows the task
  to whichever worker got it. Second run 22/22: tasks, `--after`
  continuation, OpenCode's question door, the review (work to Pi in 18 s,
  verdict in 35 s, result with the review under it in 37 s) and the restart.
- Gabriel, on the rebuilt Candidate: the category pills "should be only 4
  options, Lead candidate, Advisor, Worker/Coding, Reviewer". The pills now
  name the roles a model suits, in that order: Lead candidate and Advisor
  for the role models at xhigh or above (a model fit to lead is fit to
  advise), Worker for every coding model outside critical work, Reviewer
  above low effort; critical-tier models are Advisor and Reviewer; an image
  agent suits none. The filter reads "Suits: Any role"; Architecture, Hard
  problems, Coding and Images are gone.
- Gabriel: "add also astra high in consensflow". GPT-6 Astra at High on
  every road that reaches it: Celaeno (Codex), Taygete (Pi), Vidar
  (OpenCode); Complex work, Worker and Reviewer. The catalog is 102 entries.
- Gabriel: "DEFINE YOUR OWN doesn't have all options the others have". The
  Define-your-own form now offers the work tier (Automatic, or one of the
  four) beside the effort, the same choices a catalog agent's editor has.
- Gabriel: "advice stays lead's alone" (the ledger refuses an advice task
  from anyone but the lead; the composer offers workers' tiers only); "for
  reviewers we need only the models that are now also workers" (the Reviewer
  pill sits on the Worker models; critical work is Advisor alone); "all work
  reviewed should go away" (the review policy is none or workers' work; a
  fifth migration reads a home's `all` as `members`; the lead asks for a
  review of its own work by hand with `cf task review`).
- Gabriel: "clean code, remove all unused code". The old one-shot runner is
  gone: `hosts/lib/{runners,codex-auth,harness-transcript,image-run,packets,
  session-binding,threads,state,transcript-events}.js`, the presets' runner
  helpers, the completion module's cursor slicing, the utils left to slugify
  and stripMention, every export nothing shipped read made private; the
  window builders every adapter uses live in `hosts/lib/windows.js`. Ten test
  files and five fixtures went with it; the window tests, the Pi path tests
  and the name-neutrality check stayed (the last one now sweeps hosts, bin,
  src and skill). The README describes the board, the roles, the tiers and
  the screens as they are.
- Gabriel, on the result a Claude lead receives: the harness frames a board
  message as a teammate's request, so every delivered result now ends with
  "Decide with: cf task accept T-n · cf task reopen T-n · cf task review
  T-n", the way a question ends with how to answer it.
- Gabriel: an image agent needs a pill and a role of its own, and the team
  is picked role-first. A fifth member role, the *image designer*, drawn by
  Codex's own image tool in a Codex window (`hosts/lib/windows.js` opens an
  `image` harness as Codex on its default model; `designer.md` is its
  text); the lead asks with `cf task add --design "…"`, a task with no
  tier that goes to a free designer and is never reviewed; the catalog's
  image entry carries the Image designer pill. A sixth migration rebuilds
  the project, participant and task tables the way SQLite documents it
  (foreign keys off, copy, drop, rename, check): the role constraint knows
  the designer and has forgotten the PM, the pool constraint the designer,
  the review constraint the all policy. Both team dialogs are role-first:
  pick a role, then an agent whose model suits it (the Agents screen's
  pills), one row per member and role, Remove per row, the last role asking
  first; no checkboxes. The board composer offers "An image designer".
- Gabriel: the agent picker in the team needs the model's effort level. Each
  choice reads name · harness · model · effort, in both dialogs and in the
  New project rows.
- Gabriel: the "Good for" description was still on some model cards. The
  description is gone from the profile and the screen: a model's card says
  its tier, the roles it suits and its scores, nothing else.
- Gabriel: "you decide and finish". The team pickers say why an agent list
  is empty (no saved agent suits the role, or every suitable one holds it
  already). A live design task ran through the daemon on the Codex login:
  see the next entry for what it saved.
- The design task, live (scratchpad probe through the daemon, OpenCode
  lead, pygmalion on the team as designer, the human asking from the
  board): the task opened with no tier, went to a fresh designer window at
  once, Codex drew and saved `images/harbour-logo.png` (765 KB) in 125 s,
  and the path came back as the result. 6/6.
