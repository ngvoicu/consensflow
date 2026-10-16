//! A project, run: a rig of its own (a daemon and a pane host, in a home that
//! goes with it) whose windows are the machine's real harnesses on their own
//! logins, the project opened with its staff and the gate on, and its steps in
//! order. A step that fails ends the project, since what it left on the board is
//! not known any more; the next project starts afresh.

use std::path::PathBuf;
use std::time::Instant;

use cf_e2e::live::{self, cleanup, cut, warnings, Harness, Installed, Log};
use cf_e2e::process::own_var;
use cf_e2e::rig::Rig;
use cf_e2e::Result;

use crate::agents;
use crate::flow::Ctx;
use crate::plan::{Planned, Step};
use crate::project;
use crate::report::{self, Outcome, Row};
use crate::steps;

/// What the machine has for the run, and what the rigs start the windows with.
pub struct Machine {
    /// The harnesses installed, with their logins.
    pub installed: Vec<Installed>,
    /// The program that runs Claude Code with the flags that keep it from the
    /// human's MCP servers and browser, where the run has Claude Code.
    pub claude: Option<PathBuf>,
    /// The stand-in the rig puts first on the `PATH` where no Claude Code is
    /// in the project.
    pub stand_in: PathBuf,
}

impl Machine {
    fn of(&self, harness: Harness) -> Option<&Installed> {
        self.installed
            .iter()
            .find(|installed| installed.harness() == harness)
    }
}

/// What a project came to.
pub struct Done {
    pub rows: Vec<Row>,
    pub findings: Vec<String>,
}

/// The seconds since `started`.
fn since(started: Instant) -> u64 {
    started.elapsed().as_secs()
}

/// The rig of the project: its windows run the harnesses of the plan on the
/// machine's logins ([`live::rig`]), and Claude Code through the wrapper, where
/// the project has it, in place of the stand-in.
fn rig_for(planned: &Planned, machine: &Machine) -> cf_e2e::rig::Config {
    let needs = planned.needs();
    let installs: Vec<Installed> = needs
        .iter()
        .filter_map(|harness| machine.of(*harness).cloned())
        .collect();
    let stand_in = match (&machine.claude, needs.contains(&Harness::Claude)) {
        (Some(claude), true) => claude.clone(),
        _ => machine.stand_in.clone(),
    };
    live::rig(&installs, stand_in)
}

/// Starts the rig of the project, and opens it: the staff's agents in the
/// roster, the folder, Claude's trust of it, the project with the gate on.
fn open<'a>(
    planned: &Planned,
    machine: &Machine,
    rig: &'a Rig,
    log: &Log,
) -> Result<(PathBuf, cf_e2e::rig::Project<'a>)> {
    agents::write_roster(rig, &planned.needs(), log)?;
    let folder = project::folder(rig, planned)?;
    if let Some(claude) = machine
        .of(Harness::Claude)
        .filter(|_| planned.needs().contains(&Harness::Claude))
    {
        // Through the wrapper the project's windows start through: this window,
        // unattended on the human's own setup, reaches none of its MCP servers either.
        let program = machine.claude.as_deref().unwrap_or(claude.command());
        let argv = [program.to_string_lossy().into_owned()];
        let trust = live::window::trust_claude(rig, &argv, &folder)?;
        log.say(format!("Claude Code and the folder: {trust}"));
    }
    let project = project::open(rig, planned, &folder)?;
    log.say(format!(
        "project {} is open in {} with the gate on: {} chief; {} worker, {} advisor, {} reviewer{}",
        project.id(),
        folder.display(),
        planned.seats.chief.name(),
        planned.seats.worker.name(),
        planned.seats.advisor.name(),
        planned.seats.reviewer.name(),
        if planned.designer {
            ", the image designer"
        } else {
            ""
        }
    ));
    Ok((folder, project))
}

/// Runs the steps of the project in `ctx`, each timed; the first that fails
/// ends them, with what the product showed.
fn steps_of(ctx: &Ctx, planned: &Planned, log: &Log) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut failed = false;
    for step in &planned.steps {
        let row = |outcome, seconds| Row {
            project: planned.name(),
            step: step.name().to_owned(),
            outcome,
            seconds,
        };
        if failed {
            rows.push(row(
                Outcome::Skipped(
                    "an earlier step failed: what it left on the board is not known".to_owned(),
                ),
                0,
            ));
            continue;
        }
        if *step == Step::Question && agents::question_tool(planned.seats.worker).is_none() {
            let why = format!(
                "{} has no tool to ask a question with",
                planned.seats.worker.name()
            );
            log.say(format!("step question skipped: {why}"));
            rows.push(row(Outcome::Skipped(why), 0));
            continue;
        }
        log.say(format!("step {}: begins", step.name()));
        let started = Instant::now();
        match steps::run(ctx, *step) {
            Ok(()) => {
                log.say(format!(
                    "step {}: PASSED in {} s",
                    step.name(),
                    since(started)
                ));
                rows.push(row(Outcome::Passed, since(started)));
            }
            Err(why) => {
                log.say(format!(
                    "step {}: FAILED in {} s: {why}",
                    step.name(),
                    since(started)
                ));
                report::failure(ctx);
                failed = true;
                rows.push(row(Outcome::Failed(why.to_string()), since(started)));
            }
        }
    }
    rows
}

