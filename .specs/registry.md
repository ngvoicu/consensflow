# Spec Registry

| ID | Title | Status | Priority | Progress | Updated |
|---|---|---|---|---|---|
| devin-and-receiver-cleanup | Devin roles and receiver integration cleanup | active | high | 2/6 | 2026-09-12 |
| consensflow-v3-skills-first | ConsensFlow v3 — Skills-First (SPEC.md missing) | completed | high | 12/12 | 2026-08-19 |
| cmux-agent-threads | Agent Threads — named, resumable conversations (SPEC.md missing) | completed | high | 16/16 | 2026-08-23 |
| cmux-attached-consults | Attached consults — the pane IS the agent's window (SPEC.md missing) | completed | high | 10/10 | 2026-08-24 |
| kimi-harness | Kimi Code as a fifth harness (SPEC.md missing) | completed | high | 8/8 | 2026-08-24 |
| standalone-panes-delivery | ConsensFlow owns the panes — standalone mode in the app, results delivered to the lead | completed | high | 272/272 | 2026-09-12 |
| agent-catalog-redesign | Coherent agent catalog and roster browsing | completed | high | 42/42 | 2026-09-10 |

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

Phase 24 complete: alpha.43 published on GitHub; source b34bd42 and version/feed
tags pushed to NAS and GitHub. All five release assets downloaded and verified;
Alpha feed returns HTTP 200. Real native updater: alpha.42 sees/downloads the
release, alpha.43 is current. Stable remains unpublished. Documentation-only
publication evidence follows the immutable release commit on both main branches.

Phase 25 active: separate top-level Harnesses screen; remove off/reset globally and obsolete system facts. User confirmed both scope choices. Reinstall after verification.

Phase 25 complete: alpha.44 installed locally, separate Harnesses screen and off/reset removed everywhere. Focused Node 82/82, UI 93/93, packaged smoke 1/1; 31 saved-state hashes unchanged. Full Node retains only the known lifecycle failure on sequential rerun; three unrelated baseline lint errors remain. Alpha.43 remains the published release.

Phase 26 active: remove confusing Harnesses receipt diagnostics and their code; clarify documentation links and Pi setup failures. Reinstall locally.

Phase 26 complete: alpha.45 installed locally. Removed receipt diagnostics and their code, documentation links and command-reference section. Focused Node 93/93, browser UI 93/93, packaged smoke 1/1; 33 saved-state files and all 58 bundle files verified. Actual CLI and lead instructions remain.

Alpha.53: agent catalog complete at 40/40 tasks, installed locally.
Gemini 3.1 Pro Preview retired; Muse Contributor/free routes grouped; labeled
model-level AA scores bring coverage to 85/100 choices. Native cards verified.
121 browser, 149 Node/engine (3 platform skips), 2 packaged tests pass. All 14
saved execution configurations preserved; display profiles refreshed. Pygmalion
remains Codex Images, as current docs identify GPT Image 2 for the built-in route.

Current: Phase 20 complete, 42/42 tasks; alpha.54 installed and native UI verified.
Kimi 2.7 retired; K3 Max preset and Low/High/Max child effort transport fixed.
98 choices, 36 cards, 86 scored. 196 Node/engine passed (3 platform skips),
122 browser, 75 refactor, 2 smoke and 8 real-CLI local probes passed. All 13
saved agents, 51 original state/private files and Kimi config unchanged;
59 installed bundle hashes and signature verified. No publication or commit.

Current: standalone Phase 27 complete (206/206), alpha.55 installed locally.
Stalled OpenCode readiness reads retry within bounded startup; task posts once.
309 Node, 2 UI and 2 packaged checks pass. Real free Gefjon response verified;
59 bundle hashes/signature match, saved agents and native histories preserved.

Current: Phase 28 complete (213/213), alpha.57 installed locally. Same-native
lead resume preserves reply routing; idle Pi startup restores settlement.
The user's exact SQL reply reached the original lead automatically once,
verified in native history and the installed UI. Store 62/62, Pi/channel/watcher
159/159, browser 6/6 and packaged smoke 2/2 pass; the existing lifecycle count
assertion remains (broader regression 268/269). All 59 bundle hashes/signature
verified; saved agents and 87 native conversation identities preserved.

