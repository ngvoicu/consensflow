//! Real harnesses behind the human's gate: the proof that a project with
//! "human approval required" checked sends its messages, which the fake agents of
//! the rig cannot give. It is opt-in (`cargo xtask live approval`, `npm run
//! live:approval`): it spends the machine's quota on all five harnesses (Claude
//! Code, OpenCode, Pi, Devin and Codex, each on its cheap model) and needs each
//! one installed and logged in, so it is ignored unless asked for and is no part
//! of `check` or of CI.
//!
//! The native daemon and the real pane host run in a home of the run's own
//! (never `~/.consensflow`), one rig for each project, one project after
//! another. The windows run the machine's harnesses and read their logins where
//! they are (the machine's home, and `CODEX_HOME`; Claude Code with no
//! `CLAUDE_CONFIG_DIR`, started through a wrapper that adds
//! `--strict-mcp-config --no-chrome`), and know nothing of the shell that runs
//! this: the rig gives them an environment of its own.
//!
//! Five projects, a harness the chief of each ([`plan`]): the worker, the
//! advisor and the reviewer are the next three after it in the order of
//! [`cf_e2e::live::Harness::ALL`], so that every harness holds every seat once.
//! Each project is a folder in the run's home (`git init`, a README, the bench's
//! brief in `AGENTS.md` and `CLAUDE.md`), opened with its staff and with the
//! gate on by the call the Staff dialog's checkbox makes ([`project`]).
//!
//! The human is the page's calls (`message.approve` and `message.decline`, what
//! the board's buttons make) and the keys typed into the chief's window, which
//! the pane host takes as it takes the app's ([`human`]); the chief is a real
//! harness that runs `cf` itself. Each project runs the steps of [`steps`]:
//!
//! 1. a task for the worker, its brief and its result each held at the gate; the
//!    result is passed on while the chief is at work on a command of
//!    twenty-five seconds, and is typed into it once its turn is over;
//! 2. one for the advisor, a minute of work, paused and resumed with other
//!    words, which are held too;
//! 3. one for the reviewer, whose brief the human declines;
//! 4. a question the worker asks the chief by its harness's tool, and the
//!    answer, held (Pi has no such tool);
//! 5. and in one project, a task for the image designer.
//!
//! Every step waits with a deadline and says what it waits for ([`flow`]), and
//! is judged by what the daemon showed: the board's states, each message's state
//! and how often it was delivered, the daemon's trace and what the windows
//! recorded ([`verdict`], [`trace`]). A step that fails says what the product
//! showed ([`report`]). Two things are noted in passing and fail nothing: Codex's
//! directive markup in a result, and a chief's turn that ended with background
//! agents pending.
//!
//! `cargo xtask live approval [harness ...]` limits the run to the projects the
//! harnesses named are the chief of (`--steps worker,question` to some steps);
//! `--keep` leaves the homes where they are.

// The parts are in `live_approval/`: cargo looks for the modules of a test's
// root file beside it, where each would be a suite of its own.
#[path = "live_approval/agents.rs"]
mod agents;
#[path = "live_approval/cleanup.rs"]
mod cleanup;
#[path = "live_approval/flow.rs"]
mod flow;
#[path = "live_approval/human.rs"]
mod human;
#[path = "live_approval/plan.rs"]
mod plan;
#[path = "live_approval/project.rs"]
mod project;
#[path = "live_approval/report.rs"]
mod report;
#[path = "live_approval/run.rs"]
mod run;
#[path = "live_approval/steps.rs"]
mod steps;
#[path = "live_approval/verdict.rs"]
mod verdict;

use std::path::{Path, PathBuf};

use cf_e2e::live::{self, Harness, Log};
use cf_e2e::{Error, Result};

use report::{Outcome, Row};
use run::Machine;

/// The stand-in for the chief's window, built for these tests: it is the
/// program the rig puts first on the `PATH` as `claude`, which a project with no
/// Claude Code in it never starts.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// Where the run keeps what it makes outside its rigs' homes: the wrapper that
/// starts Claude Code.
fn kept() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join("live-approval")
}

/// The harnesses and the steps the run is limited to: the words of the
/// variables the command sets, or none.
fn limits() -> Result<(Vec<Harness>, Vec<plan::Step>)> {
    let named = |name: &str| live::requested(name).unwrap_or_default();
    let harnesses = plan::harnesses_named(&named(live::HARNESS)).map_err(Error::Daemon)?;
    let steps = plan::steps_named(&named(live::STEPS)).map_err(Error::Daemon)?;
    Ok((harnesses, steps))
}

/// The harnesses the plan needs, found on the machine as a person's shell finds
/// them, each with its version said; one that is not there, or not logged in,
/// is the error, before anything is spent.
fn machine(plan: &[plan::Planned], log: &Log) -> Result<Machine> {
    let mut needed: Vec<Harness> = Vec::new();
    for harness in plan.iter().flat_map(plan::Planned::needs) {
        if !needed.contains(&harness) {
            needed.push(harness);
        }
    }
    let mut installed = Vec::new();
    for harness in needed {
        let found = live::installed(harness)?;
        log.say(format!(
            "{} is {} ({}); its login is read from {}",
            harness.name(),
            found.command().display(),
            found.version(),
            found.home().display()
        ));
        installed.push(found);
    }
    let claude = match installed
        .iter()
        .find(|found| found.harness() == Harness::Claude)
    {
        Some(found) => Some(live::isolated_claude(&kept(), found.command())?),
        None => None,
    };
    Ok(Machine {
        installed,
        claude,
        stand_in: PathBuf::from(FAKE_AGENT),
    })
}

#[test]
#[ignore = "opt-in: spends the machine's quota on all five harnesses; cargo xtask live approval (npm run live:approval)"]
fn the_gate_sends_what_the_human_approves_between_real_harnesses_in_every_seat() -> Result {
    let log = Log::new("live approval");
    let (chiefs, steps) = limits()?;
    let plan = plan::projects(&chiefs, &steps);
    if plan.iter().all(|project| project.steps.is_empty()) {
        return Err(Error::Daemon(
            "the harnesses and the steps asked for leave nothing to run".to_owned(),
        ));
    }
    let machine = machine(&plan, &log)?;
    log.say(format!(
        "{} project{} to run, one after another: {}",
        plan.len(),
        if plan.len() == 1 { "" } else { "s" },
        plan.iter()
            .map(plan::Planned::name)
            .collect::<Vec<_>>()
            .join(", ")
    ));

    let mut rows: Vec<Row> = Vec::new();
    let mut findings: Vec<String> = Vec::new();
    for planned in &plan {
        let done = run::project(planned, &machine, &log);
        findings.extend(
            done.findings
                .into_iter()
                .map(|finding| format!("{}: {finding}", planned.name())),
        );
        rows.extend(done.rows);
    }

    log.block("what each step came to", &report::table(&rows));
    if !findings.is_empty() {
        log.block("what the run noted in passing", &findings.join("\n"));
    }
    let failed: Vec<&Row> = rows
        .iter()
        .filter(|row| matches!(row.outcome, Outcome::Failed(_)))
        .collect();
    if failed.is_empty() {
        log.say(format!(
            "ok: {} steps passed through the gate, {} skipped",
            rows.iter()
                .filter(|row| row.outcome == Outcome::Passed)
                .count(),
            rows.iter()
                .filter(|row| matches!(row.outcome, Outcome::Skipped(_)))
                .count()
        ));
        return Ok(());
    }
    Err(Error::Daemon(format!(
        "{} of {} steps failed:\n{}",
        failed.len(),
        rows.len(),
        report::table(&rows)
    )))
}
