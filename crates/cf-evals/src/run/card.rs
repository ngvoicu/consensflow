//! The card and nocard arms: a chief and a staff of real harnesses on the native
//! daemon, in a project the run opens as the New project dialog does, a scripted
//! owner typing into the chief's terminal, and the ledger measured at the end.
//!
//! Everything is real but the human. The rig starts the daemon and the pane host as
//! the app starts them, and the harnesses run in the pane host's windows on the
//! machine's own logins ([`super::machine`]).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::{agents, ask, text, Log};
use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use super::chief::{Chief, IDLE, READY};
use super::floor::Floor;
use super::owner::{Owner, Seen, Step, KEY_PAUSE, QUESTION_PAUSE};
use super::report::{file_name, keep, Card, Refused, Said};
use super::workspace;
use super::{start, Plan};
use crate::measure::{
    begun, chief_open_question, devin_chief_questions, measure, mechanics, on_windows,
    read as read_file, Check, FileRead, Options as Measuring,
};
use crate::plan::{
    dialog_answers, last_lines, terminal_answer, Agent, DialogAnswer, DialogQuestion,
};
use crate::scenarios::FollowUp;
use crate::{js, Error, Result};

/// How often the owner looks at the project.
const LOOK: Duration = Duration::from_secs(5);

/// How a text is typed into a window: as it is, or as that window's console takes it.
type Spell = Box<dyn Fn(&str) -> String>;

/// What the board and the owner did while the run went, which the report needs once
/// it is over: the chief's window, how many tasks the board listed at the last look,
/// the owner's approvals at the gate, and what the files the scenario reads held as
/// the run began and when the first worker task's result was in.
#[derive(Default)]
struct Scene {
    pane: Option<Value>,
    board_tasks: i64,
    approvals: i64,
    refused: Vec<Refused>,
    said: Said,
    read: BTreeMap<String, FileRead>,
    read_at_result: bool,
}

/// Plays the scenario once, as run number `index`: whether it held.
pub fn play(plan: &Plan, index: u32, out: &mut dyn Write) -> Result<bool> {
    let (options, scenario) = (plan.options, &plan.options.scenario);
    let mut floor = Floor::new(out);
    let workspace = workspace::fresh(plan.machine, scenario)?;
    let rig = start(plan, &mut floor, &workspace)?;
    let mut scene = Scene {
        read: begun(&workspace, &scenario.reads())?,
        ..Scene::default()
    };
    let conducted = conduct(plan, &rig, &workspace, &mut floor, &mut scene);
    let screen = scene
        .pane
        .as_ref()
        .map(|pane| last_lines(&rig.screen(pane["id"].as_str().unwrap_or_default()), 40))
        .unwrap_or_default();
    let file = rig.home().join("consensflow.db");
    // The home is kept: the ledger in it is what is measured, and the daemon holds it
    // until it is closed.
    let closed = rig.close_keeping_the_root();
    conducted?;
    closed?;

    let fixture = workspace::fixture(scenario.fixture);
    let metrics = measure(
        &file,
        &Measuring {
            files: Some((&fixture, &workspace)),
            pickers: scene.said.dialogs,
            read: Some(&scene.read),
            gone: scenario.counts_gone_files(),
        },
    )?;
    let checks = scenario.verdict(&metrics);
    let mut board = mechanics(&metrics.plumbing, scene.board_tasks);
    if options.gate {
        board.push(Check {
            name: format!(
                "the gate held the messages for the owner's approval ({} approved)",
                scene.approvals
            ),
            ok: scene.approvals > 0 && scene.refused.is_empty(),
        });
    }
    let mut kept = Card {
        options,
        scenario,
        chief: plan.chief,
        staff: &plan.staff,
        index,
        seconds: floor.seconds(),
        metrics,
        checks,
        mechanics: board,
        screen,
        said: scene.said,
        refused: scene.refused,
        approvals: scene.approvals,
        log: floor.log(),
        path: PathBuf::new(),
    };
    let name = file_name(jiff::Timestamp::now(), options, index);
    kept.path = keep(&options.reports, &name, &kept.value())?;
    kept.say(&mut floor)?;
    Ok(kept.held())
}

