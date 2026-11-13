//! Real harnesses as the chief of a project that has specs: the proof that a chief
//! is told of Specmint where its project has specs on and told nothing where it has
//! them off, that it plans big work as a spec in the folder the project names, and
//! carries the spec to the board, which the stand-in agents of the rig cannot give. It
//! is opt-in (`cargo xtask live specs`, `npm run live:specs`): it spends the
//! machine's quota on a chief of each of five harnesses (Claude Code, Pi, Devin,
//! Codex and Muse Code, each on its cheap model) and needs each one installed and
//! logged in, so it is ignored unless asked for and is no part of `check` or of CI.
//!
//! The native daemon and the real pane host run in a home of the run's own (never
//! `~/.consensflow`), one rig for each project, one project after another. The chief's
//! window runs the machine's harness and reads its login where it is (the machine's
//! home, and `CODEX_HOME`; Claude Code on a config folder of the run's own with a copy
//! of the machine's login ([`cf_e2e::live::login`]), started through a shim that adds
//! `--strict-mcp-config --no-chrome`), and knows nothing of the shell that runs this:
//! the rig gives it an environment of its own. The project's one
//! member is the stand-in worker, which does no work and answers what a brief tells it
//! to ([`project::BRIEF`]): what the run judges is the chief's.
//!
//! The human is the page's calls (`project.specs`, and with `--apply launch` `project.close`
//! and `project.resume`, which is how the setting then takes effect) and the keys typed into the chief's window,
//! which the pane host takes as it takes the app's ([`cf_e2e::live::human`]); the chief
//! is a real harness that runs `cf` itself. The variant of Specmint is the first of
//! them the harness has not got installed for its user (`--variant` names another), so
//! that ConsensFlow's own copy is the one the chief is given, and where the harness has
//! the variant, its own is, and the run says which. Each project runs the steps of
//! [`plan::Step`], in this order:
//!
//! 1. `off`: with specs off, the launch of the chief says nothing of them; a small ask
//!    is a task on the board and no specs folder appears;
//! 2. `source`: the human turns specs on while the chief's window is open (`--apply now`,
//!    the default; T-1182): the daemon logged it, and the chief is told at its rest, in a
//!    note delivered to its own window with no new one, that has the section of specs naming
//!    the folder and the skill, which is the harness's own copy or ConsensFlow's
//!    ([`launched`]). With `--apply launch` the setting takes effect at the chief's next
//!    launch (the project is closed and resumed), the window is launched with that section,
//!    and a Codex chief's own record, its thread's rollout, holds it after the Resume (the
//!    thread keeps the instructions it began with, so it is a message of ConsensFlow's that
//!    tells it);
//! 3. `small`: a small ask still goes straight on the board, no spec is made, and the
//!    worker's window is launched with nothing of specs;
//! 4. `spec`: a tiny ask in two phases, which the human has the chief plan as a spec,
//!    answering its interview; the spec is in the folder the project names, in the shape
//!    of its variant, with a row in the registry ([`evidence`], [`judge`]);
//! 5. `board`: the human approves the spec; the chief puts its tasks on the board, each
//!    of a phase after the first needing the phase before it, the workers answer, the chief
//!    accepts, and the spec and its registry show what was accepted.
//!
//! What it checks is read from where the product wrote it (the files of the project's
//! folder, the board's tasks and what each needs, the daemon's events, the windows'
//! launches), never from what an agent says; each verdict is a function of text, held
//! to fixtures of what Specmint writes. Every wait says what it waits for and has a
//! deadline; a step that cannot go on says what the product showed.
//!
//! `cargo xtask live specs [harness ...]` limits the run to the projects the harnesses
//! named are the chief of; `--steps spec,board` to some steps (`board` brings its
//! `spec`); `--variant tdd` names the variant, `--folder docs/specs` the specs folder;
//! `--keep` leaves the homes where they are.

// The parts are in `live_specs/`: cargo looks for the modules of a test's root file
// beside it, where each would be a suite of its own.
#[path = "live_specs/ctx.rs"]
mod ctx;
#[path = "live_specs/evidence.rs"]
mod evidence;
#[path = "live_specs/judge.rs"]
mod judge;
#[path = "live_specs/launched.rs"]
mod launched;
#[path = "live_specs/plan.rs"]
mod plan;
#[path = "live_specs/planning.rs"]
mod planning;
#[path = "live_specs/project.rs"]
mod project;
#[path = "live_specs/run.rs"]
mod run;
#[path = "live_specs/steps.rs"]
mod steps;

use std::path::{Path, PathBuf};

use cf_e2e::live::{self, report, Harness, Log};
use cf_e2e::{Error, Result};

use run::{Machine, Options};

/// The stand-in for the worker's window, built for these tests: it is the program the
/// rig puts first on the `PATH` as `claude` (or the shim hands the windows of the
/// stand-in agents to, where the chief is Claude Code).
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The specs folder of a run that is not given one: below the project's folder, a
/// path of two parts, so that the chief has to read where Specmint's own `.specs` is
/// meant to be somewhere else.
const FOLDER: &str = "docs/specs";

