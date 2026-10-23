//! What a long run is set up with before its first look: the harnesses this machine
//! has (each with its version, and the ones left out with why), the staff on the
//! models each has, the chief on its model, the wrappers every window starts
//! through, the fixture copied into a fresh trusted workspace, the rig of the
//! native daemon and the real pane host, and the project opened as the New project
//! dialog opens one: with the staff, specs on, and the owner's wake-up calls.

use std::path::PathBuf;
use std::sync::Arc;

use cf_e2e::live::{self, agents, Harness, Installed, Log};
use cf_e2e::rig::{Config, Masker, Rig};
use serde_json::{json, Value};

use super::options::Options;
use super::staff::{self, Found, LeftOut};
use crate::plan::{chief_environment, harness, Agent, ChiefSetup, Staff};
use crate::run::floor::Floor;
use crate::run::login::{self, Throwaway};
use crate::run::machine::Machine;
use crate::run::{daemon_of, trust, workspace, wrappers, Crew};
use crate::scenarios::Scenario;
use crate::{Error, Result};

/// Who the run is played by, before anything runs.
pub struct Cast {
    pub found: Vec<Found>,
    pub left: Vec<LeftOut>,
    /// The harnesses of the staff that are here, in the order asked.
    pub names: Vec<String>,
    pub staff: Staff,
    pub chief: ChiefSetup,
    pub agent: Agent,
    pub crew: Crew,
}

impl Cast {
    /// The cast `options` ask for, of the harnesses this machine has: an error that
    /// says why where the chief's harness, or every harness of the staff, is not.
    pub fn of(options: &Options) -> Result<Self> {
        let home = live::machine::home()?;
        let mut wanted = vec![options.chief.clone()];
        for name in &options.staff {
            if !wanted.contains(name) {
                wanted.push(name.clone());
            }
        }
        let (found, left) = staff::find(&wanted, |name| {
            let program = live::command(name).map_err(|failed| failed.to_string())?;
            let kind = Harness::from_name(name).ok_or_else(|| format!("{name} is no harness"))?;
            let version = Installed::at(kind, program.clone(), &home).version();
            Ok((program, version))
        });
        if let Some(gone) = left.iter().find(|left| left.name == options.chief) {
            return Err(Error::Refused(format!(
                "the chief's harness, {}, cannot run here: {}",
                gone.name, gone.why
            )));
        }
        let names: Vec<String> = options
            .staff
            .iter()
            .filter(|name| found.iter().any(|found| found.name == **name))
            .cloned()
            .collect();
        if names.is_empty() {
            return Err(Error::Refused(format!(
                "no harness of the staff can run here: {}",
                left.iter()
                    .map(|left| format!("{} ({})", left.name, left.why))
                    .collect::<Vec<_>>()
                    .join("; ")
            )));
        }
        let staff = staff::staff_of(
            &names,
            &staff::models_of(options.models.as_deref())?,
            &options.staff_effort,
        )?;
        let chief = chief_environment(&options.chief, options.model.as_deref())?;
        let agent = Agent::new(
            "long-chief".to_owned(),
            harness(&options.chief)?,
            &chief.model,
            options.chief_effort(),
        );
        let crew = Crew {
            chief: options.chief.clone(),
            staff: names.clone(),
            effort: options.effort.clone(),
            nocard: false,
        };
        Ok(Self {
            found,
            left,
            names,
            staff,
            chief,
            agent,
            crew,
        })
    }
}

/// The rig of the run, started, with its project open.
pub struct Opened {
    pub rig: Rig,
    pub config: Config,
    pub project: i64,
    pub daemon: String,
    /// Where the run's Claude Code windows keep their records: the throwaway folder of the
    /// run, which goes with it.
    pub claude: PathBuf,
    /// The login those windows run on, which says at the end what the run did to the owner's
    /// Claude setup or to the copy of the login.
    pub login: Option<Arc<Throwaway>>,
}

/// Starts the rig the cast runs on and opens the project: the fixture of
/// `scenario` in a fresh workspace, trusted by Claude where Claude is of the run,
/// the roster written, the project opened with the staff, specs on and wake-up
/// calls every `options.wake_minutes`.
pub fn open(
    options: &Options,
    cast: &Cast,
    scenario: &Scenario,
    floor: &mut Floor,
    masker: Masker,
) -> Result<Opened> {
    // The windows of a real Claude Code run on a throwaway config folder with a copy of the
    // machine's login, which must last the run's budget and its end (a login that would end
    // before is refused when the workspace is made, before anything is spent).
    let login = login::throwaway(
        login::real_claude(&options.chief, &cast.chief.model, &cast.staff),
        options.budget,
    );
    let machine = Machine::of(&cast.crew, login.clone())?;
    let wrappers = wrappers::write(&machine, &cast.crew, &cast.chief)?;
    let workspace = workspace::fresh(&machine, scenario)?;
    let claude_config = machine.claude_records()?;
    if login.is_some() {
        // Where the run's Claude records are while it lasts (`cargo xtask watch --tokens
        // --claude-config`), and in the kept home after it.
        floor.say(format!("claude config: {}", claude_config.display()))?;
    }
    let stand_in = wrappers
        .claude
        .clone()
        .unwrap_or_else(|| machine.bin().join("claude"));
    // What the rig says of itself, dropped unclosed, leaves through the run's edge;
    // the rig started again on a restart is given the same.
    let config = machine
        .rig(&cast.crew, &cast.chief, &stand_in)
        .masked_by(masker);
    let rig = Rig::start(config.clone())?;
    let daemon = daemon_of(&rig);
    floor.say(format!("daemon: {daemon}"))?;
    if cast.crew.in_run("claude") {
        trust(&machine, &rig, &workspace, floor)?;
    }
    let mut rows: Vec<Value> = vec![cast.agent.to_value()];
    rows.extend(cast.staff.agents.iter().map(Agent::to_value));
    agents::write_roster(&rig, &rows, &Log::new("long run"))?;
    let opened = rig.page(
        "project.open",
        json!({
            "directory": workspace,
            "agent": cast.agent.id,
            "staff": cast.staff.to_value(),
            "specs": { "on": true },
            "wakeUp": { "on": true, "every": options.wake_minutes, "unit": "minutes" },
        }),
    )?;
    let Some(project) = opened["project"]["id"]
        .as_i64()
        .filter(|_| opened["ok"] == true)
    else {
        return Err(Error::Refused(format!("project.open: {opened}")));
    };
    Ok(Opened {
        rig,
        config,
        project,
        daemon,
        claude: claude_config,
        login,
    })
}