/// The run itself: the project is opened, the prompt typed, and the owner looks at
/// the project every few seconds until it is over.
fn conduct(
    plan: &Plan,
    rig: &Rig,
    workspace: &Path,
    floor: &mut Floor,
    scene: &mut Scene,
) -> Result {
    let (options, scenario) = (plan.options, &plan.options.scenario);
    let mut rows = vec![plan.agent.to_value()];
    rows.extend(plan.staff.agents.iter().map(Agent::to_value));
    // The eval's report says the chief's and the staff's models itself: the log is
    // no live run's, whose report the launches would be.
    agents::write_roster(rig, &rows, &Log::new("eval"))?;
    let file = rig.home().join("consensflow.db");
    let mut request = json!({
        "directory": workspace,
        "agent": plan.agent.id,
        "staff": plan.staff.to_value(),
    });
    if options.gate {
        request["gate"] = json!(true);
    }
    let opened = rig.page("project.open", request)?;
    let Some(project) = opened["project"]["id"]
        .as_i64()
        .filter(|_| opened["ok"] == true)
    else {
        return Err(Error::Refused(format!("project.open: {opened}")));
    };
    let chief = Chief::new(rig, Project::new(rig, project));
    let spell = spelling(plan, rig)?;

    chief.waits_for(floor, "the chief to be ready", READY, || {
        Ok(chief.state()?.as_deref() == Some(IDLE))
    })?;
    let pane = chief.pane()?;
    scene.pane = Some(pane.clone());
    chief.settles(&pane);
    floor.note(format!(
        "chief ({}) ready; typing the prompt",
        options.chief
    ))?;
    chief.says(floor, &scenario.prompt)?;

    let mut owner = Owner::new(scenario);
    let mut last_change = Instant::now();
    let mut signature = String::new();
    loop {
        sleep(LOOK);
        approve_what_waits(rig, &chief, floor, scene)?;
        let current = chief.board()?;
        let lane = Chief::lane_of(&current);
        let tasks = tasks_of(&current);
        scene.board_tasks = i64::try_from(tasks.len()).unwrap_or(i64::MAX);
        let state = lane["activity"]["state"].as_str();
        let now = signature_of(&current, &lane, &tasks);
        if now != signature {
            signature = now;
            last_change = Instant::now();
            floor.note(format!(
                "chief {}; tasks {}",
                state.unwrap_or("undefined"),
                listed(&tasks)
            ))?;
        }
        let seen = Seen {
            elapsed: floor.elapsed(),
            quiet: last_change.elapsed(),
            busy: state != Some(IDLE)
                || tasks.iter().any(|task| {
                    matches!(
                        task["state"].as_str(),
                        Some("queued" | "working" | "waiting")
                    )
                }),
            dialog: state == Some("waiting"),
            board_empty: tasks.is_empty(),
        };
        let step = owner.step(scenario, &seen, options.timeout, &mut || {
            chief_open_question(&file)
        })?;
        keep_at_result(
            &mut scene.read,
            &mut scene.read_at_result,
            &step,
            &tasks,
            workspace,
        )?;
        match step {
            Step::Wait => {}
            Step::Dialog => {
                answer_dialog(plan, rig, &chief, &file, spell.as_ref(), floor)?;
                last_change = Instant::now();
            }
            Step::Reply(end) => {
                let reply = terminal_answer(scenario, &end.text);
                floor.note(format!(
                    "the chief asked in its terminal; the owner answered there: {reply}"
                ))?;
                chief.says(floor, &reply)?;
                last_change = Instant::now();
            }
            Step::Nudge(nudge) => {
                floor.note(format!(
                    "the chief stopped in its terminal; the owner typed there: {nudge}"
                ))?;
                chief.settles(&chief.pane()?);
                chief.says(floor, &nudge)?;
                last_change = Instant::now();
            }
            Step::Next(FollowUp::Switch) => {
                switch(plan, rig, &chief, project, floor, scene)?;
                last_change = Instant::now();
            }
            Step::Next(FollowUp::Say(next)) => {
                floor.note(format!(
                    "the owner's next message ({}): {}",
                    owner.follow_ups_sent,
                    js::first(&next, 80)
                ))?;
                chief.settles(&chief.pane()?);
                chief.says(floor, &next)?;
                last_change = Instant::now();
            }
            Step::Over => {
                floor.note("quiet: the run is over")?;
                break;
            }
            Step::TimeUp => {
                floor.note("time is up")?;
                still_working(&chief, &current, floor)?;
                break;
            }
        }
    }
    scene.said = Said {
        nudges: owner.nudges,
        replies: owner.replies,
        dialogs: owner.dialogs,
        follow_ups: owner.follow_ups_sent,
    };
    Ok(())
}

