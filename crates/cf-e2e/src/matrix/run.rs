//! One seating and one scenario in a fresh project: the rig started with the board's roster
//! and the scenario's fixture, the staff set up as the human sets it, each step asked by the
//! scripted human and waited for on the board within the scenario's bounds, and then the
//! oracle. The evidence is written before the rig is closed (which drops the harnesses' homes),
//! and again after, with what the close found.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use crate::live::ask::Item;
use crate::live::members::Members;
use crate::live::report::Check;
use crate::live::Log;
use crate::Result;

use super::evidence::{run_id, Evidence, Owner};
use super::human::{Bounds, Driver, Waited};
use super::observe::{self, Bindings, Snapshot};
use super::oracle::{self, Records};
use super::scenario::Scenario;
use super::seating::{Role, Seating};

/// How many lines of each window's screen the evidence keeps.
const LAST_LINES: usize = 60;

/// What a run is given.
pub struct Matrix<'a> {
    pub seating: Seating,
    pub scenario: Scenario,
    /// The stand-in agent's program: the windows of the stand-in seats run it, and a rig has
    /// one where none does.
    pub stand_in: &'a Path,
    /// Where the evidence's folder is made.
    pub reports: PathBuf,
    /// The owner's files, as they were before anything started.
    pub owner: Owner,
}

/// What a run came to.
pub struct Outcome {
    pub checks: Vec<Check>,
    pub evidence: Evidence,
    /// From the first ask to the last look.
    pub elapsed: Duration,
    /// The turns each seat began, and all of them together.
    pub turns: BTreeMap<&'static str, u32>,
    pub total_turns: u32,
}

/// The records of the rig's windows, as the daemon reads them.
struct RigRecords<'a> {
    members: &'a Members,
    events: Vec<Value>,
}

impl Records for RigRecords<'_> {
    fn items(&self, kind: &str, handle: &str) -> Result<Vec<Item>> {
        observe::record(self.members, &self.events, kind, handle)
    }
}

