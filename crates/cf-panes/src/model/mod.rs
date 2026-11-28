//! A model of N17's checks: the real host and the real readers under a deterministic scheduler.
//!
//! What is real: the pane's screen and its parser, the two byte counts and the output clock, the
//! input arbiter (`write_typed`, `write_paste`, `enter`, with the pane's writer, the screen's lock
//! and the input state taken as the host takes them), the host's snapshot of the screen
//! (`pane_handlers::snapshot_of`), the activity reader's order, and, from `cf-harness`, the look,
//! the three readers of an input box and the licences. What stands in: the program in the window
//! (it draws the box of Claude Code, Devin or Muse Code, echoes the human's keys as it was seen to,
//! takes a paste and an Enter, and keeps the truth), the pty, and the engine (a pass takes a look,
//! asks the licence functions and acts, with its attempts and its 10 s, 60 s and 3 s clocks, and
//! nothing else of the dispatcher).
//!
//! Each actor is a thread and exactly one runs, from one named point to the next (`sched`); the
//! model's time moves only when an actor rests (`clock`). A schedule is a seed, and it replays
//! (`CF_MODEL_SEED`). After every step the oracle holds the rules:
//! 1. no paste's body reached the program while the human's draft was not empty;
//! 2. no Enter was written whose last check, with the writer held, saw output pending: before the
//!    cap and its grace for a paste's first Enter, ever for `pane.enter`;
//! 3. the program never holds two copies of a message, and never takes one twice;
//! 4. at the end, the window left to settle for five minutes of the model's time: every message was
//!    taken once, unless the human still holds unsent text, in which case it was neither pasted
//!    nor failed.
//!
//! What the checks assume of a window (the brain's "What the checks assume") holds in the model: a
//! key's echo is wholly read within a second of the key, and the program's box changes only by input.

pub(crate) mod clock;
mod program;
pub(crate) mod sched;
mod world;

#[cfg(test)]
mod cases;
#[cfg(test)]
mod walks;
