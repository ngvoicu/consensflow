# What Node's daemon answered: the traces

These files were recorded from Node's daemon, which is gone, on every surface of
it: the agents' API, the page operations, the screens, and the native `cf`
against the API. They are fixed. Nothing records them again: they hold the Rust
daemon to what Node's answered, and a change that moves one is made in the file
by hand, on purpose, with the reason in the commit. The recorder put a wrapper in
the place of each module a suite imported, and each wrapper imported the real
one; it changed no outcome. It went with Node's daemon; it is in the flip release
`v3.0.0-alpha.82`, and in `21297242`, the commit before it was deleted.

`FORMAT.md`, beside this file, is the format of every file here and of the
traces, for the players that read them (`../api/`, `../page/`, `../screens/`,
`../files.rs` and `../support/`).

| Path | What |
|---|---|
| `<suite>-<NNN>.json.gz` | one trace per test that reached a surface |
| `pages/agents.html`, `pages/harnesses.html` | the two pages the screens served, `$TOKEN` and `$VERSION` where the token and the version go |
| `operations.json`, `daemon.json`, `files.json` | the page's operations (Node's 28 and the ones added since: `wakeup.get`, `wakeup.set`, `project.specs`, `project.rename`, `project.describe`, `specs.get`, `specs.set`, `specs.tree`, `specs.file`, `specs.present`, `specs.open`, `history.get`, `history.part`), the daemon's handle line and start and stop lines, and its log and trace line formats |

## Changed after Node

The tester (a role of a member, a pool of a task, `cf task add --test`) moved the
recordings in four ways, none of them by recording again.

- The usage `cf` prints has its entry for `--test`, in `cf-board-028` and `-029`, and the
  sentence `cf task add` says when it is given too little names it, in `cf-board-019`
  and `-021`: changed by hand, in those words alone.
- The refusal that lists the roles a member may hold lists the tester, in
  `corners-page-005`: changed by hand in the same way.
- Every trace that holds a database says `userVersion: 11` and a role check and a pool
  check without the tester, as Node's ledger had them (migration 0012 added the
  tester). They are not rewritten: the player holds the database this ledger left to
  them as Node's schema had it (`cf_ledger::testing::hold_apart_what_node_never_had_in_its_schema`).

`cf task edit` (board task T-110, 3.0.6) moved the usage `cf` prints once more, and the
sentence it says of a task verb it does not know: the usage has the entry `cf task edit
T-3 "…"` and one more line under `--after`, in `cf-board-028` and `-029`, and the sentence
names `edit`, in `cf-board-022`. Changed by hand, in those words alone. The recordings of
the crates' own goldens (`crates/cf/tests/goldens/board.json`) were changed with them.
Round 2 of the same task (T-158) changed one phrase of that entry, by hand, in `cf-board-028`
and `-029`: `(open, blocked or queued)` became `(open, queued, or paused)`.

`cf task list` with idle sessions (board task T-261, 3.0.6) moved the usage `cf` prints once
more: its line reads `cf task list [--all]` and says what the flag adds, in every print of it
in `cf-board-028` and `-029`. Changed by hand, in those words alone. The recordings of the
crates' own goldens (`crates/cf/tests/goldens/board.json`) were changed with them, and one
more there: the case `cf task list --json` carries an `idle` key (see that folder's README).