/// What the chief's screen shows at the end of the project of agents that a
/// turn left running, as a finding: Claude Code's window says so on its status
/// line.
fn background_agents(ctx: &Ctx) {
    let Ok(chief) = ctx.human.chief() else { return };
    let screen = ctx.human.screen(&chief["pane"], 12);
    let pending = screen.iter().find(|line| {
        let line = line.to_lowercase();
        (line.contains("background") || line.contains("pending")) && line.contains("agent")
    });
    match pending {
        Some(line) => ctx.note(format!(
            "the chief's window ends with background agents on its screen (a fix is on another branch): {}",
            cut(line, 160)
        )),
        None => ctx.note("no background agents pending on the chief's screen at the end".to_owned()),
    }
}

/// What the daemon's log warned of while the project ran, as a finding: a pass
/// that took long is one, and so is anything it called an error.
fn daemon_warnings(ctx: &Ctx) {
    let log = ctx.rig.log();
    let warned = warnings(&log);
    if !warned.is_empty() {
        ctx.note(format!("the daemon's log warned: {}", warned.join(" | ")));
    }
}

/// Runs the project `planned`.
pub fn project(planned: &Planned, machine: &Machine, log: &Log) -> Done {
    let log = log.scoped(planned.name());
    let setup = |outcome, seconds| Row {
        project: planned.name(),
        step: "setup".to_owned(),
        outcome,
        seconds,
    };
    let started = Instant::now();
    log.say("starts its rig");
    let mut rig = match Rig::start(rig_for(planned, machine)) {
        Ok(rig) => rig,
        Err(why) => {
            return Done {
                rows: vec![setup(Outcome::Failed(why.to_string()), since(started))],
                findings: Vec::new(),
            }
        }
    };
    if live::keep_requested() {
        log.say(format!(
            "{} is set: the home and the project stay at {}",
            live::KEEP,
            rig.keep_home().display()
        ));
    }
    let mut done = Done {
        rows: Vec::new(),
        findings: Vec::new(),
    };
    let mut folder = None;
    match open(planned, machine, &rig, &log) {
        Err(why) => {
            log.say(format!("setup FAILED: {why}"));
            done.rows
                .push(setup(Outcome::Failed(why.to_string()), since(started)));
        }
        Ok((made, project)) => {
            folder = Some(made.clone());
            let ctx = Ctx::new(&rig, project, planned, &made, &log);
            match ctx.human.waits_for_the_chief() {
                Err(why) => {
                    log.say(format!("setup FAILED: {why}"));
                    report::failure(&ctx);
                    done.rows
                        .push(setup(Outcome::Failed(why.to_string()), since(started)));
                }
                Ok(()) => {
                    log.say(format!(
                        "the chief's window is ready after {} s",
                        since(started)
                    ));
                    done.rows.push(setup(Outcome::Passed, since(started)));
                    done.rows.extend(steps_of(&ctx, planned, &log));
                    background_agents(&ctx);
                    daemon_warnings(&ctx);
                }
            }
            done.findings = ctx.findings.take();
        }
    }
    // The paths the harnesses know the folder by are asked while it is there.
    let known = match (folder.as_deref(), live::keep_requested()) {
        (Some(folder), false) => cleanup::paths(folder),
        _ => Vec::new(),
    };
    if let Err(why) = rig.close() {
        log.say(format!("closing the rig: {why}"));
        done.rows.push(Row {
            project: planned.name(),
            step: "close".to_owned(),
            outcome: Outcome::Failed(why.to_string()),
            seconds: 0,
        });
    }
    forget(machine, &known, &log);
    done
}

/// Has the harnesses forget what they kept of a project's folder, now that its
/// windows are gone: Claude Code by its own `purge` (its entry in
/// `~/.claude.json`, which holds the trust the run asked of it, and the
/// prompts the human typed in the folder), and, where they are, the sessions
/// Claude Code and Pi wrote in their own folders.
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
