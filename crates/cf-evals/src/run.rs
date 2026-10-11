//! The run of an eval (`evals/run.mjs` and `evals/scenarios/`, which the landing that
//! ports the evals replaces here): real harnesses as the chief and the staff of a
//! project on the native daemon, a scripted owner who answers what they ask, the gates
//! that tell a run is done, the switch of a chief to another harness half-way, and the
//! three arms (the card, the bare harness, the bare harness with a card). A scenario
//! is code, not data: its expectations are closures over the measure's numbers.
//!
//! It is built on `crates/cf-e2e`: the rig ([`cf_e2e::rig`]) and what the live tests
//! share ([`cf_e2e::live`]: the machine's harnesses, the trust Claude asks of a folder,
//! the windows, the trace of the daemon). Two things of the JavaScript are comments
//! only, and are kept: the owner presses Enter again, up to three times, unless the
//! trace shows the chief working, for Devin and for Codex on Windows, where a pasted
//! prompt waits in the box; and the wrapper every Claude and Codex window starts
//! through, first on the `PATH`, is a `.cmd` on Windows that names the Node and the
//! script (the shape of an npm shim, which is how the pane host opens a window on it),
//! and a shell script on the Mac and for a Pi chief, whose wrapper is the Mac's alone.
