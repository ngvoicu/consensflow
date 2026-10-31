//! Who a live check runs as the member it is about: a harness of the machine, or the
//! rig's stand-in agent, which has no model and spends no quota. The checks that run
//! each harness one after the other (the receipt door, the finish of a worker's task)
//! take them by the words of their command line ([`Who::named`]), where `fake` is the
//! stand-in, and say the version of each harness before and after its run
//! ([`Who::version`], [`between`]): a harness may update itself while it runs.

use super::machine::Harness;
use super::version_of;
use crate::{Error, Result};

/// Who is run: a real harness, or the rig's stand-in agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    StandIn,
    Harness(Harness),
}

impl Who {
    /// The one a word on the command line names: `fake`, or a harness. What the check
    /// calls them (`noun`: `door`, `worker`) is in the refusal of any other word.
    pub fn named(word: &str, noun: &str) -> Result<Self> {
        if word == "fake" {
            return Ok(Self::StandIn);
        }
        Harness::from_name(word).map(Self::Harness).ok_or_else(|| {
            Error::Daemon(format!(
                "no {noun} is called {word}: fake, {}",
                Harness::ALL.map(Harness::name).join(", ")
            ))
        })
    }

    /// What it is called.
    pub const fn name(self) -> &'static str {
        match self {
            Self::StandIn => "fake",
            Self::Harness(harness) => harness.name(),
        }
    }

    /// The version of the harness it runs, as it says it: `stand-in` for the stand-in, and
    /// `unknown` with the reason for a harness the machine does not have set up.
    pub fn version(self) -> String {
        match self {
            Self::StandIn => "stand-in".to_owned(),
            Self::Harness(harness) => version_of(harness),
        }
    }
}

/// The version of a harness as a run names it, given what it was before the run and what
/// it was after: one where it did not change, and both where the harness updated itself.
pub fn between(before: String, after: String) -> String {
    if after == before {
        before
    } else {
        format!("{before}, then {after}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_names_a_harness_or_the_stand_in_and_any_other_is_refused_with_the_checks_noun() {
        assert_eq!(Who::named("fake", "door").unwrap(), Who::StandIn);
        assert_eq!(
            Who::named("opencode", "door").unwrap(),
            Who::Harness(Harness::OpenCode)
        );
        let failed = Who::named("kimi", "worker").unwrap_err().to_string();
        assert_eq!(
            failed,
            "no worker is called kimi: fake, claude, opencode, pi, devin, codex, muse"
        );
        assert_eq!(
            Who::named("kimi", "door").unwrap_err().to_string(),
            "no door is called kimi: fake, claude, opencode, pi, devin, codex, muse"
        );
    }

    #[test]
    fn each_is_called_by_the_word_that_names_it() {
        assert_eq!(Who::StandIn.name(), "fake");
        for harness in Harness::ALL {
            assert_eq!(Who::Harness(harness).name(), harness.name());
            assert_eq!(
                Who::named(harness.name(), "x").unwrap(),
                Who::Harness(harness)
            );
        }
    }

    #[test]
    fn the_stand_in_has_the_version_of_a_stand_in_and_a_harness_that_updated_has_both() {
        assert_eq!(Who::StandIn.version(), "stand-in");
        assert_eq!(
            between("2.1.296".to_owned(), "2.1.296".to_owned()),
            "2.1.296"
        );
        assert_eq!(
            between("2.1.296".to_owned(), "2.1.297".to_owned()),
            "2.1.296, then 2.1.297"
        );
    }
}
