//! A project, run: a rig of its own (a daemon and a pane host, in a home that goes
//! with it) whose chief's window is the machine's real harness on its own login,
//! and whose staff is the stand-in worker; the project opened with specs off, and
//! its steps in order. A step that cannot go on ends the project, since what it
//! left on the board and in the folder is not known any more; the next project
//! starts afresh.

use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::live::report::Check;
use cf_e2e::live::trace::Trace;
use cf_e2e::live::window::trust_claude;
use cf_e2e::live::{self, agents, ask, cleanup, cut, members, warnings, Harness, Installed, Log};
use cf_e2e::process::own_var;
use cf_e2e::rig::{Project, Rig};
use cf_e2e::{Error, Result};

use crate::ctx::Ctx;
use crate::plan::{self, Planned, Step, VARIANTS};
use crate::project::{self, Settings};
use crate::steps;

/// How long the daemon and the pane host have to end once the project is: a real
/// harness's window takes its time to leave.
const CLOSES: Duration = Duration::from_secs(60);

/// What the machine has for the run.
pub struct Machine {
    /// The harnesses installed, with their logins: the chiefs'.
    pub installed: Vec<Installed>,
    /// The stand-in the rig's windows run for the worker, and for every Claude
    /// window where the chief is not Claude Code.
    pub stand_in: PathBuf,
    /// Where the shim that starts Claude Code through the stand-in is made.
    pub shims: PathBuf,
}

/// What the run was asked for, besides the harnesses and the steps.
pub struct Options {
    /// The variant of Specmint, or none: the first the harness has not.
    pub variant: Option<String>,
    /// The specs folder the projects are given, relative to the project's folder.
    pub folder: String,
}

impl Machine {
    fn of(&self, harness: Harness) -> Option<&Installed> {
        self.installed
            .iter()
            .find(|installed| installed.harness() == harness)
    }
}

/// A check of the project that did not hold, for a reason that is no verdict on what
/// the chief did: the project could not be set up or a step could not go on.
fn failed(planned: &Planned, what: &str, why: impl Display) -> Check {
    Check::failed(&planned.name(), what, why.to_string())
}

/// What the harness has installed of each variant, by the harness's own adapter
/// (`harness-ask specmint`), and the settings the project is given from it: the
/// variant of the run, which is the one asked for or the first the harness lacks, and
/// where the harness has it.
fn settings(rig: &Rig, planned: &Planned, options: &Options) -> Result<Settings> {
    let mut have = Vec::new();
    for name in VARIANTS {
        have.push((
            name,
            ask::specmint(planned.chief.kind(), name, rig.environment())?,
        ));
    }
    let installed = |name: &str| {
        have.iter()
            .any(|(has, skill)| *has == name && skill.installed)
    };
    let variant =
        plan::chosen_variant(options.variant.as_deref(), installed).map_err(Error::Daemon)?;
    let installed_at = have
        .iter()
        .find(|(name, skill)| *name == variant.name && skill.installed)
        .map(|(_, skill)| skill.skill.clone());
    Ok(Settings {
        folder: options.folder.clone(),
        variant,
        installed_at,
    })
}

/// Opens the project in the rig: Claude's trust of the folder where the chief is
/// Claude Code, and the project with specs off. What the human sets for its specs
/// is a step's.
fn open<'a>(
    planned: &Planned,
    machine: &Machine,
    rig: &'a Rig,
    folder: &Path,
    log: &Log,
) -> Result<Project<'a>> {
    if let Some(claude) = machine
        .of(Harness::Claude)
        .filter(|_| planned.chief == Harness::Claude)
    {
        let argv: Vec<String> = std::iter::once(claude.command().to_string_lossy().into_owned())
            .chain(live::ISOLATION.map(str::to_owned))
            .collect();
        let trust = trust_claude(rig, &argv, folder)?;
        log.say(format!("Claude Code and the folder: {trust}"));
    }
    let project = project::open(rig, planned, folder)?;
    log.say(format!(
        "project {} is open in {} with specs off: the chief is {}, and the worker is the stand-in",
        project.id(),
        folder.display(),
        planned.chief.name()
    ));
    Ok(project)
}

/// What the product showed when a step could not go on, on the error output: the
/// tasks on the board, the chief's screen, the daemon's last events and warnings,
/// and what the project's folder holds of specs.
fn what_was_shown(ctx: &Ctx) {
    let log = ctx.log;
    let tasks = ctx.tasks().map_or_else(
        |failed| failed.to_string(),
        |tasks| {
            tasks
                .iter()
                .map(steps::shown)
                .collect::<Vec<_>>()
                .join("\n")
        },
    );
    log.block("the tasks on the board", &tasks);
    if let Ok(chief) = ctx.human.chief() {
        log.block(
            "the chief's screen",
            &ctx.human.screen(&chief["pane"], 14).join("\n"),
        );
    }
    log.block("the daemon's last events", &Trace::read(ctx.rig).tail(40));
    let daemon_log = ctx.rig.log();
    let warned = warnings(&daemon_log);
    log.block(
        "the daemon's log, its warnings and errors",
        &if warned.is_empty() {
            "(none)".to_owned()
        } else {
            warned.join("\n")
        },
    );
    let held = ctx
        .files(&ctx.settings.folder)
        .map(|seen| {
            let mut lines: Vec<String> =
                seen.specs.iter().map(|found| found.path.clone()).collect();
            lines.extend(seen.elsewhere);
            lines.push(format!(
                "registry: {}",
                seen.registry
                    .as_deref()
                    .map_or_else(|| "none".to_owned(), |text| cut(text, 300))
            ));
            lines.join("\n")
        })
        .unwrap_or_else(|failed| failed.to_string());
    log.block("what the project's folder holds of specs", &held);
}

