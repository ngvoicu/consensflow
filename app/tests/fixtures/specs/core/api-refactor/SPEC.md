---
id: api-refactor
title: API Refactoring
status: paused
created: 2026-09-20
updated: 2026-10-09
priority: medium
---

# API Refactoring

## Overview

Split the handlers by resource and move validation to the schema layer.

## Phase 1: Handlers [in-progress]

- [x] [API-01] One module per resource
- [x] [API-02] Shared error type
- [ ] [API-03] Schema validation ← current
- [ ] [API-04] Remove the old router

## Phase 2: Clients [pending]

- [ ] [API-05] Regenerate the TypeScript client
- [ ] [API-06] Regenerate the Python client
- [ ] [API-07] Deprecation headers on v0
- [ ] [API-08] Remove v0

---

## Resume Context

> Paused for [user-auth](../user-auth/SPEC.md): its login route is the first
> to move.
