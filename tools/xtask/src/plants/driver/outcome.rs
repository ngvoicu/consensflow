//! Trial outcomes and the evidence kept when no run caught the plant.

use super::{Ran, Verdict};

/// How a plant ended, and what the last run said.
pub(super) struct Trial {
    pub(super) verdict: Verdict,
    pub(super) ran: Option<Ran>,
    /// The tests that failed, or what else caught it.
    pub(super) caught: Vec<String>,
    /// Why it did not apply, or why its run told nothing of it.
    pub(super) said: String,
    /// What else is to be said of it: that no run ran a test of the name it was meant
    /// for, or that the name stands for more than one test, what broke beside a catch.
    pub(super) notes: Vec<String>,
    /// The other runs that told nothing of it, each as it ended.
    pub(super) also: Vec<Trial>,
}

impl Trial {
    pub(super) fn of(verdict: Verdict, ran: Ran, caught: Vec<String>) -> Self {
        Self {
            verdict,
            ran: Some(ran),
            caught,
            said: String::new(),
            notes: Vec::new(),
            also: Vec::new(),
        }
    }

    pub(super) fn unapplied(said: String) -> Self {
        Self {
            verdict: Verdict::DoesNotApply,
            ran: None,
            caught: Vec::new(),
            said,
            notes: Vec::new(),
            also: Vec::new(),
        }
    }

    pub(super) fn not_run(said: String) -> Self {
        Self {
            verdict: Verdict::NotRun,
            ..Self::unapplied(said)
        }
    }

    pub(super) fn unstable(said: String) -> Self {
        Self {
            verdict: Verdict::Unstable,
            ..Self::unapplied(said)
        }
    }
}

/// The evidence used to decide an uncaught plant once all its runs have been tried.
pub(super) struct Uncaught {
    pub(super) last: Option<Ran>,
    pub(super) others: Option<(Ran, Vec<String>)>,
    pub(super) doubts: Vec<String>,
    pub(super) tests_ran: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::super::{finish_uncaught, Trial, Verdict};
    use super::*;
    use crate::plants::kit::Plant;

    #[test]
    fn an_uncaught_plant_keeps_its_first_inconclusive_run_and_missing_test_note() {
        let plant = Plant {
            table: "table".into(),
            name: "plant".into(),
            meant: "wanted_test".into(),
            runs: Vec::new(),
            builds: false,
            prepare: Vec::new(),
            requires: Vec::new(),
            platforms: Vec::new(),
            harnesses: Vec::new(),
            edits: Vec::new(),
        };
        let told = vec![
            Trial::not_run("first".into()),
            Trial::unstable("second".into()),
        ];
        let uncaught = Uncaught {
            last: None,
            others: None,
            doubts: Vec::new(),
            tests_ran: Some(vec!["another_test".into()]),
        };
        let trial = finish_uncaught(&plant, told, uncaught);
        assert_eq!(trial.verdict, Verdict::NotRun);
        assert_eq!(trial.said, "first");
        assert_eq!(trial.also.len(), 1);
        assert_eq!(trial.also[0].verdict, Verdict::Unstable);
        assert_eq!(trial.notes, ["no test named wanted_test ran in its runs"]);
    }
}
