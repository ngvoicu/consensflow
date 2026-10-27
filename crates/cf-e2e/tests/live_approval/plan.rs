//! What the run is to do: which projects, with which harness in which seat,
//! and which steps of each. The rotation puts every harness in every seat once
//! across the run, so that no harness is only ever the one being tested or the
//! one doing the testing.

use cf_e2e::live::Harness;

/// The seats of a project's staff, besides the human's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seats {
    pub chief: Harness,
    pub worker: Harness,
    pub advisor: Harness,
    pub reviewer: Harness,
}

impl Seats {
    /// The harnesses in the project, the chief's first.
    pub fn harnesses(&self) -> [Harness; 4] {
        [self.chief, self.worker, self.advisor, self.reviewer]
    }
}

/// A step of a project's run: what the human does with the gate, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    /// A task for a worker: the brief and the result each wait for approval.
    Worker,
    /// A task for an advisor, paused and resumed while it works.
    Advisor,
    /// A task for a reviewer, whose brief the human declines.
    Reviewer,
    /// A question the worker asks the chief, and the answer.
    Question,
    /// A task for the image designer.
    Designer,
}

impl Step {
    /// Every step, in the order a project runs them.
    pub const ALL: [Step; 5] = [
        Step::Worker,
        Step::Advisor,
        Step::Reviewer,
        Step::Question,
        Step::Designer,
    ];

    /// What it is called in the report and in `CONSENSFLOW_LIVE_STEPS`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Advisor => "advisor",
            Self::Reviewer => "reviewer",
            Self::Question => "question",
            Self::Designer => "designer",
        }
    }
}

/// One project of the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    /// Its place in the rotation, from 1.
    pub number: usize,
    pub seats: Seats,
    /// Whether the image designer is on its staff, and given a task.
    pub designer: bool,
    /// The steps it runs.
    pub steps: Vec<Step>,
}

impl Planned {
    /// What it is called in the report: its number and its chief.
    pub fn name(&self) -> String {
        format!("p{} {}", self.number, self.seats.chief.name())
    }

    /// The harnesses the project needs on the machine: its staff's, and Codex
    /// where the image designer is.
    pub fn needs(&self) -> Vec<Harness> {
        let mut needs = self.seats.harnesses().to_vec();
        if self.designer && !needs.contains(&Harness::Codex) {
            needs.push(Harness::Codex);
        }
        needs
    }
}

/// The seats of the project of the rotation `at` (from 0): the chief is the
/// `at`th harness, and the worker, the advisor and the reviewer the next ones
/// after it, round the list.
fn seats(at: usize) -> Seats {
    let harness = |offset: usize| Harness::ALL[(at + offset) % Harness::ALL.len()];
    Seats {
        chief: harness(0),
        worker: harness(1),
        advisor: harness(2),
        reviewer: harness(3),
    }
}

/// The projects of a run: one for each harness the run is limited to as chief,
/// or one for each harness when it is limited to none, each with the steps
/// asked for (all of them when none are).
///
/// The image designer is Codex alone, and has the run of one project only: the
/// first whose chief is not Codex, and in which Codex holds no other seat when
/// there is such a project, so that its window is the only Codex there.
pub fn projects(chiefs: &[Harness], steps: &[Step]) -> Vec<Planned> {
    let wanted = |step: Step| steps.is_empty() || steps.contains(&step);
    let mut projects: Vec<Planned> = (0..Harness::ALL.len())
        .map(seats)
        .enumerate()
        .filter(|(_, seats)| chiefs.is_empty() || chiefs.contains(&seats.chief))
        .map(|(at, seats)| Planned {
            number: at + 1,
            seats,
            designer: false,
            steps: Step::ALL
                .into_iter()
                .filter(|step| *step != Step::Designer && wanted(*step))
                .collect(),
        })
        .collect();
    let without_codex = |project: &Planned| !project.seats.harnesses().contains(&Harness::Codex);
    let lodging = projects.iter().position(without_codex).or_else(|| {
        projects
            .iter()
            .position(|project| project.seats.chief != Harness::Codex)
    });
    if let (Some(at), true) = (lodging, wanted(Step::Designer)) {
        projects[at].designer = true;
        projects[at].steps.push(Step::Designer);
    }
    projects
}

/// The harnesses a list on a command line or in the environment names, split
/// at commas or blanks; the words that are no harness are the error.
pub fn harnesses_named(list: &str) -> Result<Vec<Harness>, String> {
    words(list)
        .map(|word| Harness::from_name(word).ok_or_else(|| format!("no harness is called {word}")))
        .collect()
}

/// The steps a list names, as [`harnesses_named`] reads it.
pub fn steps_named(list: &str) -> Result<Vec<Step>, String> {
    words(list)
        .map(|word| {
            Step::ALL
                .into_iter()
                .find(|step| step.name() == word)
                .ok_or_else(|| format!("no step is called {word}"))
        })
        .collect()
}

