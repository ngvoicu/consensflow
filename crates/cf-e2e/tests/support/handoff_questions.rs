//! Shared stand-in chief-switch fixtures for the regressions and the offline matrix.

use cf_e2e::rig::{chief_body, Config, Project, Rig};
use cf_e2e::Outcome;
use serde_json::{json, Value};
use std::error::Error;
use std::time::Duration;

fn config() -> Config {
    Config::new(env!("CARGO_BIN_EXE_fake-agent")).judged_by_outcome()
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

fn windows_of(rig: &Rig, project: &Project) -> Vec<Value> {
    let id = format!("p{}-chief", project.id());
    rig.open_frames()
        .into_iter()
        .filter(|frame| frame["id"] == id.as_str())
        .collect()
}

fn after<'a>(window: &'a Value, flag: &str) -> Option<&'a str> {
    let argv = window["argv"].as_array()?;
    let at = argv.iter().position(|word| word == flag)?;
    argv.get(at + 1)?.as_str()
}

fn wait_for_the_chief(rig: &Rig, project: &Project) -> cf_e2e::Result<()> {
    rig.wait_for("the chief's window to be idle", secs(60), || {
        Ok(project.lane("chief")?["activity"]["state"] == "idle")
    })
}

/// What the worker puts to the chief, on the task it was given.
const QUESTION: &str = "Which grammar?";

/// A board with T-1 given to a stand-in worker, which asked the chief `QUESTION`
/// through `cf ask`, and the chief was given it. The project, the question's id and the
/// worker's session.
pub(crate) fn asked(rig: &Rig) -> Result<(Project<'_>, i64, String), Box<dyn Error>> {
    let project = Project::open(rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    wait_for_the_chief(rig, &project)?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} CF ask :: {QUESTION}",
            added["member"]["tier"].as_str().unwrap_or_default()
        ),
    )?;
    rig.wait_for("the question to be given to the chief", secs(60), || {
        Ok(project
            .inbox("chief")?
            .iter()
            .any(|m| m["kind"] == "question" && m["state"] == "delivered"))
    })?;
    let question = project
        .inbox("chief")?
        .into_iter()
        .find(|m| m["kind"] == "question")
        .ok_or("no question in the chief's inbox")?;
    let session = project.lane("worker")?["participant"]["handle"]
        .as_str()
        .ok_or("the worker has no session")?
        .to_owned();
    assert_eq!(question["taskNumber"], 1, "{question}");
    assert_eq!(question["sender"], session.as_str(), "{question}");
    let id = question["id"].as_i64().ok_or("the question has no id")?;
    Ok((project, id, session))
}

/// The human switches the chief to the staff's other agent, and the new chief's window
/// has been told it is the chief: that message, as its record holds it.
pub(crate) fn handoff_after_a_switch(
    rig: &Rig,
    project: &Project,
) -> Result<String, Box<dyn Error>> {
    wait_for_the_chief(rig, project)?;
    let switched = rig.page(
        "chief.switch",
        chief_body(rig, "worker", json!({ "project": project.id() }))?,
    )?;
    assert_eq!(switched["ok"], true, "{switched}");
    rig.wait_for("a second chief window to be opened", secs(60), || {
        Ok(windows_of(rig, project).len() == 2)
    })?;
    let second = windows_of(rig, project).remove(1);
    let session = after(&second, "--session-id")
        .ok_or("the new chief's window has no session")?
        .to_owned();
    rig.wait_for("the new chief to be told it is the chief", secs(60), || {
        Ok(rig.transcript(&session).contains("You are the chief now"))
    })?;
    Ok(rig.transcript(&session))
}

/// The field question was recorded before holding tracking. Exercise that path
/// and the current tracked path with the same handoff assertions.
pub(crate) fn cancelled_question_handoff(tracked: bool) -> Outcome {
    let mut rig = Rig::start(config())?;
    let (project, question, session) = asked(&rig)?;
    let project_id = project.id();
    if !tracked {
        let file = rig.home().join("consensflow.db");
        rig.kill_daemon();
        rig.wait_for_daemon_exit(secs(10))?;
        let root = rig.close_keeping_the_root()?;
        {
            let db = cf_e2e::door::database::write(file)?;
            assert_eq!(
                db.execute(
                    "DELETE FROM question_binding WHERE question_id = ?",
                    [question]
                )?,
                1,
                "the delivered question becomes a pretracking fixture"
            );
        }
        rig = Rig::start(config().existing_root(root))?;
        let resumed = rig.page("project.resume", json!({ "project": project_id }))?;
        assert_eq!(resumed["ok"], true, "{resumed}");
    }
    let project = Project::new(&rig, project_id);
    wait_for_the_chief(&rig, &project)?;
    let chief = windows_of(&rig, &project)
        .into_iter()
        .next()
        .ok_or("the chief has no window")?;

    // The worker's window is at work on a turn of the human's, so what is put to it
    // waits in its queue.
    let frame = rig.open_frame(&format!("p{}-{session}", project.id()), secs(30))?;
    let typed = rig.host(
        "pane.input",
        json!({
            "id": frame["id"],
            "generation": frame["generation"],
            "bytes": "SLEEP 90 resting\r".as_bytes(),
        }),
    )?;
    assert_eq!(typed["ok"], true, "{typed}");
    rig.wait_for("the worker's window to be at work", secs(30), || {
        Ok(project.lane(&session)?["activity"]["state"] != "idle")
    })?;

    // The chief answers and cancels at once, as the field's chief did.
    let answered = rig.cf_in_window(
        &chief,
        ["answer", &format!("m-{question}"), "Use the new grammar"],
    )?;
    assert_eq!(answered.code, Some(0), "{answered}");
    let cancelled = rig.cf_in_window(&chief, ["task", "cancel", "T-1"])?;
    assert_eq!(cancelled.code, Some(0), "{cancelled}");

    // The old question remains delivered; a tracked question is withdrawn and
    // its unreceived answer returned. The answer and task are cancelled in both.
    let answer = project
        .inbox(&session)?
        .into_iter()
        .find(|m| m["kind"] == "answer")
        .ok_or("the chief's answer is in nobody's inbox")?;
    assert_eq!(
        answer["state"], "cancelled",
        "the setup needs the answer taken back by the cancel: {answer}"
    );
    assert_eq!(project.task(1)?["state"], "cancelled");
    let asked = project
        .inbox("chief")?
        .into_iter()
        .find(|m| m["id"] == question)
        .unwrap_or(Value::Null);
    assert_eq!(
        asked["state"],
        if tracked { "cancelled" } else { "delivered" },
        "{asked}"
    );
    let returned = project
        .inbox("chief")?
        .into_iter()
        .filter(|m| {
            m["kind"] == "note"
                && m["body"]
                    .as_str()
                    .is_some_and(|body| body.contains("Use the new grammar"))
        })
        .count();
    assert_eq!(
        returned,
        usize::from(tracked),
        "the tracked answer returns once"
    );

    let handoff = handoff_after_a_switch(&rig, &project)?;
    assert!(
        handoff.contains("Nothing on the board waits on you."),
        "the handoff says nothing waits: {handoff}"
    );
    assert!(
        !cf_e2e::rig::names_command(&handoff, &format!("answer m-{question}")),
        "the new chief is not sent to answer m-{question}: {handoff}"
    );
    rig.close()?;
    Ok(())
}