/// Runs `matrix`: the checks and the evidence; the error is the rig's, a run that could not be
/// set up at all (its evidence's manifest is written before it starts).
pub fn run(matrix: &Matrix, log: &Log) -> Result<Outcome> {
    let Matrix {
        seating,
        scenario,
        stand_in,
        reports,
        owner,
    } = matrix;
    let id = run_id(seating.board, scenario.name);
    let evidence = Evidence::under(reports, &id);
    let mut manifest = json!({
        "run": id,
        "board": seating.board,
        "seats": seating.manifest(),
        "scenario": {
            "name": scenario.name,
            "steps": scenario.steps.iter().map(|step| step.name).collect::<Vec<_>>(),
            "fixture": scenario.fixture.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            "deadlineSeconds": scenario.deadline.as_secs(),
            "turnCeiling": scenario.turns,
        },
        "notCovered": seating.not_covered().into_iter().map(Role::name).collect::<Vec<_>>(),
        "ownerBefore": owner.manifest(),
        "startedAt": jiff::Timestamp::now().to_string(),
    });
    evidence.json("manifest.json", &manifest)?;
    log.say(format!(
        "the run's evidence goes to {}",
        evidence.folder.display()
    ));
    let not_covered: Vec<&str> = seating.not_covered().into_iter().map(Role::name).collect();
    if !not_covered.is_empty() {
        evidence.text(
            "NOT-COVERED.txt",
            &format!(
                "The rig's stand-in played {} in this run: it covers no harness there.\n",
                not_covered.join(", ")
            ),
        )?;
    }

    let folder_name = format!("matrix-{id}");
    let plan = seating.plan(&folder_name, &scenario.fixture);
    let mut members = Members::start(&plan, stand_in, log)?;
    log.say(format!(
        "the project is open in {}",
        members.folder().display()
    ));
    let mut checks = seating.staff(&mut members);
    log.say("the staff is set up");
    let ready = members.chief_is_idle(log)?;
    checks.push(Check::new(
        "chief",
        &format!(
            "the chief's window ({}) went idle",
            seating.seat(Role::Chief).who()
        ),
        ready,
        members.lane("chief").map_or_else(
            |failed| failed.to_string(),
            |lane| lane["activity"].to_string(),
        ),
    ));

    let chief_kind = seating.seat(Role::Chief).kind();
    let mut driver = Driver::new(
        &members,
        log,
        Bounds::starting(scenario.deadline, scenario.turns),
        chief_kind,
    );
    let mut over = None;
    if ready {
        over = steps(&mut driver, seating, scenario, &id, &mut checks)?;
    }
    let elapsed = driver.bounds.elapsed();
    let asks_in = driver.asks_in();
    let said = std::mem::take(&mut driver.said);
    drop(driver);

    let snapshot = Snapshot::of(&members)?;
    let bindings = Bindings::of(&snapshot);
    let records = RigRecords {
        members: &members,
        events: snapshot.events.clone(),
    };
    checks.extend(oracle::asks(&said, scenario.steps.len()));
    checks.extend(oracle::tasks(seating, &snapshot, &bindings, &records));
    checks.extend(oracle::fixture(members.folder()));
    let turns = snapshot.turns_by_seat(seating, asks_in);
    let total_turns = snapshot.total_turns(asks_in);
    checks.push(Check::new(
        "bounds",
        "the scenario ended within its deadline and its turn ceiling",
        over.is_none() && elapsed <= scenario.deadline && total_turns <= scenario.turns,
        format!(
            "{} s of {}, {total_turns} turns of {}{}",
            elapsed.as_secs(),
            scenario.deadline.as_secs(),
            scenario.turns,
            over.as_ref()
                .map_or_else(String::new, |why| format!(": {why}"))
        ),
    ));

    manifest["elapsedSeconds"] = json!(elapsed.as_secs());
    manifest["turns"] = json!(turns);
    manifest["totalTurns"] = json!(total_turns);
    manifest["project"] = json!(members.folder());
    evidence.json("manifest.json", &manifest)?;
    evidence.json(
        "human.json",
        &Value::Array(said.iter().map(super::human::Said::manifest).collect()),
    )?;
    evidence.json("bindings.json", &bindings_of(&snapshot, &bindings))?;
    windows(&members, &evidence)?;
    evidence.text(
        "events.jsonl",
        &snapshot
            .events
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )?;
    evidence.checks(&checks)?;

    // The close drops the harnesses' homes, and the kit's look at the owner's setup is its.
    let folder = members.folder().to_path_buf();
    let closed = members.close();
    checks.push(match closed {
        Ok(()) => Check::passed(
            "owner",
            "the rig ended, and the kit found every harness's home of the owner's as it was",
            "closed",
        ),
        Err(failed) => Check::failed(
            "owner",
            "the rig ended, and the kit found every harness's home of the owner's as it was",
            failed.to_string(),
        ),
    });
    checks.push(owner.unchanged(&owner.again()));
    evidence.checks(&checks)?;
    if checks.iter().all(|check| check.ok) {
        // A green run's project is no evidence: the folder it made goes (a red one's stays, named in its manifest).
        let _ = std::fs::remove_dir_all(&folder);
    }
    Ok(Outcome {
        checks,
        evidence,
        elapsed,
        turns,
        total_turns,
    })
}

