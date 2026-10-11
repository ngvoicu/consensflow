//! Every report in `evals/reports/`, one line each on the console and one row each in
//! `evals/RESULTS.md` (`evals/summarize.mjs`, which the landing that ports the evals
//! replaces here): scenario, chief, staff, how many expectations held, the numbers
//! behind them. Reports are kept: a run is evidence, and a later run beside it is the
//! comparison. `cargo xtask eval summary` runs it.
