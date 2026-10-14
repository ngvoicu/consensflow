//! What the run is to do: one project for each harness asked for as its chief (all
//! five when none is), the steps of each, and the variant of Specmint it plans
//! with. A project's staff is the stand-in worker, which does no work and answers
//! what a brief tells it to: the run judges what the chief does with specs, and
//! the staff's work is nothing it looks at.

use cf_e2e::live::Harness;

/// The variants of Specmint, in the order a run takes the first one a harness lacks:
/// the plain markdown ones before the HTML ones, which a cheap model writes slowly.
/// (Held to the variants the repository vendors, `skill/specmint/manifest.json`.)
pub const VARIANTS: [&str; 4] = ["core", "tdd", "core-html", "tdd-html"];

/// A step of a project's run, in the order they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    /// Specs off: the chief is told nothing of them, a small ask goes on the board
    /// as a task, and no specs folder appears.
    Off,
    /// Specs on, taking effect at the chief's next launch: the window is launched
    /// with the section of specs and the skill of the variant, a copy of the
    /// harness's own where it has the variant, of ConsensFlow's where it has not.
    Source,
    /// Specs on: a small ask still goes straight on the board, and no spec is made.
    Small,
    /// Specs on: a tiny ask in two phases, which the human has the chief plan as a
    /// spec, through Specmint's interview; the spec is in the folder, with its
    /// registry.
    Spec,
    /// The chief puts the spec's tasks on the board, a phase's after the one before
    /// it, and marks the spec's progress as it accepts them.
    Board,
}

impl Step {
    /// Every step, in the order a project runs them.
    pub const ALL: [Step; 5] = [
        Step::Off,
        Step::Source,
        Step::Small,
        Step::Spec,
        Step::Board,
    ];

    /// What it is called in the report and in `CONSENSFLOW_LIVE_STEPS`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Source => "source",
            Self::Small => "small",
            Self::Spec => "spec",
            Self::Board => "board",
        }
    }

    /// The step this one goes on from: the board has the spec to carry out.
    const fn needs(self) -> Option<Step> {
        match self {
            Self::Board => Some(Step::Spec),
            _ => None,
        }
    }
}

/// One project of the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    /// Its place in the rotation of the harnesses, from 1.
    pub number: usize,
    /// The harness of its chief.
    pub chief: Harness,
    /// The steps it runs, in order.
    pub steps: Vec<Step>,
}

impl Planned {
    /// What it is called in the report: its number and its chief.
    pub fn name(&self) -> String {
        format!("p{} {}", self.number, self.chief.name())
    }

    /// Whether it runs `step`.
    pub fn runs(&self, step: Step) -> bool {
        self.steps.contains(&step)
    }
}

/// The projects of a run: one for each harness the run is limited to as chief, or
/// one for each harness when it is limited to none, each with the steps asked for
/// (all of them when none are) and, for a step that goes on from another, the other
/// first.
pub fn projects(chiefs: &[Harness], steps: &[Step]) -> Vec<Planned> {
    let wanted: Vec<Step> = Step::ALL
        .into_iter()
        .filter(|step| {
            steps.is_empty()
                || steps
                    .iter()
                    .any(|asked| asked == step || asked.needs() == Some(*step))
        })
        .collect();
    Harness::ALL
        .into_iter()
        .enumerate()
        .filter(|(_, chief)| chiefs.is_empty() || chiefs.contains(chief))
        .map(|(at, chief)| Planned {
            number: at + 1,
            chief,
            steps: wanted.clone(),
        })
        .collect()
}

/// The variant a project plans with, and whether the harness has it installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chosen {
    pub name: &'static str,
    /// Whether the harness has this variant installed for its user, so that its own
    /// copy is the one the chief is given, and ConsensFlow's is not.
    pub installed: bool,
}

/// The variant a harness's project plans with: the one asked for (`--variant`),
/// which the harness may have installed or not; and else the first of [`VARIANTS`]
/// it has not (`has` says whether it has one), so that ConsensFlow's own copy is
/// the one the chief is given. A harness that has all four is given the first.
pub fn chosen_variant(asked: Option<&str>, has: impl Fn(&str) -> bool) -> Result<Chosen, String> {
    let chosen = |name: &'static str| Chosen {
        name,
        installed: has(name),
    };
    if let Some(asked) = asked {
        return VARIANTS
            .into_iter()
            .find(|name| *name == asked)
            .map(chosen)
            .ok_or_else(|| {
                format!(
                    "no variant is called {asked}: Specmint's are {}",
                    VARIANTS.join(", ")
                )
            });
    }
    Ok(chosen(
        VARIANTS
            .into_iter()
            .find(|name| !has(name))
            .unwrap_or(VARIANTS[0]),
    ))
}

/// The harnesses a list on a command line or in the environment names, split at
/// commas or blanks; the words that are no harness are the error.
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

/// The one variant a list names, if it names any; more than one is the error.
pub fn variant_named(list: &str) -> Result<Option<String>, String> {
    let mut named = words(list);
    let first = named.next();
    match named.next() {
        None => Ok(first.map(str::to_owned)),
        Some(_) => Err("a run plans with one variant: name one with --variant".to_owned()),
    }
}

fn words(list: &str) -> impl Iterator<Item = &str> {
    list.split(|at: char| at == ',' || at.is_whitespace())
        .filter(|word| !word.is_empty())
}

#[cfg(test)]
#[path = "plan/tests.rs"]
mod tests;
