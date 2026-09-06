# Interview 01 — ConsensFlow owns the panes, and delivers results to the lead

Date: 2026-09-05. Round 1: research-01 presented, six questions posed,
awaiting answers.

## What the research changed (three reversals, owned)

1. **Emulator.** Recommended earlier: wterm first, xterm.js as fallback. The
   research (research-01 §4.3) read wterm's issue tracker: the built-in core
   ignores DECSTBM (scroll regions), ignores mouse tracking, clamps resize at
   256×256; every fixed vim bug lives in the ghostty core, which is 400 KB and
   four months old. **Now:** ship xterm.js 6 behind a four-method interface
   (`write`, `onData`, `resize`, `dispose`) and keep a wterm adapter as the
   second implementation, re-evaluated in a quarter.
2. **Where the pane API lives.** Recommended earlier: a token-gated loopback
   listener in Rust. The research (§4.1.1) found the Node↔Rust stdin/stdout
   pipe already exists and carries one line. **Now:** JSON-lines over that
   pipe; Node's server stays the only HTTP server, the only token, and the
   only enforcer of "named operations only". No second port.
3. **Which origin owns the page.** The window loads the Node server's origin
   (`WebviewUrl::External`), and Tauri v2 IPC — including the `Channel` that
   streams PTY bytes — is only available to the app's own origin. **Now:** the
   pane UI is served from the Tauri origin and the roster editor is kept
   verbatim in an iframe on the Node origin (§4.1, option A). Probe in
   phase 1: an `http://localhost` iframe inside a `tauri://` page under the
   existing ATS exception.

## Assumptions stated (from the user's own words, not questions)

1. A conversation is the unit. One window per conversation is a standing
   invariant, so "per pane" and "per conversation" are one setting, stored on
   the thread row.
2. The human's setting beats the lead's. Two fields: `notify: auto|manual`
   and `notifySetBy: human|lead`. `cf run --notify` writes only when
   `notifySetBy !== 'human'`; the UI always writes and marks `human`.
3. Pull stays. `cf catchup` and its `seen` marks are unchanged; a delivery
   moves the lead's mark through the same `markRead` shape, or `--unread`
   re-shows what was just pushed.
4. The human can chat with a worker in its pane, and right-click **Send to
   lead** on a chosen reply. Automatic and manual delivery share one write
   path: bracketed paste, then a separate `\r`.
5. Automatic delivery fires only for a worker turn that answers a turn WE
   sent (`cf run` / `cf say`); a human-driven exchange in the pane is
   manual-only, so a ten-turn human chat does not push ten notices.
6. Pane title = conversation name · agent · delivery mode, from the row.
7. The tiling progression is fixed: 2 side by side, 3 second column split,
   4 a 2×2 grid, and so on. CSS grid, no layout engine.
8. Mode selector at the top, collapsible; claude/pi show today's editor;
   standalone shows the sidebar and the pane area.
9. No daemon. The app process is the long-lived one; the delivery watcher
   lives in the Node server the app already runs (quiescence hint from the
   worker's PTY, the harness store as the truth, a slow floor poll, the
   `--wait` grace).
10. The screen is never read. PTY silence is a hint; `harnessTurns` decides.
11. `childEnv` strips whatever variable hands `cf` the pane id/token, in the
    same function that strips `CMUX_SOCKET*` today. Every new pane verb is
    guarded by `CONSENSFLOW_CHILD`.
12. The three cmux seams (`CMUX_SURFACE_ID` as lead key, `surface` on the
    row, `cmux tree` for liveness) are replaced, keeping the fail-open
    contract. The window path (`openWindow`, `handOver`, the piped-run
    refusal) is not rewritten: a PTY child passes `isTTY`.
13. Tests: Rust `#[cfg(test)]` PTY tests with `sh -c`; a stub pane server in
    `tests/` for `cf`'s side; delivery rules as pure functions over rows and
    turn fixtures; Playwright in `app/` for the page, outside `npm test`;
    the evals rewritten in the same change as the skill.