/// The tasks of a board that a lane has or that are open, as the owner counts them.
pub(crate) fn tasks_of(board: &Value) -> Vec<&Value> {
    board["lanes"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|lane| lane["tasks"].as_array().into_iter().flatten())
        .chain(board["open"].as_array().into_iter().flatten())
        .collect()
}

/// Whether the first worker task of a board, the lowest number that is no advice,
/// review, test or image, has its result in: done, or accepted already.
fn first_result_in(tasks: &[&Value]) -> bool {
    tasks
        .iter()
        .filter(|task| {
            !matches!(
                task["pool"].as_str(),
                Some("advisor" | "reviewer" | "tester" | "designer")
            )
        })
        .min_by_key(|task| task["number"].as_i64().unwrap_or(i64::MAX))
        .is_some_and(|task| matches!(task["state"].as_str(), Some("done" | "accepted")))
}

/// At a look at the board, whose tasks are `tasks`, where the owner's next step is
/// `step`: the files the scenario reads, as they are in `workspace`, kept in `read` as
/// the first worker task's result left them; once, and `kept` says it was. They are
/// read as the owner is about to say the next thing, which is the scenario's next step
/// (an undo, Sol 2026-10-15): nothing of that step is asked for yet, so nothing of it
/// can be in them. A look of the board alone would be behind a chief that went on at
/// once.
fn keep_at_result(
    read: &mut BTreeMap<String, FileRead>,
    kept: &mut bool,
    step: &Step,
    tasks: &[&Value],
    workspace: &Path,
) -> Result {
    if *kept || !matches!(step, Step::Next(FollowUp::Say(_))) || !first_result_in(tasks) {
        return Ok(());
    }
    for (file, seen) in read.iter_mut() {
        seen.at_result = read_file(workspace, file)?;
    }
    *kept = true;
    Ok(())
}

