//! What a chief did, read from the ledger of a finished eval run (`evals/measure.mjs`,
//! which the landing that ports the evals replaces here): the tasks it put on the
//! board and how many ran side by side, what it asked and told the human, the
//! advice and reviews it asked for, the edits it made with its own hands, its last
//! words, and which files of the fixture the run changed or added.
//!
//! The ledger file is read with SQLite and no ledger's code, whichever daemon wrote
//! it: the board is one of the things a run checks, so it cannot be what the tasks
//! are counted from. The daemon must have closed the ledger first: it holds its file
//! (a copy of the file and of its write-ahead log is what is read of a ledger in use:
//! `onCopy` of the JavaScript). The recordings in `tests/fixtures/ledgers` are the
//! ledgers the measure is held to, a `.json` of the numbers each must give, made by a
//! ledger file of [`crate::ledger_file`].
//!
//! Two things of the JavaScript are comments only, and are kept:
//!
//! - a file a harness keeps in the project for itself, which no agent wrote, is no
//!   changed file of the run: Claude Code's `.claude/scheduled_tasks.lock`, while a
//!   wakeup it scheduled waits (a Claude chief on Windows scheduled one to check on
//!   its worker, 2026-10-04);
//! - the chief's open question is its newest words that ended a turn with a question
//!   since the owner last typed into its window (ConsensFlow's own messages carry
//!   their header), and it stays open through later turns that ask nothing: a Codex
//!   chief asked the owner one, then waited for the answer to write its note (Windows,
//!   2026-10-03).
