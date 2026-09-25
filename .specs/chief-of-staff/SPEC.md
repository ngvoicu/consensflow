---
id: chief-of-staff
title: Chief of Staff and Staff — the lead and the team renamed everywhere, data included
status: in-progress
created: 2026-09-25
updated: 2026-09-25
priority: high
tags: [rename, chief, staff, ledger, ui, roles]
---

# Chief of Staff and Staff

Gabriel, 2026-09-25: "can we rename everywhere, even in the skill if
needed, the Lead to be called Chief of Staff? and Team could be Staff,
right? and everything to go into this paradigm?" → "da" to: `@chief` as
the handle, member roles unchanged, both layers now with a migration,
ConsensFlow and Agents keep their names.

## The vocabulary

| was | is | where |
|---|---|---|
| lead, Lead, `lead` | the chief, Chief of Staff, `chief` | role value, handle `@chief`, `skill/core/chief.md`, `roles/chief/…`, the skill `consensflow-chief` |
| team, Team, Session team | the staff, Staff | `cf staff`, `/api/staff`, `staff.last`, `project.open { staff }`, the Staff dialog |
| member, members | staff member, the staff | texts; `member` stays as the identifier for one staff member |
| worker, advisor, reviewer, designer, human, project | unchanged | |

In prose, "the chief" is the short form and "Chief of Staff" the title
(UI labels, the role texts' first line, the README's first mention).

## Decisions

- **The stored value changes too.** Migration 5 rebuilds `participant`
  with `chief` in the role check and rewrites `lead` to `chief` in
  `role` and `handle`. Message bodies written before it (a note saying
  `@lead`) stay as history. There is no production data: go-live starts
  clean (decision 2026-09-21), and the Candidate's home is development
  data.
- **One name for the file and the skill.** `skill/core/lead.md` becomes
  `chief.md`; the private skill a Claude window loads is
  `consensflow-chief`; a member's window finds its role text where it
  did.
- **Identifiers follow the words.** `teamOf` → `staffOf`, `lastTeam` →
  `lastStaff`, `renderTeam` → `renderStaff`, `#team-button` →
  `#staff-button`, and so on: the code says what the board says.
- **Rejected:** a display-only rename (the code would say lead where
  the board says chief, and whoever reads it next needs a dictionary);
  renaming the member roles (not asked; "staffer" says less than
  "worker").

## Phases

### Phase A: the core [done]

- [x] [TEST-COS-01] Migration 5 turns a version-4 ledger's `lead` rows into `chief` (role and handle), keeps every id and reference, and a fresh ledger creates the chief as `chief`; `COORDINATOR_*` say `chief`; `createProject({ chief })`; every core test says `chief`.
- [x] [IMPL-COS-02] Ledger, schema, daemon (`staffOf`), dispatcher, API (`/api/staff`, refusals' words), page (`staff.last`, `project.open { staff }`), CLI (`cf staff`, usage), role-skills (`chief`), the role texts (`chief.md`, `coordinating.md`, the members' texts), the fake agent and the integration suite. Satisfies TEST-COS-01.

### Phase B: the board [done]

- [x] [TEST-COS-03] The board page and the Agents screen say Staff and Chief of Staff wherever they said Team and Lead; the New project dialog picks the staff; the Staff dialog edits it; the chief's lane reads "Chief of Staff".
- [x] [IMPL-COS-04] `app/ui` (index.html, core/*.js), the UI tests, the packaged smoke. Satisfies TEST-COS-03.

### Phase C: the words around it [ ]

- [ ] [IMPL-COS-05] README, this spec's registry row, the brain: a decision record, the glossary, the status board; the memory notes that say lead.

## TDD log

- 2026-09-25, Phases A and B in one pass: every `lead`/`Lead`/`team`/`Team` word and identifier renamed across src, hosts, bin, skill, tests, evals, the board page and the Rust allow-list (`staff.last`); `skill/core/lead.md` → `chief.md`; migration 5 rebuilds `participant` with `chief` (a test opens a version-4 ledger with `lead` rows and reads `chief` with every id kept), and migrations now run with foreign keys off and a reference check after, since dropping the table had cascaded through task and message. Node 684/0, browser 84/84, Rust 111, integration 9/9.
