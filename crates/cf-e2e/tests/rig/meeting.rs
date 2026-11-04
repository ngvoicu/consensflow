//! A meeting with the chief end to end through the real daemon and pane host (3.0.7
//! chief-meeting, CMT-17): the human starts it through the page's operation, and
//! what the board has for the chief waits, queued, untyped and unread by the chief's
//! `cf inbox`; the Stop lets it go, a member's question first. The app's quit ends a
//! meeting, and what it held goes in after the human's Resume.

use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::approval::quit_and_start_again;
use crate::decided_result::{cf, results};
use crate::{rig, secs, Outcome};

/// What the chief's `cf inbox` lists a held message with in place of its text.
const HELD: &str = "(this waits until your meeting with the human ends)";

/// The roster of the case: the chief, and two workers on the stand-in, in one tier.
fn write_roster(rig: &Rig) -> cf_e2e::Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "worker", "kind": "claude-code", "model": "fake" },
            { "id": "worker-b", "kind": "claude-code", "model": "fake" },
        ],
    });
    rig.stand_ins(&roster)
}

/// The human starts (`on`) or stops a meeting, as the page does.
fn meeting(rig: &Rig, project: i64, on: bool) -> cf_e2e::Result<Value> {
    let said = rig.page("project.meeting", json!({ "project": project, "on": on }))?;
    assert_eq!(said["ok"], true, "{said}");
    Ok(said)
}

/// The daemon's events of `kind` for the message `id`.
fn of(events: &[Value], kind: &str, id: &Value) -> Vec<Value> {
    events
        .iter()
        .filter(|event| event["kind"] == kind && &event["data"]["message"] == id)
        .cloned()
        .collect()
}

/// Where in the daemon's events the delivery of `id` began.
fn begun_at(rig: &Rig, id: &Value) -> usize {
    rig.events()
        .iter()
        .position(|event| event["kind"] == "delivery.begun" && &event["data"]["message"] == id)
        .unwrap_or(usize::MAX)
}

/// The question in the chief's inbox, if there is one.
fn question(project: &Project) -> cf_e2e::Result<Option<Value>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .find(|message| message["kind"] == "question"))
}

#[test]
fn a_meeting_started_on_the_page_holds_the_chiefs_messages_and_its_stop_lets_them_go_question_first(
) -> Outcome {
    let rig: Rig = rig()?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    let first = project.add_member("worker")?;
    project.add_member("worker-b")?;
    let tier = first["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let chief = rig.open_frame(&format!("p{id}-chief"), OPEN)?;
    let started = meeting(&rig, id, true)?;
    assert!(started["project"]["meeting"].is_string(), "{started}");

    // The human talks with the chief, which hands out two tasks: one ends in a result,
    // the other in a question to the chief.
    rig.tell(
        id,
        &format!(
            "DISPATCH --tier {tier} Reply with exactly: ONE_DONE\nDISPATCH --tier {tier} CF ask :: Which format?"
        ),
    )?;
    rig.wait_for("a result and a question for the chief", secs(120), || {
        Ok(results(&project)?.len() == 1 && question(&project)?.is_some())
    })?;
    std::thread::sleep(secs(5));
    let result = results(&project)?.remove(0);
    let asked = question(&project)?.unwrap_or(Value::Null);
    for held in [&result, &asked] {
        assert_eq!(held["state"], "queued", "{held}");
        assert!(of(&rig.events(), "delivery.begun", &held["id"]).is_empty());
    }
    let board = project.board()?;
    let listed: Vec<Value> = board["held"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|message| message["id"].clone())
        .collect();
    assert_eq!(listed, vec![asked["id"].clone(), result["id"].clone()]);
    let inbox = cf(&rig, &chief, &["inbox"])?;
    assert!(inbox.contains(HELD), "{inbox}");
    assert!(!inbox.contains("ONE_DONE"), "{inbox}");
    assert!(!inbox.contains("Which format?"), "{inbox}");
    // `cf task get`, the read that left a held result out and said nothing (T-942), puts a
    // line in its place: under its message's own line, and nothing else of the message (the
    // brief above it, which shares the words, is the brief's own).
    for held in [&result, &asked] {
        let thread = cf(
            &rig,
            &chief,
            &["task", "get", &format!("T-{}", held["taskNumber"])],
        )?;
        let line = format!(
            "(m-{} waits until your meeting with the human ends: it comes to you then)",
            held["id"]
        );
        let at = thread
            .find(&format!("m-{} [queued]", held["id"]))
            .unwrap_or_else(|| panic!("m-{} is not in the thread: {thread}", held["id"]));
        let under: Vec<&str> = thread[at..]
            .lines()
            .skip(1)
            .take_while(|line| !line.is_empty())
            .collect();
        assert_eq!(under, [line.as_str()], "{thread}");
    }

    meeting(&rig, id, false)?;
    rig.wait_for("both to be delivered", secs(120), || {
        let result = results(&project)?.remove(0);
        let asked = question(&project)?.unwrap_or(Value::Null);
        Ok(result["state"] == "delivered" && asked["state"] == "delivered")
    })?;
    assert!(
        begun_at(&rig, &asked["id"]) < begun_at(&rig, &result["id"]),
        "the question first"
    );
    for message in [&result, &asked] {
        assert_eq!(of(&rig.events(), "delivery.begun", &message["id"]).len(), 1);
    }
    rig.close()?;
    Ok(())
}

#[test]
fn the_apps_quit_ends_a_meeting_and_what_it_held_goes_in_after_the_resume() -> Outcome {
    let rig: Rig = rig()?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    let added = project.add_member("worker")?;
    rig.open_frame(&format!("p{id}-chief"), OPEN)?;
    meeting(&rig, id, true)?;
    rig.tell(
        id,
        &format!(
            "DISPATCH --tier {} Reply with exactly: ONE_DONE",
            added["member"]["tier"].as_str().unwrap_or_default()
        ),
    )?;
    rig.wait_for("the result for the chief", secs(120), || {
        Ok(results(&project)?.len() == 1)
    })?;
    std::thread::sleep(secs(3));
    assert_eq!(results(&project)?[0]["state"], "queued");

    let rig = quit_and_start_again(rig)?;
    let project = Project::new(&rig, id);
    let board = project.board()?;
    assert_eq!(board["project"]["state"], "suspended");
    assert!(board["project"].get("meeting").is_none(), "{board}");
    let ended: Vec<Value> = rig
        .events()
        .into_iter()
        .filter(|event| event["kind"] == "project.meeting")
        .map(|event| event["data"].clone())
        .collect();
    assert_eq!(
        ended.last(),
        Some(&json!({ "on": false, "ended": "quit", "held": 1 }))
    );
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    rig.wait_for("the result to be delivered", secs(120), || {
        Ok(results(&project)?[0]["state"] == "delivered")
    })?;
    rig.close()?;
    Ok(())
}
