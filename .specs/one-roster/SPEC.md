---
id: one-roster
title: One roster — every catalog agent is an agent, as the catalog has it; teams pick from all of them
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
- **A catalog agent is exactly what the catalog ships.** It is never
  edited or removed: Edit and Remove exist only for the agents defined by
  hand, and `agents.json` keeps only those, in full. A release that moves
  an entry reaches every field, because there is nothing of the human's
  on it. Whoever wants a catalog model with other settings defines their
  own agent. *(First cut the same afternoon: edits as overrides on a
  catalog agent, with Reset. Gabriel: "why do we have edit button on
  agents that are premade?" Replaced within the hour.)*
- **Old files read the same.** A row an older ConsensFlow saved from the
  catalog (a full copy with `preset`) reads as the catalog entry; the
  daemon drops such rows at start, and the profile it used to store. A
  custom row that happens to carry a catalog name with another harness
  hides that catalog entry.
- **One row per agent on the Agents screen**, in every sort and grouping:
  name, model · harness · effort, the tier pill, the route, and Edit and
  Remove when it is your own. Search, work tier, group by and sort by
  stay. "Show" offers all, or only your own.
- **The team dialogs offer every agent**, grouped by work tier (T1 ·
  Critical work first), harness by harness and by name within a tier, each
  line `name · model · harness · effort`; a member's row says
  `model · harness · effort · tier`. *(First cut: grouped by harness with
  the tier on the line. Gabriel: "in team i need to see the agent's effort
  level; model; and the dropdown list must be sorted by criticality".)*
- **Rejected:** a custom agent shadowing a catalog name on purpose (Add
  refuses catalog names); a saved list beside the catalog (that was the
  screen that looked bad).

## Phases

### Phase A: the roster [done]

- [x] [TEST-OR-01] `listAgents` lists the catalog and the custom agents;
  `agentRow` gives the row the launcher runs; `addAgent` refuses a
  catalog name; `editAgent` and `removeAgent` refuse a catalog agent and
  write nothing; `normalizeRoster` drops legacy catalog copies and
  profiles; a custom row with a catalog name and another harness hides
  the entry. The daemon normalizes at start. No `cf agent sync` or
  `reset`.
- [x] [IMPL-OR-02] Satisfies TEST-OR-01.

### Phase B: the Agents screen [done]

- [x] [TEST-OR-03] One row shape; no Add for catalog entries; Edit and
  Remove only on your own; Show all / mine; the model cards still share
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
- 2026-09-23, later: the pick lists regrouped by work tier, T1 first, and the Team dialog's rows say what each member runs; the fixture gained a T1 agent last in the roster to prove the order; board page 38/38.
- 2026-09-23, later still: the rows of both dialogs read by role, then by tier, then by name (`99ea554`); a critical reviewer ahead of the workers and a critical worker last among the picks prove it; board page 38/38.
- 2026-09-23, Phase D: README, the packaged smoke test and the brain; the Candidate rebuilt on it.
- 2026-09-23, later: catalog agents read-only (no Edit, no overrides, no Reset); roster, cli, server, smoke, page and Agents-screen tests rewritten for it; Harnesses+Agents 30/30, board page 38/38.