14. Windows: codex is WSL-only per first-party docs; the launcher, sidecar
    triple, `rsync`, `open`, `env -u` and `#!/bin/sh` stubs are listed in
    §7.2. App-owned panes remove the `env -u` blocker because `childEnv`
    strips keys in-process.

## Questions posed to the user

1. **Notice or full answer?** The user said "send the result". The research
   recommends pushing ONE line — `@nyx answered in nyx-coral-lane (3 new
   turns) — cf catchup nyx-coral-lane --unread` — and leaving the body to
   `cf catchup`. Two reasons from this repo's own record: `boundedAnswer`
   exists because 493,390 characters were once pasted into a lead, and
   Claude Code's queue flushes at the next tool boundary, so a long body
   landing mid-task derails the lead while a one-line note reads as a note.
   Cost: the lead runs one extra command. If the full body must be pushed,
   the spec gains a truncation policy and risk 1 grows.
2. **Is `standalone` the new name of `cmux`, with cmux support ending?**
   Today `standalone` is the alias for `cmux`. (a) rename, cmux becomes the
   alias, cmux support ends on a date; (b) a fourth mode, two liveness paths
   and two lead-id sources forever. Recommendation: (a).
3. **Default `notify` for a new conversation: `auto` or `manual`?** The
   user's phrasing implied auto. The research argues manual: pi and kimi
   have no data on mid-turn stdin at all, and auto on a harness that
   swallows the `\r` looks exactly like a working feature that never
   delivers. Proposal: `manual` until the phase-1 probe passes for a
   harness, then `auto` per harness.
4. **Roster editor: kept verbatim in an iframe, or rebuilt in the new
   page?** Keeping it preserves "one implementation of the editor";
   rebuilding retires that rule deliberately.
5. **A lead outside the app gets no push in v1?** A lead in iTerm with its
   workers in the app can be identified but not written to. Options: (a)
   push only when the lead's own pane is in the app — the only option with
   no silent failure; (b) a file the lead's `cf` polls — a daemon in all but
   name. Recommendation: (a).
6. **"Delete CLAUDE.md and AGENTS.md" — the files, or today's paragraph?**
   Both files carry the load-bearing rules and a paragraph added today.
   Nothing was deleted; the answer decides what is.

## Not settled by research — proposed as round 2 (live probes on this machine)

- Does a bracketed-paste body plus a separate `\r` submit, per harness
  (claude, codex, pi, opencode, kimi)? Does the TUI enable the kitty
  keyboard protocol, and does a raw `\r` still submit when it has? This
  decides whether push is viable at all for some harnesses. It spends
  tokens and opens real windows.
- Does an `http://localhost:<port>` iframe load inside a `tauri://` page
  under the existing `exceptionDomain`?

## Answers (round 1) — 2026-09-06

1. **Full answer.** ("răspuns întreg") Overrides research-01 §5.4; absorbed
   by a bound, an idle gate, and a per-harness downgrade to notice when P1
   fails — research-02 §1.
2. **cmux disappears.** `standalone` is the mode's real name, `cmux` the
   alias; support ends with Phase 5 — research-02 §2.
3. **`auto`.** Default auto; unprobed lead harnesses deliver a notice —
   research-02 §3.
4. **"Whatever is cleaner."** Resolved by the implementer: the roster editor
   stays in an iframe — research-02 §4.
5. **"I don't understand — every pane will be in ConsensFlow."** Read as: the
   app is the whole workspace, a tab is a directory plus one lead pane, and
   there is no lead outside the app — research-02 §5.
6. **Delete the files.** `CLAUDE.md` and `AGENTS.md` removed 2026-09-06
   (plain `rm`; git records the deletion). The rules they carried live in
   `research-01.md` §2.1 (invariants, with file:line) and this spec.

The `.specs/` deletions of the four completed specs were not answered;
their registry rows are kept and marked `(SPEC.md missing)`.

## Round 2 — the co-lead review and the answers (2026-09-06)

