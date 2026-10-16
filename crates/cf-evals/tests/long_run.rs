//! The long run's watch, held to ledgers and records made for each invariant, with
//! no harness running: for each, a case that breaks it and a case that holds, the
//! measure's way (a ledger file made from the ledger's own migrations, its rows
//! written by hand or recorded). The field's shapes are recorded ledgers
//! (`tests/fixtures/ledgers/field-*.sql`): A2's task done while its job ran, A3's
//! notice after its result, B5's result delivered twice and glued to the next. The
//! plants of `tests/plants/daemon/long-run.toml` run this suite.

#![allow(clippy::unwrap_used)]

mod support;

// The cases are in `tests/long_run/`: cargo looks for the modules of a test's root
// file beside it, where each would be a suite of its own.
#[path = "long_run/deliveries.rs"]
mod deliveries;
#[path = "long_run/end.rs"]
mod end;
#[path = "long_run/field.rs"]
mod field;
#[path = "long_run/keeper.rs"]
mod keeper;
#[path = "long_run/order.rs"]
mod order;
#[path = "long_run/scene.rs"]
mod scene;
#[path = "long_run/signals.rs"]
mod signals;
#[path = "long_run/turns.rs"]
mod turns;
#[path = "long_run/windows.rs"]
mod windows;
#[path = "long_run/work.rs"]
mod work;
