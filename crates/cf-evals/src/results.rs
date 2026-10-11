//! The results file (`evals/results.mjs`, which the landing that ports the evals
//! replaces here): one row per report, newest last, the same numbers the summary
//! prints. Pure, so it is tested without a report on disk.
//!
//! The reports kept in `evals/reports` are evidence, and an old one is still read:
//! a report from before the mechanics were counted has none, and one from when the
//! board took questions counted the terminal's apart and spelled it
//! `questionsToHuman`. `evals/RESULTS.md` written from the kept reports is
//! byte for byte what the JavaScript wrote.