/// Asks each step and waits for it on the board, within the bounds: why the scenario was
/// ended, where its bounds ended it. A step that did not go in, or was not reached, is a check
/// that failed, and the steps after it are not asked.
fn steps(
    driver: &mut Driver,
    seating: &Seating,
    scenario: &Scenario,
    id: &str,
    checks: &mut Vec<Check>,
) -> Result<Option<String>> {
    for (index, step) in scenario.steps.iter().enumerate() {
        if let Some(why) = driver.over_now()? {
            return Ok(Some(why));
        }
        let snapshot = Snapshot::of(driver.members)?;
        let bindings = Bindings::of(&snapshot);
        let text = if seating.chief_is_stand_in() {
            format!("\n{}", (step.stand_in)(&bindings))
        } else {
            step.asks.to_owned()
        };
        let marker = scenario.marker(id, index);
        let said = driver.asks(step.name, &marker, &text)?.clone();
        if !said.went_in() {
            let why = said.failed.unwrap_or_else(|| "it did not go in".to_owned());
            return Ok(driver
                .over_now()?
                .or(Some(format!("ask {} did not go in: {why}", index + 1))));
        }
        let reached = driver.wait(
            &format!("the step {} on the board", step.name),
            scenario.deadline,
            || {
                let snapshot = Snapshot::of(driver.members)?;
                let bindings = Bindings::of(&snapshot);
                Ok((step.reached)(&snapshot, &bindings).then_some(()))
            },
        )?;
        let seconds = driver.bounds.elapsed().as_secs();
        match reached {
            Waited::Found(()) => checks.push(Check::passed(
                "steps",
                &format!("step {} ({}) was reached", index + 1, step.name),
                format!("at {seconds} s"),
            )),
            Waited::NotYet => {
                checks.push(Check::failed(
                    "steps",
                    &format!("step {} ({}) was reached", index + 1, step.name),
                    format!("not by {seconds} s"),
                ));
                return Ok(Some(format!("step {} was not reached", index + 1)));
            }
            Waited::Over(why) => {
                checks.push(Check::failed(
                    "steps",
                    &format!("step {} ({}) was reached", index + 1, step.name),
                    why.clone(),
                ));
                return Ok(Some(why));
            }
        }
    }
    Ok(None)
}

/// Each task's binding, as the evidence has it: the logical name, the numbers, and each
/// task's state, pool, tier, requester and assignee.
fn bindings_of(snapshot: &Snapshot, bindings: &Bindings) -> Value {
    let tasks: Vec<Value> = snapshot
        .tasks
        .iter()
        .map(|task| {
            let assignee = task["assignee"].as_str().unwrap_or_default();
            json!({
                "number": task["number"],
                "state": task["state"],
                "pool": task["pool"],
                "tier": task["tier"],
                "requester": task["requester"],
                "assignee": task["assignee"],
                "assigneeAgent": snapshot.agent_of(assignee),
                "title": task["title"],
            })
        })
        .collect();
    json!({ "bound": bindings.manifest(), "tasks": tasks })
}

/// How many of the daemon's last lines the evidence keeps.
const DAEMON_LINES: usize = 400;

/// Each window's last lines, a file to each pane the daemon opened; how each was opened (its
/// pane, generation and command line: never its environment, which holds the board's tokens);
/// and the daemon's last lines.
fn windows(members: &Members, evidence: &Evidence) -> Result {
    let opens: Vec<Value> = members
        .rig()
        .open_frames()
        .iter()
        .map(|frame| json!({ "id": frame["id"], "generation": frame["generation"], "argv": frame["argv"] }))
        .collect();
    evidence.json("opens.json", &Value::Array(opens))?;
    let log = members.rig().log();
    let lines: Vec<&str> = log.lines().collect();
    evidence.text(
        "daemon-tail.log",
        &format!(
            "{}\n",
            lines[lines.len().saturating_sub(DAEMON_LINES)..].join("\n")
        ),
    )?;
    let mut ids: Vec<String> = members
        .rig()
        .open_frames()
        .iter()
        .filter_map(|frame| frame["id"].as_str().map(str::to_owned))
        .collect();
    ids.sort();
    ids.dedup();
    for id in ids {
        let screen = members.screen(&id);
        let lines: Vec<&str> = screen.lines().collect();
        let last = lines[lines.len().saturating_sub(LAST_LINES)..].join("\n");
        evidence.text(&format!("windows/{id}.txt"), &format!("{last}\n"))?;
    }
    Ok(())
}
