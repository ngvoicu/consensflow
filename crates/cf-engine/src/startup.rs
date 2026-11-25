//! Why a window did not start, as an automatic note says it (T-1328).
//!
//! A note about a launch that failed goes to the chief, a member, the board, a result or the
//! ledger. What the adapter, the host or a program said of the failure is free text, and a path
//! in it can name a credential (`C:\tools\pi hunter22.cmd`): quoting what looks safe failed
//! twice (the joint review T-1322 found five blockers, then two left). So a note of this kind
//! is built from a [`Startup`], which holds a stage that is fixed here, and the kind of failure
//! and its code where the producer typed them (`cf_base::startup::StartFailure`). It has no
//! `String`, and no constructor takes one: the words of a program, a host or an adapter cannot
//! be built into it.
//!
//! The full words stay where the person reads them on their own screen: the harness's terminal,
//! the page's fields and ConsensFlow's local log (masked, as every quote is).

use cf_base::startup::StartFailure;

/// How far a launch got before it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// The adapter could not prepare the launch.
    Prepare,
    /// The host did not open the window.
    Open,
    /// The window opened and never came up.
    Up,
    /// The window came up and could not take its first message.
    FirstMessage,
}

/// What is known of why, besides the stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Why {
    /// A program could not be started or read, of this kind.
    Program(StartFailure),
    /// The host would not open the window, in words of its own that no note carries.
    HostRefused,
    /// The host did not answer in time.
    HostSilent,
    /// ConsensFlow has no adapter for the harness: its own fixed words, with the harness's name.
    NoAdapter,
    /// Words of an adapter or of a harness that no note carries.
    Unsaid,
}

/// A launch that failed to start: the stage and the reason, both typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Startup {
    stage: Stage,
    why: Why,
}

impl Startup {
    pub(crate) const fn new(stage: Stage, why: Why) -> Self {
        Self { stage, why }
    }

    /// The words of a note for the participant's `who` (a harness as the pages name it).
    pub(crate) fn words(&self, who: &str) -> String {
        let stage = match self.stage {
            Stage::Prepare => "the launch failed",
            Stage::Open => "the window did not open",
            Stage::Up => "the window never came up",
            Stage::FirstMessage => "the window could not take its first message",
        };
        match self.why {
            Why::Program(failure) => format!("{stage}: {}", failure.words(who)),
            Why::HostRefused => format!("{stage}: the host would not open it"),
            Why::HostSilent => format!("{stage}: the host did not answer in time"),
            Why::NoAdapter => format!("{stage}: ConsensFlow cannot open {who} windows"),
            Why::Unsaid => format!("{stage} (its cause is in ConsensFlow's log)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use cf_base::startup::{StartFailure, StartKind};

    use super::*;

    #[test]
    fn a_startup_is_said_by_its_stage_and_its_kind_and_nothing_else() {
        let missing = StartFailure::of(StartKind::NotFound).with_errno("ENOENT");
        assert_eq!(
            Startup::new(Stage::Open, Why::Program(missing)).words("Pi"),
            "the window did not open: Pi's program was not found (ENOENT); check that Pi is installed and that ConsensFlow can find it"
        );
        assert_eq!(
            Startup::new(Stage::Prepare, Why::Unsaid).words("Pi"),
            "the launch failed (its cause is in ConsensFlow's log)"
        );
        assert_eq!(
            Startup::new(Stage::Prepare, Why::NoAdapter).words("Pi"),
            "the launch failed: ConsensFlow cannot open Pi windows"
        );
        assert_eq!(
            Startup::new(Stage::Open, Why::HostRefused).words("Codex"),
            "the window did not open: the host would not open it"
        );
        assert_eq!(
            Startup::new(Stage::Up, Why::HostSilent).words("Codex"),
            "the window never came up: the host did not answer in time"
        );
        assert_eq!(
            Startup::new(Stage::FirstMessage, Why::Unsaid).words("Codex"),
            "the window could not take its first message (its cause is in ConsensFlow's log)"
        );
    }
}