fn words(list: &str) -> impl Iterator<Item = &str> {
    list.split(|at: char| at == ',' || at.is_whitespace())
        .filter(|word| !word.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_harness_is_the_chief_of_one_project_and_holds_every_other_seat_once() {
        let all = projects(&[], &[]);
        assert_eq!(all.len(), Harness::ALL.len());
        for harness in Harness::ALL {
            let seat = |of: fn(&Seats) -> Harness| {
                all.iter()
                    .filter(|project| of(&project.seats) == harness)
                    .count()
            };
            assert_eq!(seat(|seats| seats.chief), 1, "{harness:?}");
            assert_eq!(seat(|seats| seats.worker), 1, "{harness:?}");
            assert_eq!(seat(|seats| seats.advisor), 1, "{harness:?}");
            assert_eq!(seat(|seats| seats.reviewer), 1, "{harness:?}");
        }
        // No project seats a harness twice.
        for project in &all {
            let distinct: std::collections::HashSet<_> =
                project.seats.harnesses().into_iter().collect();
            assert_eq!(distinct.len(), 4, "{}", project.name());
        }
    }

    #[test]
    fn the_first_project_is_a_claude_chief_with_an_opencode_worker() {
        let first = &projects(&[], &[])[0];
        assert_eq!(first.number, 1);
        assert_eq!(first.seats.chief, Harness::Claude);
        assert_eq!(first.seats.worker, Harness::OpenCode);
        assert_eq!(first.seats.advisor, Harness::Pi);
        assert_eq!(first.seats.reviewer, Harness::Devin);
        assert_eq!(first.name(), "p1 claude");
    }

    #[test]
    fn a_harness_named_limits_the_run_to_the_projects_it_is_the_chief_of() {
        let only = projects(&[Harness::Pi], &[]);
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].seats.chief, Harness::Pi);
        assert_eq!(only[0].number, 3);
        let two = projects(&[Harness::Devin, Harness::Claude], &[]);
        let chiefs: Vec<_> = two.iter().map(|project| project.seats.chief).collect();
        assert_eq!(chiefs, [Harness::Claude, Harness::Devin]);
    }

    #[test]
    fn the_designer_has_the_one_project_whose_chief_is_not_codex_and_that_has_no_codex() {
        let all = projects(&[], &[]);
        let with: Vec<_> = all.iter().filter(|project| project.designer).collect();
        assert_eq!(with.len(), 1);
        // The claude project, whose staff is opencode, pi and devin.
        assert_eq!(with[0].seats.chief, Harness::Claude);
        assert_eq!(with[0].steps.last(), Some(&Step::Designer));
        assert_eq!(with[0].needs().last(), Some(&Harness::Codex));
        // The others have it neither as a step nor as a harness they need.
        for project in all.iter().filter(|project| !project.designer) {
            assert!(!project.steps.contains(&Step::Designer));
            assert_eq!(project.needs().len(), 4);
        }
    }

    #[test]
    fn the_designer_goes_where_codex_is_a_seat_when_no_chosen_project_is_without_one() {
        // The opencode project's staff is pi, devin and codex.
        let one = projects(&[Harness::OpenCode], &[]);
        assert!(one[0].designer);
        assert_eq!(one[0].needs().len(), 4);
        // Codex is the only chief: no project's chief is not Codex, so no designer.
        let codex = projects(&[Harness::Codex], &[]);
        assert!(!codex[0].designer);
        assert!(!codex[0].steps.contains(&Step::Designer));
    }

    #[test]
    fn the_steps_asked_for_are_the_only_steps_run() {
        let only = projects(&[Harness::Claude], &[Step::Reviewer, Step::Worker]);
        assert_eq!(only[0].steps, [Step::Worker, Step::Reviewer]);
        assert!(!only[0].designer);
        let designer = projects(&[Harness::Claude], &[Step::Designer]);
        assert_eq!(designer[0].steps, [Step::Designer]);
        assert!(designer[0].designer);
    }

    #[test]
    fn a_list_names_harnesses_and_steps_by_commas_or_blanks_and_a_wrong_word_is_the_error() {
        assert_eq!(
            harnesses_named("claude, pi opencode,,"),
            Ok(vec![Harness::Claude, Harness::Pi, Harness::OpenCode])
        );
        assert_eq!(harnesses_named(""), Ok(Vec::new()));
        assert_eq!(
            harnesses_named("claude,gemini"),
            Err("no harness is called gemini".to_owned())
        );
        assert_eq!(
            steps_named("question designer"),
            Ok(vec![Step::Question, Step::Designer])
        );
        assert_eq!(
            steps_named("pause"),
            Err("no step is called pause".to_owned())
        );
    }
}
