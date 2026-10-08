# What Node's CLI said: cf's recordings

These files were recorded from Node's CLI, which is gone, and are fixed. Nothing
records them again: they hold the Rust `cf` to what Node's CLI said and did, and a
change that moves one is made in the file by hand, on purpose, with the reason in
the commit. The recorders ran Node's CLI and went with it; they are in the flip
release `v3.0.0-alpha.82`, and in `21297242`, the commit before they were
deleted.

| File | What Node's CLI did | Read by |
|---|---|---|
| `cli.<platform>.json` | the standalone verbs (`--help`, the version, `catalog`, `agent`, `setup`, `doctor`): every scenario played against Node's CLI, `darwin` for macOS (and any system but Windows), `win32` for Windows, 374 cases each: what it printed, said on the error output and exited with, and the files it left. `FORMAT.md` says what is in them | `../cli_goldens/` |
| `board.json` | the board commands, which a window's token makes `cf`: each case served by a scripted API, with the requests Node's board made, what it printed, its errors and its exit code. Recorded at `b54361c` from Node's `runCoreCli`, a commit that holds that code and the scripted API | `../board_goldens.rs` |

The words after each verb were recorded too, through Node's `util.parseArgs`:
`crates/cf-base/tests/goldens/args.json`.