The spec went to astraeus (`astraeus-lilac-dune`, codex, MAX effort) with
Gabriel's requirements translated and seven points to attack. Verdict:
"BLOCKED — the spec is not implementation-ready", with the requester/in-pane
split, Rust PTYs and the reuse of the Node↔Rust pipe called sound. Sixteen
findings; read whole with `cf catchup astraeus-lilac-dune`. The four that
were Gabriel's to decide, and his answers:

1. **Cap on a delivered answer?** — "the lead absolutely must read
   everything." No cap. Inline when the harness admits the paste, else a
   file the lead is told to read in full; never a notice, never silence.
2. **What `auto` means?** — every completed worker reply, including replies
   to the human's own questions in the pane. `row.sent` is no longer a gate.
3. **Two policy scopes (tab and pane) with precedence?** — yes.
4. **Layout counted with the lead (2 beside, 3 lead full-height + 2 stacked,
   4 a 2×2)?** — "the first, as I said."
5. Added mid-turn: **Windows native only; WSL is out of scope.**

The other twelve findings were taken as written into revision 2 of SPEC.md:
launch tickets and role-scoped credentials, Node as the serialised writer,
ownership from the ticket, a completion model per harness, readiness from
the lead's own transcript, delivery records with receipts, the transcript-
backed "Send reply to lead…" list, tab persistence and process-tree
lifecycle, a versioned bridge protocol, xterm bundled with acks, an offline
end-to-end suite, and codex native on Windows (verified against OpenAI's
docs the same day).

## Round 3 — astraeus's second review, and the scope decision (2026-09-06)

Second verdict, quoted: "BLOCKED — revision 2 is substantially stronger,
but still has contradictions that could produce duplicate launches, unsafe
submissions, or silently missed answers." Of the sixteen: 7 closed, 8
partial, 1 open; twelve new findings with edits. All twelve are in
revision 3. Two resolutions are the implementer's, logged in the Decision
Log: PTY silence is no longer a readiness condition (the lead's transcript
and the draft latch decide, and the latch clears only on an observed
submission); file delivery is `cf read <id>`, which prints everything and
records coverage.

Gabriel: "no Windows machine; we'll do it later; macOS for now." Windows
and Linux packaging move to a later spec; every choice stays compatible.

## Round 4 — astraeus's third review (2026-09-06)

Third verdict, quoted: "BLOCKED — revision 3 is close, but not
implementation-ready as written. Eight of the twelve findings are closed;
four remain partially open." And: "After those four contract edits, I
would proceed with the first two phases." The four, all in revision 4:
`cf read` records an attempt, coverage needs every part's end marker in
the lead's transcript (pi keeps only the tail of a large tool output);
`draft.clear` carries generation and the submitted epoch so a delayed
clear cannot erase a newer draft, and Deliver now never bypasses a latched
draft; an in-place native session replacement invalidates the binding and
suspends delivery; the receipt digest covers an envelope that embeds the
delivery id. Two guards: completion extraction never passes through the
8 KiB display normaliser; native adapters are built only after their probe
passes, and P6 covers an inbox arrival while already idle. Phase 1 and 2
exit evidence recorded as he listed it.

A process note: the message announcing revision 4 reached astraeus
before the commit existed — the edit script had failed on an anchor and
nothing had been written. A correction followed with the real commit.

## Round 5 — READY (2026-09-06)

astraeus, quoted: "READY for phases 1 and 2 at `d1b8ef2`, reread from
disk." One correction before Phase 3, applied the same day: a `cf read`
part is covered only when its complete framing appears in the lead's
model-visible tool result and the digest of the observed body equals the
immutable part — an end marker alone never establishes coverage, because a
receiver keeping only the tail keeps the marker and drops the text. Phase
2 records attempts without granting coverage, so neither phase is blocked.

## Addition after READY (2026-09-06) — the lead does not wait

Gabriel: in the current skill the lead waits for workers to answer; in the
new skill it must be told that answers come automatically, or that the
owner will say when to read. Not implementing yet. Recorded as an
acceptance criterion, an Architecture paragraph, the skill test
(TEST-PANE-49) and a new eval scenario `a-lead-sends-and-returns`.
