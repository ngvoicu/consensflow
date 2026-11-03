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

use super::evidence::{self, fixture_hashes, run_id, Evidence, Owner};
use super::human::{Bounds, Driver, LOOK};
use super::machine::{Next, Observation, Steps};
use super::observe::{self, Bindings, Snapshot};
use super::oracle::{self, Answered, Judged, Records};
use super::scenario::{Known, Scenario};
use super::seating::{self, Role, Seating};

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
    /// The run this one replays, where it does ([`replay`]).
    pub replay_of: Option<String>,
}

/// A stand-in board's deadline, whatever its scenario's: a stand-in answers at once, and a run
/// that waits longer is a red to say, not to wait out.
pub const STAND_IN_DEADLINE: Duration = Duration::from_secs(180);

/// The deadline of `scenario` on a stand-in board: [`STAND_IN_DEADLINE`] and what it waits by
/// design, never more than its own.
pub fn stand_in_deadline(scenario: &Scenario) -> Duration {
    scenario.deadline.min(STAND_IN_DEADLINE + scenario.waits)
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
    /// The scenario's checks that fail on today's main by design, each until the work its
    /// detail names lands: never among the passes.
    pub known: Vec<Check>,
}

impl Outcome {
    /// The checks that did not hold, the known reds aside.
    pub fn failed(&self) -> Vec<&Check> {
        self.checks.iter().filter(|check| !check.ok).collect()
    }
}

