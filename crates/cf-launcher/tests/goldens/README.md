# What Node's terminal command said: cf-launcher's recordings

These files were recorded from Node's terminal command (the launcher that
`cf setup` and the app wrote, and the report of its stale Claude hooks), which is
gone, and are fixed. Nothing records them again: they hold the Rust launcher to
what Node said, and a change that moves one is made in the file by hand, on
purpose, with the reason in the commit. The recorder ran Node's modules over a
throwaway home and went with them; it is in the flip release `v3.0.0-alpha.82`,
and in `21297242`, the commit before it was deleted.

None is keyed by a platform: what a platform cannot make is left out of what it
records, and the replay leaves the same out.

| File | What Node said | Read by |
|---|---|---|
| `installs-cmd.json` | `installTerminalCommand` in the form of cmd.exe (`OS=Windows_NT`, which any system can make, and which Windows makes whatever its environment says) | `../goldens.rs` |
| `installs-sh.json` | the same in the form of `sh`, which Windows cannot make, with the cases whose words are a POSIX system's own | `../goldens.rs` |
| `readings.json` | `terminalRuntime` over the texts a command may hold: what it read, `{ runtime, entry }`, or null; and `dirnames`, what `path.posix.dirname` says of each path, which is how a bundle is found from the entry | `../goldens.rs` |

Each case of an `installs` file is `{ name, pin, path, place, before, after,
installed, error }`:

- `pin`: whether the environment names a `CONSENSFLOW_HOME`;
- `path`: where the folder is on `PATH`: `on`, `off`, `among` others, or `near`,
  with folders that begin or end like it and none that is it;
- `place`: the one candidate: `bin` (a folder, there or not), `blocked` (under a
  file) or `file` (a file where the folder goes);
- `before`: what the folder held, `{ name, text, mode }` or `{ name, directory }`,
  by name without `.cmd`;
- `after`: what it holds, the same, with `executable` where there are modes, and
  the `text` as Node wrote it, with `<runtime>`, `<cli>` (Node's CLI entry) and
  `<home>` for what is the machine's; none when there is no folder;
- `installed`: what the call answered, `{ name, onPath }` or null;
- `error`: what it threw, with the root of the home as `$ROOT` and every
  separator `/`, or null.

The Rust launcher keeps one difference on purpose, and says it once (`kept` in
`../goldens.rs`): Node's launcher ran a runtime and its CLI entry, and this one
runs the native `cf`. The stale-hook report (`stale-hooks.json`) is
`crates/cf-harness/tests/goldens/claude/`'s.