Current: Phase 29 complete (226/226), alpha.58 installed. Live Pi conversation
identity follows /new and /resume; worker context survives later app resume.
Real native /new probe passes with no remote calls. Installed Gefjon result
d-122 arrived automatically in the user's actual new Pi conversation exactly
once. Focused Node 230/230, UI 6/6, packaged 2/2. Final full Node: 1278 passed,
six skips, the one known lifecycle assertion failure. All 59 bundle hashes and
signature verified; 55 state/private files unchanged across replacement,
saved agents and all 88 worker native identities preserved. No publication.

Current: Phase 30 complete (229/229), alpha.59 installed. Native terminal size waits for startup and resizes serialize. Browser 126/126, related Node 154/154, native PTY 1/1, packaged smoke 2/2 pass. Actual OpenCode p-197 fills the pane at 204 columns by 58 rows with no internal error toast. Same native lead, agents and 88 worker identities preserved; 59 installed bundle hashes verified. No publication.

Current: Phase 31 complete (242/242), alpha.60 installed and running. All-four installed native new/resume/app-restart matrix: 16 complete automatic receipts, one attempt each, with local providers. Node 1307 pass / 0 fail / 6 skips; browser 127/127; packaged smoke 2/2. Signature/81 bundle hashes verified, 57 state files unchanged at replacement, all 90 original native worker identities preserved. Codex follows the main lead through its bundled native broker. Unchanged whole-tree formatting error and a separate zero-latency Claude transcript-order finding are documented in SPEC.md; neither is represented as passing. No publication or commit.

Current: Phase 32 complete (247/247), alpha.61 installed. Claude late-written ancestor completion fixed; user live testing follows. Node 1326 pass/6 skips/0 fail; Rust 92/92; packaged smoke 2/2. Private runtime storage enforced for launcher/socket/updater exceptions; saved agents and 90 worker identities preserved. Debug build output removed (20.38 GiB). No commit/publication.

Design recorded: PM-owned advisors for reading/web research/planning/review/testing; all findings go to the PM, and only the PM writes/revises specifications; one session with separate PM and Lead grids. Implementation tasks are not yet added; alpha.61 remains the installed build. See the final design note in standalone-panes-delivery/SPEC.md.

Phase 33 active: implement the confirmed PM advisor and PM/Lead grid design; alpha.61 remains installed until verification.

2026-09-12: user requires current running app remain untouched. Prepare isolated source/build verification only; reinstall is deferred. Claude continued-in routing investigation added without live-state mutation.


2026-09-12 current: standalone receiver migration and PM advisors are complete at 272/272 in a separate candidate. Installed alpha.61 and active sessions are untouched. Full Node 1077 passed / 6 gated skips; browser 122, real bridge 19, Rust 109, packaged smoke 2; lint and Clippy pass. Stock native receiver checks confirmed 24 replies / 34 parts across Claude Code, Codex, Pi and OpenCode using only local mock providers; crash-window replay recovered all without resending. [Candidate acceptance, isolated launcher and profile](standalone-panes-delivery/acceptance-receiver-pull-candidate.md). No commit, publication or installation.

2026-09-12 Devin research complete: installed 3000.6.14 and private latest 3000.10.21
passed 72 native local-mock assertions, including repeated full replies, new/load,
cancellation and owner replacement. Recommendation is one ConsensFlow ACP pane
using the shared inbox; stock-TUI hooks cannot provide complete idle collection.
Devin integration remains active at 2/6; no installed app or current session changes.
[Decision and native evidence](devin-and-receiver-cleanup/research-native-transport.md).

2026-09-12 follow-up: user prefers exploring stock Devin TUI; ACP chat UI is not
selected. Eleven actual TUI scenarios / 54 assertions establish repeated native
hook collection and next-prompt recovery, with cancellation delayed by a waiting
Stop hook. Full automatic idle wake remains unresolved. [TUI findings](devin-and-receiver-cleanup/research-devin-tui.md).