/// Runs the steps of the project in `ctx`, each in its turn, and says what each
/// checked. A check that does not hold ends nothing: the steps after it check what
/// they check. A step that cannot go on ends them, with what the product showed.
fn steps_of(ctx: &Ctx, planned: &Planned, log: &Log) -> Vec<Check> {
    let mut checks = Vec::new();
    for step in &planned.steps {
        if *step == Step::Board && ctx.spec_path().is_none() {
            log.say("step board: not run, no spec was written for it to carry out");
            continue;
        }
        log.say(format!("step {}: begins", step.name()));
        let mut made = Vec::new();
        let outcome = steps::run(ctx, *step, &mut made);
        for check in &made {
            log.say(format!(
                "step {}: {} {}: {}",
                step.name(),
                if check.ok { "held:" } else { "did NOT hold:" },
                check.what,
                cut(&check.detail, 200)
            ));
        }
        let held = made.iter().all(|check| check.ok);
        checks.extend(made);
        if let Err(why) = outcome {
            log.say(format!("step {}: could not go on: {why}", step.name()));
            checks.push(failed(
                planned,
                &format!("{}: the step goes on", step.name()),
                why,
            ));
            what_was_shown(ctx);
            break;
        }
        // A step whose check did not hold says what the product showed, as one that
        // could not go on does: the chief that wrote no spec is the likely failure.
        if !held {
            what_was_shown(ctx);
        }
    }
    checks
}

/// Has the harnesses forget what they kept of a project's folder, now that its
/// windows are gone: Claude Code by its own `purge` (its entry in `~/.claude.json`,
/// which holds the trust the run asked of it), and the sessions Claude Code and Pi
/// wrote in their own folders.
fn forget(machine: &Machine, known: &[String], log: &Log) {
    let Some(home) = own_var("HOME").map(PathBuf::from) else {
        return;
    };
    if let Some(claude) = machine.of(Harness::Claude) {
        let purged = cleanup::forget_in_claude(claude.command(), &home, known);
        if !purged.is_empty() {
            log.say(format!(
                "Claude Code forgot the project at {}",
                purged.join(", ")
            ));
        }
    }
    let removed = cleanup::remove(&cleanup::records(&home, known));
    if !removed.is_empty() {
        log.say(format!(
            "removed what Claude Code and Pi kept of the folder: {}",
            removed
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
}

/// Runs the project `planned`: what each of its steps checked.
pub fn project(planned: &Planned, machine: &Machine, options: &Options, log: &Log) -> Vec<Check> {
    let log = log.scoped(planned.name());
    let Some(chief) = machine.of(planned.chief) else {
        return vec![failed(planned, "setup", "the machine has no such harness")];
    };
    log.say("starts its rig");
    let config = match members::rig_for(
        std::slice::from_ref(chief),
        &machine.shims,
        &machine.stand_in,
    ) {
        Ok(config) => config,
        Err(why) => return vec![failed(planned, "setup", why)],
    };
    let mut rig = match Rig::start(config) {
        Ok(rig) => rig,
        Err(why) => return vec![failed(planned, "setup", why)],
    };
    if live::keep_requested() {
        log.say(format!(
            "{} is set: the home and the project stay at {}",
            live::KEEP,
            rig.keep_home().display()
        ));
    }
    let mut checks = Vec::new();
    let mut known = Vec::new();
    let setup = agents::write_roster(&rig, &project::roster(planned))
        .and_then(|()| settings(&rig, planned, options))
        .and_then(|settings| project::folder(&rig, planned).map(|folder| (settings, folder)));
    match setup {
        Err(why) => {
            log.say(format!("setup FAILED: {why}"));
            checks.push(failed(planned, "setup", why));
        }
        Ok((settings, folder)) => {
            rig.forget_claude_memory_of(&folder);
            log.say(format!(
                "plans with {} in {}: {}",
                settings.variant.name,
                settings.folder,
                match &settings.installed_at {
                    Some(file) => format!("the harness has it installed, at {file}"),
                    None => "the harness has not got it installed, so ConsensFlow's own copy is the chief's".to_owned(),
                },
            ));
            if !live::keep_requested() {
                known = cleanup::paths(&folder);
            }
            match open(planned, machine, &rig, &folder, &log) {
                Err(why) => {
                    log.say(format!("setup FAILED: {why}"));
                    checks.push(failed(planned, "setup", why));
                }
                Ok(opened) => {
                    let ctx = Ctx::new(&rig, opened, planned, &folder, &settings, &log);
                    match ctx.human.waits_for_the_chief() {
                        Err(why) => {
                            log.say(format!("setup FAILED: {why}"));
                            checks.push(failed(planned, "setup", why));
                            what_was_shown(&ctx);
                        }
                        Ok(()) => {
                            log.say("the chief's window is ready");
                            checks.extend(steps_of(&ctx, planned, &log));
                        }
                    }
                }
            }
        }
    }
    if let Err(why) = rig.close_within(CLOSES) {
        log.say(format!("closing the rig: {why}"));
        checks.push(failed(planned, "close", why));
    }
    forget(machine, &known, &log);
    checks
}
