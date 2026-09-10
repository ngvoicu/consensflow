# Spec Registry

| ID | Title | Status | Priority | Progress | Updated |
|---|---|---|---|---|---|
| consensflow-v3-skills-first | ConsensFlow v3 — Skills-First (SPEC.md missing) | completed | high | 12/12 | 2026-08-19 |
| cmux-agent-threads | Agent Threads — named, resumable conversations (SPEC.md missing) | completed | high | 16/16 | 2026-08-23 |
| cmux-attached-consults | Attached consults — the pane IS the agent's window (SPEC.md missing) | completed | high | 10/10 | 2026-08-24 |
| kimi-harness | Kimi Code as a fifth harness (SPEC.md missing) | completed | high | 8/8 | 2026-08-24 |
| standalone-panes-delivery | ConsensFlow owns the panes — standalone mode in the app, results delivered to the lead | active | high | 196/197 | 2026-09-10 |

Phase 19 is complete: alpha.37 is installed and source is integrated into the
original checkout. Native delivery, persistent results, PM window, updater and
all applicable automated gates passed. Detailed evidence: [alpha.37 acceptance](standalone-panes-delivery/acceptance-alpha37.md).
Private bundled skills are role-scoped: `consensflow-lead` and `consensflow-pm`.
Claude Code, Codex, OpenCode and Pi support lead/PM; Kimi is worker-only.
Pi's private per-process extension is approved and prepared only if Pi exists.
Harness version/update diagnostics are informational and never block operations.
The five owned old global skills were removed manually; the app neither installs
nor removes global skills. Native result reads and worker navigation survive restart.
User authorized reinstall/checks after closing the app; preserve histories and
profiles. No commit or publication authorized.

Phase 20 active: focused pane height and worker activation on session resume regressions.

Pending alpha.38 also includes confirmed permanent pane deletion, including closed sidebar panes.

PM layout correction: PM is above lead in the sidebar and uses the main window, outside the session grid. Separate PM windows are retired.

Alpha.41 is installed, including the PM main-window change and delivery tracking fixes.

Latest: alpha.41 installed. Live receipt matrix 12 passed; eight provider-blocked combinations. See [acceptance](standalone-panes-delivery/acceptance-alpha41.md).

Phase 22 complete: alpha.42 installed and reopened with saved conversation
identities preserved. Full PM/lead startup instructions for all four harnesses;
role suite 21/21, packaged smoke 1/1, session-concurrency 3/3. Native PM checks
passed for all four harnesses: OpenCode/Pi reruns used Gefjon's explicit free
Muse model, superseding the earlier default-model timeout/quota results.
Broader verification: Node 1221 passed, one existing lifecycle failure, five
skipped; integration 24/25 with one timing-sensitive failed-worker assertion.
See Phase 22's full record. Progress now counts actual task checkboxes.

Phase 23 complete: alpha.43 installed with the duplicate Updates header button
removed. Native-menu event tests and channel-specific unavailable-feed feedback
pass. UI 89/89, Rust updater 8/8, Node updater 16/16, packaged smoke 1/1.
Existing state and Stable preference preserved; all 57 bundle file hashes match.
Both public feeds still return 404; no signed release/feed has been published.
No GitHub release or NAS push was performed.

Phase 24 active: user authorized commit, GitHub publication and NAS push for
alpha.43. Prepare exact installed artifacts; publish only the Alpha feed.

Alpha.43 artifacts prepared and verified; packaged upgrade passed. Publication
is current. Full Node retains the disclosed lifecycle count assertion failure.
