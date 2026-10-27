//! The run of an eval: real harnesses as the chief and the staff of a project on the
//! native daemon, a scripted owner who answers what they ask, the gates that tell a
//! run is done, the switch of a chief to another harness half-way, and the three
//! arms (the card, the bare harness, the bare harness with a card). A scenario is
//! code, not data ([`crate::scenarios`]): its expectations are closures over the
//! measure's numbers. Spends real tokens; never part of a gate.
//!
//! ```text
//! cargo xtask eval --scenario six-decisions [--chief claude] [--staff claude,codex]
//! ```
//!
//! It is built on `crates/cf-e2e`: the rig ([`cf_e2e::rig`]) and what the live tests
//! share ([`cf_e2e::live`]: the machine's harnesses, the trust Claude asks of a
//! folder, the harness code of the product asked as a process). Three things are
//! easy to miss in how it runs:
//!
//! - the owner presses Enter again, up to three times, unless the trace shows the
//!   chief working, for Devin and for Codex on Windows, where a pasted prompt waits
//!   in the box (`chief::Chief::says`);
//! - the wrapper every Claude and Codex window starts through, first on the `PATH`,
//!   is a `.cmd` on Windows that names the Node and the script (the shape of an npm
//!   shim, which is how the pane host opens a window on it), and a shell script on
//!   the Mac and for a Pi chief, whose wrapper is the Mac's alone (`wrappers`);
//! - the chief's open question stays open through later turns that ask nothing
//!   ([`crate::measure::chief_open_question`]).

mod alone;
// What a long run (`crate::long_run`) is made of too: how the board is read, the
// chief's window, where a run speaks, the machine and its wrappers, the scripted
// owner and the workspace.
pub(crate) mod card;
pub(crate) mod chief;
pub(crate) mod floor;
pub(crate) mod login;
pub(crate) mod machine;
mod options;
pub(crate) mod owner;
mod report;
pub(crate) mod workspace;
pub(crate) mod wrappers;

use std::io::Write;
use std::path::Path;

use cf_e2e::daemon_log::Kind;
use cf_e2e::live::{ask, window, ISOLATION};
use cf_e2e::rig::Rig;

use floor::Floor;
use machine::Machine;
pub use options::{Arm, Crew, Options};
use wrappers::Wrappers;

use crate::plan::{chief_environment, harness, staff_for, Agent, ChiefSetup, Staff};
use crate::Result;

/// What is decided before a run begins, and holds for each run of them.
pub struct Plan<'a> {
    pub options: &'a Options,
    pub machine: &'a Machine,
    pub wrappers: &'a Wrappers,
    pub chief: &'a ChiefSetup,
    /// The chief's agent, which the daemon opens no project without: the chief's
    /// harness, model and effort.
    pub agent: Agent,
    pub staff: Staff,
}

/// `run`: plays the scenario `repeat` times and says whether it held each time: the
/// status is 0 when it did.
pub fn command(words: &[String], out: &mut dyn Write) -> Result<i32> {
    let options = Options::of(words)?;
    // The harness code of the product is asked as a process: built first, before the
    // first harness spends anything, so that a build that cannot be made stops the run here.
    ask::prepare()?;
    let chief = chief_environment(&options.chief, options.model.as_deref())?;
    let staff = staff_for(
        &options.staff,
        &options.staff_models(),
        Some(&options.staff_effort),
    )?;
    let agent = Agent::new(
        format!("eval-{}-chief", options.chief),
        harness(&options.chief)?,
        &chief.model,
        options.chief_effort(),
    );
    let crew = options.crew();
    // The windows of a real Claude Code run on a throwaway config folder with a copy of the
    // machine's login, which must last as long as the plays of this command can: every repeat
    // at its time limit (a login that would end before is refused when the first workspace is
    // made, before anything is spent).
    let login = login::throwaway(
        login::real_claude(&options.chief, &chief.model, &staff),
        options.planned(),
    );
    let machine = Machine::of(&crew, login)?;
    let wrappers = wrappers::write(&machine, &crew, &chief)?;
    let plan = Plan {
        options: &options,
        machine: &machine,
        wrappers: &wrappers,
        chief: &chief,
        agent,
        staff,
    };
    let mut held = true;
    for index in 1..=options.repeat {
        let this = if options.arm == Arm::Bare {
            alone::play(&plan, index, out)?
        } else {
            card::play(&plan, index, out)?
        };
        held = this && held;
    }
    Ok(i32::from(!held))
}

/// Starts the rig of a run on this machine, says which daemon it is (the Windows
/// matrix reads that back to say what it ran against), and trusts the fresh
/// workspace for Claude in a window of the run's own host when Claude is in the run.
fn start(plan: &Plan, floor: &mut Floor, workspace: &Path) -> Result<Rig> {
    let options = plan.options;
    // The rig has a `claude` of its own to put first on the `PATH`, which the `PATH`
    // of a run leaves out: the wrapper is the program it would have named.
    let stand_in = plan
        .wrappers
        .claude
        .clone()
        .unwrap_or_else(|| plan.machine.bin().join("claude"));
    let rig = Rig::start(
        plan.machine
            .rig(&options.crew(), plan.chief, &stand_in)
            .case("an eval run"),
    )?;
    floor.say(format!("daemon: {}", daemon_of(&rig)))?;
    if options.in_run("claude") {
        trust(plan.machine, &rig, workspace, floor)?;
    }
    Ok(rig)
}

/// Which daemon a rig runs, as a run says it: `native (rust 1.90.0)`.
pub(crate) fn daemon_of(rig: &Rig) -> String {
    let daemon = rig.start_line();
    format!(
        "{} ({})",
        match daemon.kind {
            Kind::Native => "native",
            Kind::Node => "node",
        },
        daemon.runtime
    )
}

/// Claude asked to trust the fresh `workspace`, in a window of the rig's own host
/// started as the machine's Claude is, and what came of it said.
pub(crate) fn trust(machine: &Machine, rig: &Rig, workspace: &Path, floor: &mut Floor) -> Result {
    let claude = machine.install("claude")?;
    let words: Vec<String> = std::iter::once(claude.command().to_string_lossy().into_owned())
        .chain(ISOLATION.map(str::to_owned))
        .collect();
    let argv = ask::program(&words, rig.environment())?;
    let trust = window::trust_claude(rig, &argv, workspace)?;
    floor.say(format!("trust: {trust}"))?;
    Ok(())
}
