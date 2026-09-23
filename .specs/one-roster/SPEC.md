---
id: one-roster
title: One roster — every catalog agent is an agent, edits are overrides, teams pick from all of them
status: completed
created: 2026-09-23
updated: 2026-09-23
priority: high
tags: [roster, catalog, agents, team]
---

# One roster

Gabriel, 2026-09-23, on the Agents screen sorted flat: "looks bad… wouldn't
it be better to have all the time all agents in the roster without having
the Add button? all premade agents + custom ones in the roster, and the
team menu to show them all, but add also criticality in the list."

## Decisions

- **Every catalog agent is an agent.** The roster is the catalog plus the
  agents defined by hand. Nothing is "saved" or "added" from the catalog;
  Add exists only under Define your own, for a name the catalog does not
  have.
- **Edits are overrides.** The file on disk (`agents.json`) keeps custom
  agents in full, and for a catalog agent only what the human changed:
  model, effort, tier or description, keyed by the catalog entry. Reading
  merges the catalog with the overrides, so a release that moves an entry
  reaches every field the human did not touch. Reset drops the overrides.
  Remove exists only for custom agents. A catalog agent's harness cannot
  change.
- **Old files read the same.** A row an older ConsensFlow saved from the
  catalog (a full copy with `preset`) reads as overrides of the fields
  that differ; the daemon folds such rows into overrides at start and
  drops the profile it used to store. A custom row that happens to carry a
  catalog name with another harness hides that catalog entry.
- **One row per agent on the Agents screen**, in every sort and grouping:
  name, model · harness · effort, the tier pill, the route, and Edit, plus
  Reset when edited and Remove when custom. Search, work tier, group by
  and sort by stay. "Show" offers all, or only edited and custom.
- **The team dialogs offer every agent**, grouped by harness, each line
  `name · model · harness · effort · tier`.
- **Rejected:** a custom agent shadowing a catalog name on purpose (Add
  refuses catalog names); a saved list beside the catalog (that was the
  screen that looked bad).

## Phases

### Phase A: the roster [done]

- [x] [TEST-OR-01] `listAgents` lists the catalog and the custom agents;
  `agentRow` gives the merged row the launcher runs; `addAgent` refuses a
  catalog name; `editAgent` on a catalog agent writes only overrides and
  drops the row when nothing differs; `resetAgent`; `removeAgent` is for
  custom agents; `normalizeRoster` folds legacy copies and drops profiles;
  a custom row with a catalog name and another harness hides the entry.
  The daemon normalizes at start. `cf agent reset`; no `cf agent sync`.
- [x] [IMPL-OR-02] Satisfies TEST-OR-01.

### Phase B: the Agents screen [done]

- [x] [TEST-OR-03] One row shape; no Add for catalog entries; Reset and
  Remove where they apply; Show all / mine; the model cards still share
  what their rows share.
- [x] [IMPL-OR-04] Satisfies TEST-OR-03.

### Phase C: the team dialogs [done]

- [x] [TEST-OR-05] Team and New project list every agent, grouped by
  harness, with the tier on each line.
- [x] [IMPL-OR-06] Satisfies TEST-OR-05.

### Phase D: the rest [done]

- [x] [IMPL-OR-07] README, the smoke test, the brain.

## TDD log

- 2026-09-23, Phase A: roster, cf, page and server tests rewritten; Node 666 passed in the gate.
- 2026-09-23, Phase B: the Agents screen's tests adapted or rewritten (30 with Harnesses); the board page's 50 still pass.
- 2026-09-23, Phase C: the pick lists grouped by harness with the tier on each line; board page 38/38 in its own file.
- 2026-09-23, Phase D: README, the packaged smoke test and the brain; the Candidate rebuilt on it.