/// What of the board changes when the run is not quiet: the chief's state, the
/// tasks' and every window's.
pub(crate) fn signature_of(board: &Value, lane: &Value, tasks: &[&Value]) -> String {
    json!([
        lane["activity"]["state"],
        tasks
            .iter()
            .map(|task| json!([task["number"], task["state"]]))
            .collect::<Vec<_>>(),
        board["lanes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|lane| json!([lane["participant"]["handle"], lane["activity"]["state"]]))
            .collect::<Vec<_>>(),
    ])
    .to_string()
}

/// The tasks as the run says them as it goes: `T-1:working T-2:queued`, or `none`.
pub(crate) fn listed(tasks: &[&Value]) -> String {
    let said: Vec<String> = tasks
        .iter()
        .map(|task| format!("T-{}:{}", text(&task["number"]), text(&task["state"])))
        .collect();
    if said.is_empty() {
        "none".to_owned()
    } else {
        said.join(" ")
    }
}

/// With the gate on, the owner approves every message as the board's For you does.
fn approve_what_waits(rig: &Rig, chief: &Chief, floor: &mut Floor, scene: &mut Scene) -> Result {
    let board = chief.board()?;
    for waiting in board["gated"].as_array().into_iter().flatten() {
        let reply = rig.page("message.approve", json!({ "message": waiting["id"] }))?;
        if reply["ok"] == false {
            floor.note(format!(
                "approving m-{} was REFUSED: {}",
                text(&waiting["id"]),
                text(&reply["error"])
            ))?;
            scene.refused.push(Refused {
                message: waiting["id"].clone(),
                error: reply["error"].clone(),
            });
        } else {
            scene.approvals += 1;
        }
    }
    Ok(())
}

/// A chief's own question dialog holds its window, waiting for input: the owner
/// answers it there. A Devin chief's question the scenario has an answer for gets it
/// typed into the dialog's Other; any other, and any other harness's dialog, Enter:
/// its first option.
fn answer_dialog(
    plan: &Plan,
    rig: &Rig,
    chief: &Chief,
    file: &Path,
    spell: &dyn Fn(&str) -> String,
    floor: &mut Floor,
) -> Result {
    let pane = chief.pane()?;
    let tail = chief.last_lines(&pane, 40);
    floor.note(format!(
        "the chief's question dialog waits. Its screen: {}",
        tail[tail.len().saturating_sub(6)..].join(" ⏎ ")
    ))?;
    let questions = if plan.options.chief == "devin" {
        devin_chief_questions(file, rig.environment())?
    } else {
        None
    };
    let replies = match questions {
        None => vec![DialogAnswer {
            question: String::new(),
            answer: None,
            keys: vec![vec![13]],
        }],
        Some(questions) => {
            let asked: Vec<DialogQuestion> = questions.iter().map(DialogQuestion::of).collect();
            dialog_answers(&plan.options.scenario, &asked, spell)
        }
    };
    for DialogAnswer {
        question,
        answer,
        keys,
    } in replies
    {
        floor.note(match answer {
            None if question.is_empty() => "the owner took the first option there".to_owned(),
            None => format!("the owner took the first option there for \"{question}\""),
            Some(text) => format!("the owner answered \"{question}\" there: {text}"),
        })?;
        for bytes in keys {
            chief.press(&pane, &bytes)?;
            sleep(KEY_PAUSE);
        }
        sleep(QUESTION_PAUSE);
    }
    Ok(())
}

/// The owner switches the chief from its card; the new window takes the handoff
/// first, then the owner goes on in it.
fn switch(
    plan: &Plan,
    rig: &Rig,
    chief: &Chief,
    project: i64,
    floor: &mut Floor,
    scene: &mut Scene,
) -> Result {
    let target = plan.options.switch_to.as_deref().unwrap_or_default();
    let agent = format!("eval-{target}-worker");
    let before = chief.pane()?["generation"].clone();
    floor.note(format!(
        "the owner switches the chief to {target} ({agent})"
    ))?;
    let reply = rig.page(
        "chief.switch",
        json!({ "project": project, "agent": agent, "when": "turn" }),
    )?;
    if reply["ok"] == false {
        return Err(Error::Refused(format!("chief.switch: {reply}")));
    }
    chief.waits_for(floor, "the new chief's window", READY, || {
        let pane = chief.pane()?;
        Ok(!pane.is_null() && pane["generation"] != before)
    })?;
    let pane = chief.pane()?;
    scene.pane = Some(pane.clone());
    chief.settles(&pane);
    Ok(())
}

/// What the windows of the members that are still at work show when time is up: a
/// harness's record keeps only what it finished, and a member's turn that never ends
/// shows only on its screen.
fn still_working(chief: &Chief, board: &Value, floor: &mut Floor) -> Result {
    for lane in board["lanes"].as_array().into_iter().flatten() {
        let working = lane["activity"]["state"] == "working";
        if lane["participant"]["role"] == "chief" || !working || lane["pane"].is_null() {
            continue;
        }
        let shown = chief.last_lines(&lane["pane"], 40);
        floor.note(format!(
            "@{} still works. Its screen: {}",
            text(&lane["participant"]["handle"]),
            shown[shown.len().saturating_sub(20)..].join(" ⏎ ")
        ))?;
    }
    Ok(())
}

/// How the owner's answers are typed into a Devin chief's question dialog: Windows'
/// console drops what is not ASCII, so each answer the scenario can give goes in as
/// the console carries it (the daemon's own spelling, asked of the harness code).
/// Elsewhere, and for any other chief, as it is.
fn spelling(plan: &Plan, rig: &Rig) -> Result<Spell> {
    if plan.options.chief != "devin" || !on_windows(rig.environment()) {
        return Ok(Box::new(str::to_owned));
    }
    let mut spelled: Vec<(String, String)> = Vec::new();
    for answer in &plan.options.scenario.answers {
        spelled.push((answer.text.clone(), ask::console_text(&answer.text)?));
    }
    Ok(Box::new(move |text| {
        spelled
            .iter()
            .find(|(said, _)| said == text)
            .map_or_else(|| text.to_owned(), |(_, spelled)| spelled.clone())
    }))
}

#[cfg(test)]
mod tests;