/// `checks`, with the scenario's `known` reds taken out: each known red that failed is said
/// apart (its detail names the work that turns it green); one that held is a red of its own,
/// for its mark is to go; and one that no check made is a red too.
pub fn sort_known(checks: Vec<Check>, known: &[Known]) -> (Vec<Check>, Vec<Check>) {
    let mut kept = Vec::new();
    let mut reds = Vec::new();
    let mut seen = vec![false; known.len()];
    for check in checks {
        let mark = known
            .iter()
            .position(|known| check.who == known.who && check.what.contains(known.what));
        match mark {
            Some(at) if !check.ok => {
                seen[at] = true;
                reds.push(Check::failed(
                    &check.who,
                    &check.what,
                    format!("known red until {}: {}", known[at].until, check.detail),
                ));
            }
            Some(at) => {
                seen[at] = true;
                kept.push(Check::failed(
                    &check.who,
                    &check.what,
                    format!(
                        "a known red now holds ({}): take its mark out; {}",
                        known[at].until, check.detail
                    ),
                ));
            }
            None => kept.push(check),
        }
    }
    for (known, seen) in known.iter().zip(seen) {
        if !seen {
            kept.push(Check::failed(
                known.who,
                known.what,
                format!("a known red no check made (until {})", known.until),
            ));
        }
    }
    (kept, reds)
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

    fn transcript(&self, number: i64, last: usize) -> Result<Vec<Value>> {
        let answer = self.members.page(
            "task.transcript",
            serde_json::json!({ "task": number, "limit": last }),
        )?;
        Ok(answer["items"].as_array().cloned().unwrap_or_default())
    }

    fn escapes(&self, handle: &str) -> usize {
        let id = observe::pane_id(self.members, handle);
        self.members.rig().keys_pressed(&id, &[27])
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
        replay_of,
    } = matrix;
    let mut scenario = scenario.clone();
    if seating.control {
        scenario.deadline = stand_in_deadline(&scenario);
    }
    let scenario = &scenario;
    let id = run_id(&seating.label(), scenario.name);
    let evidence = Evidence::under(reports, &id);
    let mut manifest = json!({
        "run": id,
        "replayOf": replay_of,
        "source": source(),
        "seating": seating.manifest(),
        "scenario": {
            "name": scenario.name,
            "version": scenario.version,
            "steps": scenario.steps.iter().map(|step| step.name).collect::<Vec<_>>(),
            "fixture": fixture_hashes(&scenario.fixture),
            "deadlineSeconds": scenario.deadline.as_secs(),
            "turnCeiling": scenario.turns,
            "known": scenario.known.iter().map(|known| json!({ "who": known.who, "what": known.what, "until": known.until })).collect::<Vec<_>>(),
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
    let mut plan = seating.plan(&folder_name, &scenario.fixture);
    plan.finder = scenario.finder;
    for (name, value) in scenario.vars {
        plan = plan.with_var(name, *value);
    }
    for (name, value) in scenario.daemon_vars {
        plan = plan.with_daemon_var(name, *value);
    }
    for extra in scenario.extra {
        plan = plan.with(seating::stand_in_row(extra.agent, extra.tier));
    }
    let mut members = Members::start(&plan, stand_in, log)?;
    log.say(format!(
        "the project is open in {}",
        members.folder().display()
    ));
    let mut checks = seating.staff(&mut members);
    for extra in scenario.extra {
        let what = format!("{} {} is staff at {}", extra.role, extra.agent, extra.tier);
        checks.push(match members.add(extra.agent, &[extra.role]) {
            Ok(tier) => Check::new(
                "staff",
                &what,
                tier == extra.tier,
                format!("member.add gave {} the tier {tier:?}", extra.agent),
            ),
            Err(failed) => Check::failed("staff", &what, failed.to_string()),
        });
    }
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
        // A step the app is restarted before stops the steps there: the app quits and starts
        // again on the same home, the human resumes the project, and the steps go on from it
        // with the bounds and what the human typed so far.
        let (mut from, mut restarted, mut said) = (0, false, 0);
        loop {
            match steps(
                &mut driver,
                seating,
                scenario,
                &id,
                &mut checks,
                (from, restarted),
                &mut said,
            )? {
                Stop::Over(why) => {
                    over = why;
                    break;
                }
                Stop::Restart(at) => {
                    let (bounds, typed) = driver.carried();
                    members = members.restart(log)?;
                    let resumed = members.resume();
                    checks.push(Check::new(
                        "app",
                        &format!(
                            "the app started again before step {}, and the project resumed",
                            at + 1
                        ),
                        resumed.is_ok(),
                        resumed.map_or_else(
                            |failed| failed.to_string(),
                            |answer| answer["ok"].to_string(),
                        ),
                    ));
                    driver = Driver::resumed(&members, log, bounds, chief_kind, typed);
                    (from, restarted) = (at, true);
                }
            }
        }
    }
    let elapsed = driver.bounds.elapsed();
    let asks_in = driver.asks_in();
    let said = std::mem::take(&mut driver.said);
    drop(driver);

    let snapshot = Snapshot::of(&members)?;
    let bindings = Bindings::by(&snapshot, scenario.binds);
    let records = RigRecords {
        members: &members,
        events: snapshot.events.clone(),
    };
    let asked = scenario
        .steps
        .iter()
        .filter(|step| step.human.is_none())
        .count();
    checks.extend(oracle::asks(&said, asked));
    let judged = Judged {
        seating,
        snapshot: &snapshot,
        bindings: &bindings,
        records: &records,
        folder: members.folder(),
    };
    checks.extend((scenario.checks)(&judged));
    for refused in scenario.refusals {
        checks.push(oracle::chief_call(
            &judged,
            refused.task,
            refused.action,
            0,
            Answered::Refused(refused.code, &[]),
        ));
    }
    for (earlier, later) in scenario.order {
        checks.extend(oracle::after_done(&snapshot, &bindings, earlier, later));
    }
    let (mut checks, known) = sort_known(checks, scenario.known);
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
    // Assisted is not unassisted: what the human said besides the steps is said apart.
    manifest["assisted"] = json!({
        "answers": said.iter().filter(|said| said.step == "answer").count(),
        "nudges": said.iter().filter(|said| said.step == "nudge").count(),
    });
    manifest["project"] = json!(members.folder());
    evidence.json("manifest.json", &manifest)?;
    evidence.json(
        "human.json",
        &Value::Array(said.iter().map(super::human::Said::manifest).collect()),
    )?;
    evidence.json("bindings.json", &bindings_of(&snapshot, &bindings))?;
    windows(&members, &evidence)?;
    records_of(&members, seating, &snapshot, &evidence)?;
    evidence::ledger_snapshot(
        &members.home().join("consensflow.db"),
        &evidence.folder.join("ledger.db"),
    )?;
    evidence.text(
        "events.jsonl",
        &snapshot
            .events
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )?;
    checks.push(evidence::holds_none(&evidence.folder, &tokens(&members)));
    evidence.checks(&checks, &known)?;

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
    evidence.checks(&checks, &known)?;
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
        known,
    })
}

