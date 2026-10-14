//! A project the human renames and writes a line under, end to end through the
//! real daemon and pane host (`npm run test:integration`), its chief the
//! stand-in `claude`: both changes are in the daemon's event file, what they were
//! and what they became; the project's folder is the one it was opened in; and no
//! window is told of either. The chief that is open is given nothing by them, and
//! its next window, once the human has closed the project and resumed it, has
//! neither the line nor the new name on its command line, in its environment or
//! in its role text.

use std::fs;

use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::{after, rig, secs, session_of, Outcome};

/// What the human calls the project, in place of the name its folder gave it.
const NAME: &str = "Quayside";
/// What the human says of it, for their eyes alone.
const LINE: &str = "Ships the quay's cargo, for the owner's eyes alone";

/// The user's messages a conversation was given, oldest first, as the stand-in
/// recorded them.
fn given(rig: &Rig, session: &str) -> Vec<String> {
    rig.transcript(session)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["type"] == "user")
        .filter_map(|record| record["message"]["content"].as_str().map(str::to_owned))
        .collect()
}

/// The windows opened for the chief of `project`, oldest first.
fn chief_windows(rig: &Rig, project: &Project) -> Vec<Value> {
    let id = format!("p{}-chief", project.id());
    rig.open_frames()
        .into_iter()
        .filter(|frame| frame["id"] == id.as_str())
        .collect()
}

/// Waits until the chief's turn is over.
fn until_the_chief_is_idle(rig: &Rig, project: &Project) -> cf_e2e::Result<()> {
    rig.wait_for("the chief to be idle", secs(60), || {
        Ok(project.lane("chief")?["activity"]["state"] == "idle")
    })
}

#[test]
fn a_project_the_human_renames_and_describes_is_in_the_event_file_and_no_window_is_told_of_either(
) -> Outcome {
    let rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    let first = rig.open_frame(&format!("p{id}-chief"), OPEN)?;
    let session = session_of(&first);
    until_the_chief_is_idle(&rig, &project)?;
    let told = given(&rig, &session);
    let opened = rig.page("projects.list", json!({}))?["projects"][0].clone();
    assert_ne!(opened["name"], NAME);

    // The human renames it, and writes the line under its name.
    let renamed = rig.page("project.rename", json!({ "project": id, "name": NAME }))?;
    assert_eq!(renamed["ok"], true, "{renamed}");
    let described = rig.page(
        "project.describe",
        json!({ "project": id, "description": LINE }),
    )?;
    assert_eq!(described["ok"], true, "{described}");
    let listed = rig.page("projects.list", json!({}))?["projects"][0].clone();
    assert_eq!(listed["name"], NAME);
    assert_eq!(listed["description"], LINE);
    assert_eq!(
        listed["directory"], opened["directory"],
        "its folder is the one it was opened in"
    );

    // Each is in the event file, with what it was and what it became.
    let logged = |kind: &str| {
        rig.events()
            .into_iter()
            .find(|event| event["kind"] == kind && event["project"] == id)
            .map(|event| event["data"].clone())
    };
    rig.wait_for("both events to be in the event file", secs(10), || {
        Ok(logged("project.renamed").is_some() && logged("project.described").is_some())
    })?;
    assert_eq!(
        logged("project.renamed"),
        Some(json!({ "from": opened["name"], "to": NAME }))
    );
    assert_eq!(
        logged("project.described"),
        Some(json!({ "from": null, "to": LINE }))
    );

    // The dispatcher was woken by both, and the chief is given nothing.
    until_the_chief_is_idle(&rig, &project)?;
    std::thread::sleep(secs(2));
    assert_eq!(given(&rig, &session), told, "no message came of either");

    // Its next window has neither on its command line, in its environment (the
    // frame it was opened with) or in its role text.
    let closed = rig.page("project.close", json!({ "project": id }))?;
    assert_eq!(closed["ok"], true, "{closed}");
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    rig.wait_for("the chief's next window", secs(60), || {
        Ok(chief_windows(&rig, &project).len() >= 2)
    })?;
    let next = chief_windows(&rig, &project).remove(1);
    let role = fs::read_to_string(after(&next, "--append-system-prompt-file"))?;
    assert!(!role.is_empty(), "the role text of the window was read");
    let launch = format!("{next}\n{role}");
    for words in [LINE, NAME] {
        assert!(
            !launch.contains(words),
            "a window is told {words:?}: {launch}"
        );
    }
    rig.close()?;
    Ok(())
}
