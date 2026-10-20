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
| `operations.json`, `daemon.json`, `files.json` | the page's operations (Node's 28 and the ones added since: `wakeup.get`, `wakeup.set`, `project.specs`, `project.rename`, `project.describe`, `specs.get`, `specs.set`, `specs.tree`, `specs.file`, `specs.present`, `specs.open`), the daemon's handle line and start and stop lines, and its log and trace line formats |

## Changed after Node

The tester (a role of a member, a pool of a task, `cf task add --test`) moved the
recordings in four ways, none of them by recording again.

- The usage `cf` prints has its entry for `--test`, in `cf-board-028` and `-029`, and the
  sentence `cf task add` says when it is given too little names it, in `cf-board-019`
  and `-021`: changed by hand, in those words alone.
- The refusal that lists the roles a member may hold lists the tester, in
  `corners-page-005`: changed by hand in the same way.
- `cf tell`'s usage has a third line, for a task still queued (the field report of
  2026-10-15: a tell to a queued task waits for its window), in `cf-board-028` and
  `-029`: changed by hand, that line alone.
- The owner's decisions of 2026-10-16 on the same report: the usage has a line for the
  transcript's `--page` and `--find`, and its tail the lines of `--file`, in `cf-board-028`
  and `-029`; and every run of `cf task add`, `reopen`, `resume` and `tell` that printed a
  reply has a last line saying how many characters it sent and how they end, in
  `cf-board-004`, `-008`, `-009`, `-011`, `-014` to `-017`, `-020`, `-021` and `-022`:
  changed by hand, those lines alone, each written as `cf` writes it of the run's words.
- The second review of that work (2026-10-16): a task whose brief reached no window has
  no transcript of its own, and the board and `cf task get --transcript` say so, in
  `cf-board-015` (a task cancelled before any window had it): changed by hand, that
  answer's `"reached": false` and the line `cf` prints of it alone. The page
  traces whose drawer showed such a task the whole copy of the window its brief waited
  for (`core-page-024`, `-025` and `corners-page-008`) depart instead, as `../page/`
  names them.
- Every trace that holds a database says `userVersion: 11` and a role check and a pool
  check without the tester, as Node's ledger had them (migration 0012 added the
  tester). They are not rewritten: the player holds the database this ledger left to
  them as Node's schema had it (`cf_ledger::testing::hold_apart_what_node_never_had_in_its_schema`).