/// Where the run keeps what it makes outside its rigs' homes: the shim that starts Claude Code,
/// in a new folder of the run's own in the build's temp folder (a folder of a run before, at the
/// same place, is never written into), taken away with the run.
fn kept() -> Result<cf_e2e::door::Made> {
    kept_in(Path::new(env!("CARGO_TARGET_TMPDIR")))
}

/// [`kept`] in the folder `parent`, which every run makes its own in.
fn kept_in(parent: &Path) -> Result<cf_e2e::door::Made> {
    cf_e2e::door::made_in(parent, "live-specs-").map_err(|source| Error::File {
        action: "make a folder in",
        path: parent.to_path_buf(),
        source,
    })
}

/// What the run is limited to and asked for: the words of the variables the command
/// sets, or none.
fn limits() -> Result<(Vec<Harness>, Vec<plan::Step>, Options)> {
    let named = |name: &str| live::requested(name).unwrap_or_default();
    let chiefs = plan::harnesses_named(&named(live::HARNESS)).map_err(Error::Daemon)?;
    let steps = plan::steps_named(&named(live::STEPS)).map_err(Error::Daemon)?;
    let variant = plan::variant_named(&named(&live::variable("variant"))).map_err(Error::Daemon)?;
    let folder = live::option("folder").unwrap_or_else(|| FOLDER.to_owned());
    let apply = project::Apply::named(live::option("apply").as_deref()).map_err(Error::Daemon)?;
    Ok((
        chiefs,
        steps,
        Options {
            variant,
            folder,
            apply,
        },
    ))
}

/// The harnesses the plan needs, found on the machine as a person's shell finds them,
/// each with its version said; one that is not there, or not logged in, is the error,
/// before anything is spent.
fn machine(plan: &[plan::Planned], log: &Log) -> Result<Machine> {
    let mut installed = Vec::new();
    for chief in plan.iter().map(|planned| planned.chief) {
        if !installed
            .iter()
            .any(|found: &live::Installed| found.harness() == chief)
        {
            installed.push(log.found(chief)?);
        }
    }
    Ok(Machine {
        installed,
        stand_in: PathBuf::from(FAKE_AGENT),
        shims: kept()?,
    })
}

/// How long this run is planned to take, a chief of each harness: a board of ten minutes and
/// a plan carried out in twenty-five (`planning`), in each.
const PLANNED: std::time::Duration = std::time::Duration::from_secs(60 * 60);

#[test]
#[ignore = "opt-in: spends the machine's quota on a chief of each harness; cargo xtask live specs (npm run live:specs)"]
fn a_chief_of_every_harness_is_told_of_specs_where_they_are_on_plans_one_and_carries_it_to_the_board(
) -> Result {
    let log = Log::begin("live specs")?.lasting(PLANNED);
    let (chiefs, steps, options) = limits()?;
    // A harness the run was limited to, or named a model for, is asked for: a machine
    // that lacks it is the error. Any other it lacks is left out, and the report says so.
    let mut named = chiefs.clone();
    named.extend_from_slice(live::agents::asked()?.named());
    let (seated, left_out) = live::seated(&named, live::lacks).map_err(Error::Daemon)?;
    log.leave_out(left_out);
    let plan = plan::projects(&chiefs, &steps, &seated);
    if plan.is_empty() || plan.iter().all(|project| project.steps.is_empty()) {
        return Err(Error::Daemon(
            "the harnesses and the steps asked for leave nothing to run".to_owned(),
        ));
    }
    let machine = machine(&plan, &log)?;
    log.say(format!(
        "{} project{} to run, one after another: {}; the specs folder is {}",
        plan.len(),
        if plan.len() == 1 { "" } else { "s" },
        plan.iter()
            .map(plan::Planned::name)
            .collect::<Vec<_>>()
            .join(", "),
        options.folder
    ));
    let mut checks = Vec::new();
    for planned in &plan {
        checks.extend(run::project(planned, &machine, &options, &log));
    }
    report::conclude(&log, "what each check came to", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cf_e2e::live::layout::Layout;

    #[test]
    fn two_runs_each_make_the_folder_of_their_shim_new_and_an_earlier_ones_is_as_it_was() {
        // The build's temp folder holds an earlier run's folder of the same kind, which is no
        // run of this one's to write into.
        let mut layout = Layout::new().unwrap();
        let parent = layout.home().join("target-tmp");
        layout
            .plant_tree(
                &parent.join("live-specs-Earlier"),
                &[("claude-isolated", "an earlier run's wrapper\n")],
            )
            .unwrap();
        let first = kept_in(&parent).unwrap();
        let second = kept_in(&parent).unwrap();
        assert_ne!(first.path(), second.path());
        for kept in [&first, &second] {
            assert!(kept.path().starts_with(&parent), "{:?}", kept.path());
            // The shim that starts Claude Code is written there through the door, for the run's own windows.
            let program = live::isolated_claude(kept.path(), Path::new("/the/claude")).unwrap();
            assert!(program.starts_with(kept.path()), "{program:?}");
            assert!(program.exists(), "{program:?}");
        }
        layout.untouched().unwrap();
        // Taken away with the run, the earlier run's left.
        let (a, b) = (first.path().to_path_buf(), second.path().to_path_buf());
        drop((first, second));
        assert!(!a.exists() && !b.exists());
        layout.untouched().unwrap();
    }
}
