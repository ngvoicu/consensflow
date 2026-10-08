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
| `operations.json`, `daemon.json`, `files.json` | the page's 28 operations, the daemon's handle line and start and stop lines, and its log and trace line formats |
