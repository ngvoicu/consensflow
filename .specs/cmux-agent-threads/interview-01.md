# Interview 01 — cmux-mode agent threads

Date: 2026-08-23. Round 1: findings presented, six questions posed, awaiting
answers.

## Assumptions stated

1. The harness owns the session; ConsensFlow only remembers its id. No daemon,
   no database, no supervised long-lived children. (User's constraint,
   confirmed viable in research-01 §2.)
2. Session ids are keyed by `(workspace, agent)` — the same key as the pane
   title, so the pane↔conversation binding needs no new mechanism.
3. The prompt-cache benefit is a consequence of resuming, not something we
   implement. (research-01 §5.)
4. The run directory stays the shared state between a pane and the lead;
   `recordLatestRun` already exists as the seam.
5. `cf run` remains a subprocess with a real exit code. Nothing scrapes a
   terminal.

## Questions posed to the user

1. **Scope shape** — cmux-only behaviour, or a `--thread` capability available
   in every mode that cmux merely defaults to ON?
2. **Reset policy** — when does a conversation end? Manual only, run count,
   age, or a combination?
3. **The one-root trade-off** — accept that harness session files live outside
   `~/.consensflow` and `cf off` / `cf reset` cannot remove them, or attempt
   per-harness cleanup?
4. **Concurrency** — two runs at one agent at once: refuse, queue, or fork?
5. **The lead's read verb** — shape and output of the command that lets the
   main pane read an answer produced in an agent pane.
6. **Roster drift** — a resumed session was created under an older model or
   effort; resume anyway, warn, or start fresh?

## Answers (round 1)

1. **Scope** — build the capability in every mode; default it ON in cmux only.
2. **Reset** — `--new` is the ONLY reset. No age cutoff, no run-count cutoff.
   A conversation lives until the user starts a new one.
3. **One root** — accept it. ConsensFlow never cleans another tool's session
   store.
4. **Concurrency** — superseded by a better idea from the user: **sessions get
   random memorable names**, so one agent can hold several conversations at
   once and each is addressable ("ask ares in bubble-sky about xyz"). No lock,
   no refusal; starting something new is a first-class move.
5. **Read verb** — print the answer; shape left to the implementer.
6. **Roster drift** — resume anyway. Consequence accepted and documented: the
   answer may come from the model the session was created with, not the one
   `cf agent list` currently shows.

## Answers (round 2)

1. **Panes follow sessions**, not agents. Two conversations with one agent are
   two panes. This reverses the pane-per-agent prose shipped in `aa7d77b`.
2. **Names**: two words, hyphenated, unique per workspace, drawn from concrete
   everyday vocabulary (weather, colour, nature, texture) and deliberately NOT
   mythology — so a session name can never be mistaken for an agent name.
3. **Lifecycle**: `cf off` keeps sessions (it does not touch `workspaces/`);
   `cf reset` removes them with the rest of the root. Accepted.