/// The commit the run's source is at, as git says it, and whether the tree had changes: what a
/// replay on other source says it differs from.
pub fn source() -> Value {
    let git = |words: &[&str]| {
        crate::process::Run::new("git")
            .args(words)
            .cwd(crate::checkout::root())
            .inheriting_env()
            .run()
            .map(|ran| ran.stdout.trim().to_owned())
            .unwrap_or_default()
    };
    json!({
        "commit": git(&["rev-parse", "HEAD"]),
        "changed": !git(&["status", "--porcelain", "--untracked-files=no"]).is_empty(),
    })
}

/// The tokens the windows were given (`CONSENSFLOW_TOKEN`, and any variable of a window's
/// whose name says it holds a key or a secret): what the evidence must not hold.
fn tokens(members: &Members) -> Vec<String> {
    members
        .rig()
        .open_frames()
        .iter()
        .filter_map(|frame| frame["env"].as_object().cloned())
        .flat_map(|env| env.into_iter())
        .filter(|(name, _)| {
            ["TOKEN", "SECRET", "API_KEY"]
                .iter()
                .any(|word| name.to_uppercase().contains(word))
        })
        .filter_map(|(_, value)| value.as_str().map(str::to_owned))
        .collect()
}

/// Each window's own record, as the daemon reads it, a file to each session of the run:
/// `records/<handle>.json`, its items' roles, words and ends.
fn records_of(
    members: &Members,
    seating: &Seating,
    snapshot: &Snapshot,
    evidence: &Evidence,
) -> Result {
    let mut handles: Vec<(String, &'static str)> =
        vec![("chief".to_owned(), seating.seat(Role::Chief).kind())];
    for event in snapshot
        .events
        .iter()
        .filter(|event| event["kind"] == "session.started")
    {
        let handle = event["data"]["handle"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let kind = event["data"]["member"]
            .as_str()
            .and_then(|agent| seating.of_agent(agent))
            .map_or("claude-code", super::seating::Seat::kind);
        handles.push((handle, kind));
    }
    for (handle, kind) in handles {
        let items = observe::record(members, &snapshot.events, kind, &handle).unwrap_or_default();
        let rows: Vec<Value> = items
            .iter()
            .map(|item| json!({ "id": item.id, "role": item.role, "text": item.text, "complete": item.complete }))
            .collect();
        evidence.json(&format!("records/{handle}.json"), &Value::Array(rows))?;
    }
    Ok(())
}

/// Runs again, alone, the scenario of the run whose manifest is `manifest`: in a fresh project,
/// with the same seats, its tasks bound again by what they are. A scenario of another version
/// is refused; source that changed since is said, in the log and the new manifest
/// (`replayOf`, `source`). The red's own folder is left as it was.
pub fn replay(
    manifest: &Path,
    stand_in: &Path,
    reports: PathBuf,
    owner: Owner,
    log: &Log,
) -> Result<Outcome> {
    let read: Value =
        serde_json::from_str(&crate::files::read_string(manifest)?).map_err(|failed| {
            crate::Error::Daemon(format!("{} is no manifest: {failed}", manifest.display()))
        })?;
    let seating = Seating::from_manifest(&read["seating"])?;
    let name = read["scenario"]["name"].as_str().unwrap_or_default();
    let scenario = Scenario::named(name)?;
    let version = read["scenario"]["version"].as_u64().unwrap_or_default();
    if version != u64::from(scenario.version) {
        return Err(crate::Error::Daemon(format!(
            "{} ran {name} version {version}, and this source has version {}: a replay runs the same scenario",
            manifest.display(),
            scenario.version
        )));
    }
    let now = source();
    if read["source"]["commit"] != now["commit"] {
        log.say(format!(
            "the replay runs on {}, and the red ran on {}",
            now["commit"], read["source"]["commit"]
        ));
    }
    run(
        &Matrix {
            seating,
            scenario,
            stand_in,
            reports,
            owner,
            replay_of: read["run"].as_str().map(str::to_owned),
        },
        log,
    )
}

/// Where the steps stopped: over (and why, where it was ended), or at a step the app is to be
/// restarted before, by its index.
enum Stop {
    Over(Option<String>),
    Restart(usize),
}

/// Asks each step and runs the scenario's machine ([`Steps`]) on the board until the step is
/// reached: why the scenario was ended, where it was. A step that did not go in, or was not
/// reached, is a check that failed, and the steps after it are not asked.
fn steps(
    driver: &mut Driver,
    seating: &Seating,
    scenario: &Scenario,
    id: &str,
    checks: &mut Vec<Check>,
    (from, restarted): (usize, bool),
    said: &mut usize,
) -> Result<Stop> {
    let mut machine = Steps::new(scenario.deadline, scenario.turns, scenario.facts);
    for (index, step) in scenario.steps.iter().enumerate().skip(from) {
        if step.restarts && !(restarted && index == from) {
            return Ok(Stop::Restart(index));
        }
        if let Some(why) = driver.over_now()? {
            return Ok(Stop::Over(Some(why)));
        }
        let snapshot = Snapshot::of(driver.members)?;
        let bindings = Bindings::by(&snapshot, scenario.binds);
        let text = if seating.chief_is_stand_in() {
            format!("\n{}", (step.stand_in)(&bindings))
        } else {
            (step.asks)(&bindings)
        };
        let marker = scenario.marker(id, index);
        if let Some(human) = step.human {
            // The human's own act on the page: nothing is typed into the chief's window.
            driver
                .log
                .say(format!("the human, on the page: {}", step.name));
            human(driver.members)?;
        }
        let asked = if step.human.is_some() {
            None
        } else {
            Some(driver.asks(step.name, &marker, &text)?.clone())
        };
        if let Some(asked) = asked.filter(|asked| !asked.went_in()) {
            let why = asked
                .failed
                .unwrap_or_else(|| "it did not go in".to_owned());
            return Ok(Stop::Over(
                driver
                    .over_now()?
                    .or(Some(format!("ask {} did not go in: {why}", index + 1))),
            ));
        }
        let what = format!("step {} ({}) was reached", index + 1, step.name);
        loop {
            let snapshot = Snapshot::of(driver.members)?;
            let bindings = Bindings::by(&snapshot, scenario.binds);
            let question = driver.question(&snapshot)?;
            let seen = Observation {
                elapsed: driver.bounds.elapsed(),
                turns: snapshot.total_turns(driver.asks_in()),
                reached: (step.reached)(&snapshot, &bindings),
                ended: ended(&snapshot, &bindings, scenario.ends),
                question: question.clone(),
                settled: settled(&snapshot),
                progress: progress(&snapshot),
            };
            let seconds = seen.elapsed.as_secs();
            match machine.next(&seen) {
                Next::Wait => std::thread::sleep(LOOK),
                Next::Reached => {
                    checks.push(Check::passed("steps", &what, format!("at {seconds} s")));
                    break;
                }
                Next::Answer { answer, .. } => {
                    *said += 1;
                    if question.is_some_and(|question| question.dialog == Some(true)) {
                        driver
                            .log
                            .say(format!("the human answers the chief's dialog: {answer}"));
                        driver.answer_dialog(&snapshot, scenario.facts, &answer)?;
                    } else {
                        let marker = format!("[matrix {id} answer {}]", *said);
                        driver.asks("answer", &marker, &answer)?;
                    }
                }
                Next::Nudge => {
                    *said += 1;
                    let marker = format!("[matrix {id} nudge {}]", *said);
                    driver.asks("nudge", &marker, scenario.nudge)?;
                }
                Next::End(why) => {
                    checks.push(Check::failed("steps", &what, why.clone()));
                    return Ok(Stop::Over(Some(why)));
                }
            }
        }
    }
    Ok(Stop::Over(None))
}

/// A bound task that failed or was cancelled, but those named in `ends` (whose end is the
/// scenario's own doing): its name, number and state.
pub fn ended(
    snapshot: &Snapshot,
    bindings: &Bindings,
    ends: &[&str],
) -> Option<(String, i64, String)> {
    bindings
        .found
        .iter()
        .filter(|(name, _)| !ends.contains(name))
        .find_map(|(name, numbers)| {
            numbers.iter().find_map(|number| {
                let state = snapshot.task(*number)?["state"].as_str()?;
                ["failed", "cancelled"]
                    .contains(&state)
                    .then(|| ((*name).to_owned(), *number, state.to_owned()))
            })
        })
}

/// Whether the board is at rest: the chief idle, no window at work or starting, and nothing
/// waiting to be delivered into the chief's window.
pub fn settled(snapshot: &Snapshot) -> bool {
    let chief_idle = snapshot.lanes.iter().any(|lane| {
        lane["participant"]["handle"] == "chief" && lane["activity"]["state"] == "idle"
    });
    let at_work = snapshot.lanes.iter().any(|lane| {
        ["working", "starting"].contains(&lane["activity"]["state"].as_str().unwrap_or_default())
    });
    let waiting = snapshot.chief_inbox.iter().any(|message| {
        ["queued", "delivering", "gated"].contains(&message["state"].as_str().unwrap_or_default())
    });
    chief_idle && !at_work && !waiting
}

/// What the board has done so far, as one text: each task's state, and how many messages were
/// delivered. A window's activity alone does not change it.
pub fn progress(snapshot: &Snapshot) -> String {
    let tasks: Vec<String> = snapshot
        .tasks
        .iter()
        .map(|task| {
            format!(
                "{}:{}",
                task["number"],
                task["state"].as_str().unwrap_or_default()
            )
        })
        .collect();
    let delivered = snapshot
        .events
        .iter()
        .filter(|event| event["kind"] == "delivery.confirmed")
        .count();
    format!("{} delivered {delivered}", tasks.join(","))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in board ends a scenario at three minutes, and one that waits by design (minutes
    /// of wake-up calls) at three and what it waits; never past the scenario's own deadline.
    #[test]
    fn a_stand_in_deadline_is_three_minutes_and_what_its_scenario_waits_by_design() {
        let plain = Scenario::named("smoke").unwrap();
        assert_eq!(stand_in_deadline(&plain), STAND_IN_DEADLINE);
        let waiting = plain.clone().waiting(Duration::from_secs(360));
        assert_eq!(stand_in_deadline(&waiting), Duration::from_secs(540));
        let mut short = waiting;
        short.deadline = Duration::from_secs(200);
        assert_eq!(stand_in_deadline(&short), Duration::from_secs(200));
        assert_eq!(
            stand_in_deadline(&Scenario::named("field5-nudges").unwrap()),
            Duration::from_secs(540)
        );
    }

    const KNOWN: &[Known] = &[
        Known {
            who: "D",
            what: "it is cancelled",
            until: "T-1122 (I-7)",
        },
        Known {
            who: "F",
            what: "it is open",
            until: "T-1118 (I-7)",
        },
    ];

    #[test]
    fn a_known_red_is_said_apart_never_passed_and_a_red_of_its_own_once_it_holds_or_goes_unmade() {
        let (kept, known) = sort_known(
            vec![
                Check::passed("D", "it is done", "T-1 is done"),
                Check::failed("D", "it is cancelled", "T-1 is done"),
                Check::failed("F", "it is open", "T-2 is done"),
            ],
            KNOWN,
        );
        assert_eq!(kept, [Check::passed("D", "it is done", "T-1 is done")]);
        assert_eq!(known.len(), 2);
        assert!(known.iter().all(|check| !check.ok));
        assert!(
            known[0]
                .detail
                .starts_with("known red until T-1122 (I-7): T-1 is done"),
            "{}",
            known[0].detail
        );

        // Once the work lands, the known red holds: its mark is to go, and it is a red until it does.
        let (kept, known) = sort_known(
            vec![
                Check::passed("D", "it is cancelled", "T-1 is cancelled"),
                Check::failed("F", "it is open", "x"),
            ],
            KNOWN,
        );
        assert_eq!(known.len(), 1);
        assert!(
            !kept[0].ok
                && kept[0]
                    .detail
                    .starts_with("a known red now holds (T-1122 (I-7)): take its mark out"),
            "{:?}",
            kept[0]
        );

        // A known red no check made is a red too: a renamed check cannot hide it.
        let (kept, known) = sort_known(vec![Check::failed("D", "it is cancelled", "x")], KNOWN);
        assert_eq!(known.len(), 1);
        assert_eq!(
            kept,
            [Check::failed(
                "F",
                "it is open",
                "a known red no check made (until T-1118 (I-7))"
            )]
        );
    }
}
