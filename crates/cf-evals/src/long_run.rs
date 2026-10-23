//! The long run: a real project worked for hours by a chief and a staff of every
//! harness, with ConsensFlow watched the whole time. The serious bugs of 3.0.3 and
//! 3.0.4 all came from long runs on real projects, which no suite played: a task
//! left working after its worker finished (A1), an unfinished turn taken for the
//! result while a background job ran (A2), a job ended mid-edit with its notice
//! after the result (A3), a result holding every progress line (A4), a false "turn
//! was cut" after an idle compaction (A5), a resumed Devin window that could not
//! write (B1), windows closing mid-task (B2), a result delivered twice (B5). Each
//! was found by the owner, not by a test. This is that test.
//!
//! ```text
//! cargo xtask eval long [--chief claude] [--staff claude,codex,pi,opencode,devin]
//!                       [--hours 2] [--quiet] [--restart] [--kill] …
//! ```
//!
//! It is an eval of a new kind, and lives with the evals: everything a run of them
//! is made of is what it needs (the machine's harnesses on their own logins, the
//! wrappers that shut out MCP servers and the browser, the fixture copied into a
//! trusted workspace, the rig of the native daemon and the real pane host, the
//! scripted owner who answers in the chief's terminal, the kept report, and the
//! dry run on the stand-in agent), and what it holds is read as the measure reads a
//! ledger, with SQLite and none of the product's code. `watch`'s one look at a
//! running project is made continuous: a look every few seconds ([`look`]), held to
//! ten invariants ([`invariants`]), the last of them that each disruption has its
//! effect, and once more after the app has closed. The live suites of `crates/cf-e2e` are tests of
//! a few minutes with their flags in variables, the wrong shape for hours with
//! flags, disruptions and a report.
//!
//! The parts, each its own module:
//!
//! - [`options`]: what a run is asked to do; [`staff`]: the members, on the models
//!   each harness has, and the harnesses left out and why; [`ask`]: the project's
//!   ask, the owner's answers and follow-up, for real chiefs and for the stand-in.
//! - [`ledger`], [`record`] and [`look`]: what one look reads; [`invariants`]: what
//!   it is held to, and the breaks, each recorded once with its evidence, what looks
//!   like a key or a token in it hidden ([`mask`]).
//! - [`disruptions`]: when each disruption is played, at a moment that makes its
//!   shape real; `play` plays one on the rig, and sees its effect.
//! - `stage` (the rig and the live look), `watcher` (the look, the exits at the
//!   host's time), `tracker` and [`processes`] (the processes of the windows, and the
//!   strangers, each said and left running: the run signals only what it started,
//!   through the handle it holds) and `conduct` (the run, look by look).
//! - [`timeline`] and [`report`]: what the run keeps, JSON and Markdown; `stalls`: what
//!   it keeps of a window that read working or starting too long, and of every window
//!   at an end the project did not reach, with the ledger copied beside the report.
//!
//! **This spends real quota** on every harness of the staff, for hours, and is no
//! part of any gate. The dry run on the stand-in agent spends nothing
//! (`tests/long_dry_run.rs`).

pub mod ask;
mod conduct;
pub mod disruptions;
mod edge;
pub mod invariants;
pub mod known;
pub mod ledger;
pub mod look;
pub mod mask;
pub mod options;
mod play;
pub mod processes;
pub mod record;
pub mod report;
mod setup;
pub mod staff;
mod stage;
mod stalls;
pub mod timeline;
mod tracker;
mod watcher;

pub use conduct::command;
