# What Node answered: cf-catalog's recordings

These files were recorded from Node's catalog and roster, which are gone, and
are fixed. Nothing records them again: they hold the Rust catalog and roster to
what Node answered, and a change that moves one is made in the file by hand, on
purpose, with the reason in the commit. The recorders ran Node's modules and went
with them; they are in the flip release `v3.0.0-alpha.82`, and in `21297242`, the
commit before they were deleted. (`../../data/presets.json` was recorded the same
way, but it is the catalog's source now: see `../../data/README.md`.)

| File | What Node answered | Read by |
|---|---|---|
| `catalog.json` | the catalog: each entry by name, the efforts, the work tiers, the harnesses and each kind's harness | `../goldens.rs`, `../catalog/` |
| `profiles.json` | `agentProfile` over a corpus of agents | `../goldens.rs` |
| `roster.json` | each roster operation on a file, at a fixed time: the file before and after, and what it answered or refused | `../roster_goldens.rs` |
| `unwritable/<platform>.json` | what the roster says when it cannot save, played on the real roster over a temporary root with the folders, files and modes of each situation; a file a platform, since the system names its own errors. In a message the root is `$ROOT` and the process id `$PID` | `../unwritable.rs` |

A value JSON cannot hold, `undefined`, is written `{"$undefined":true}`. Inputs
that made the JavaScript throw a TypeError (a row that is no object, a model that
is no text) were left out and counted: the Rust refuses them as a file that is no
agents file, a difference kept on purpose. The roster's errors name its file
`«home»/agents.json`.

On a Unix system, the roster reads a file before it writes it and refuses one it
cannot read, so a folder that is a file, a file in the way of the folder and a
directory at the file all stop at the read; on Windows a path through a file is
`ENOENT`, which the roster reads as a missing file, so those situations reach the
write, and the `win32` golden records what happens there.
