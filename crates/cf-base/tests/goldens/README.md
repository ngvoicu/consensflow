# What Node answered: cf-base's recordings

These files were recorded from Node, which is gone, and are fixed. Nothing
records them again: Node's CLI, its `path` and its libuv are what they hold the
Rust port to, and a change that moves one is made in the file by hand, on
purpose, with the reason in the commit. The recorders ran Node's modules and went
with them; they are in the flip release `v3.0.0-alpha.82`, and in `21297242`, the
commit before they were deleted.

| File | What Node answered | Read by |
|---|---|---|
| `args.json` | `util.parseArgs` over the words after each verb of the CLI, for 3,183 lists of up to three words, taken from the ones people get wrong; the same on every system | `../args.rs` |
| `errno/<platform>.json` | libuv's error names and words as Node printed them on that system (`util.getSystemErrorMap()`), a file a platform, `process.platform` naming it: the words are the same everywhere, the numbers are not (on Unix the negated errno of the system for the names that have one, and libuv's own otherwise; on Windows every number is libuv's own) | `../errno.rs` |
| `path.json` | `path.join` and `path.normalize`, for the POSIX and the Windows flavour both: every segment alone, every ordered pair, a seeded sample of triples, and each segment as a text of its own. The segments are the texts an environment variable may hold: dots and separators of either kind, drives, UNC and device roots, Windows' reserved names, and text outside ASCII (where JavaScript's lengths are in UTF-16 units) | `../path.rs` |
