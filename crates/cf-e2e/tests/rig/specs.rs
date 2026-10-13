//! A chief plans big work as specs where its project has them on, end to end
//! through the real daemon and pane host, the windows the stand-in `claude`:
//! the human says it on the page (`project.specs`) or for every project (the
//! agents file's preference), and the chief's next launch carries the Specmint
//! skill, written beside its role in the launch's folder (which Claude is given)
//! and named in its role text; a project that has them off, and every worker,
//! are given nothing, and a skill Claude has installed itself is the one used.

use std::fs;
use std::path::{Path, PathBuf};

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::{after, rig, secs, Outcome};

/// What the tdd variant ships beyond its `SKILL.md`, which the copy must hold.
const TDD_REFERENCE: &str = "references/testing-knowledge.md";

/// The windows opened for the chief of `project`, oldest first.
fn chief_windows(rig: &Rig, project: &Project) -> Vec<Value> {
    let id = format!("p{}-chief", project.id());
    rig.open_frames()
        .into_iter()
        .filter(|frame| frame["id"] == id.as_str())
        .collect()
}

/// The human closes the project and resumes it, and the chief's next window is
/// waited for: `windows` of them by then.
fn relaunch(rig: &Rig, project: &Project, windows: usize) -> Result<Value, cf_e2e::Error> {
    let id = project.id();
    let closed = rig.page("project.close", json!({ "project": id }))?;
    assert_eq!(closed["ok"], true, "{closed}");
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    rig.wait_for("the chief's next window", secs(60), || {
        Ok(chief_windows(rig, project).len() >= windows)
    })?;
    Ok(chief_windows(rig, project).remove(windows - 1))
}

/// The folder Claude is given for a window's role (`--add-dir`): its skills are
/// under `.claude/skills`.
fn skills_of(frame: &Value) -> PathBuf {
    Path::new(&after(frame, "--add-dir"))
        .join(".claude")
        .join("skills")
}

/// The names of the skills a window's role folder holds.
fn skills_named(frame: &Value) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(skills_of(frame))
        .map(|entries| {
            entries
                .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The role text a window was given: the file Claude's system prompt appends.
fn role_of(frame: &Value) -> String {
    fs::read_to_string(after(frame, "--append-system-prompt-file")).unwrap_or_default()
}

/// Whether a role text tells of specs: its section of them, or the skill. (Not
/// the word: the text names the folder of this checkout's `cf`.)
fn told_of_specs(role: &str) -> bool {
    role.contains("This project's specs") || role.contains("Specmint")
}

/// The agents file with the human's preference that specs are on for every project.
fn turn_specs_on_for_every_project(rig: &Rig) -> Outcome {
    let file = rig.home().join("agents.json");
    let mut roster: Value = serde_json::from_str(&files::read_string(&file)?)?;
    roster["preferences"] = json!({ "ownHarnessOnly": false, "specs": true });
    files::write(&file, format!("{roster}\n"))?;
    Ok(())
}

#[test]
fn a_chief_of_a_project_with_specs_on_is_given_the_skill_beside_its_role_at_its_next_launch_and_a_worker_never_is(
) -> Outcome {
    let rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let first = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    // With specs off, its role is alone and says nothing of them.
    assert_eq!(skills_named(&first), ["consensflow-chief"]);
    assert!(!role_of(&first).contains("This project's specs"));

    // The human says it; the chief's window as it was.
    let said = rig.page(
        "project.specs",
        json!({ "project": project.id(), "on": true, "folder": "plans", "variant": "tdd" }),
    )?;
    assert_eq!(said["ok"], true, "{said}");
    assert_eq!(
        said["project"]["specs"],
        json!({ "on": true, "folder": "plans", "variant": "tdd" })
    );
    assert_eq!(chief_windows(&rig, &project).len(), 1);

    // Its next launch has the skill, beside its role, and names it.
    let next = relaunch(&rig, &project, 2)?;
    assert_eq!(
        skills_named(&next),
        ["consensflow-chief", "specmint-tdd"],
        "{:?}",
        next["argv"]
    );
    let skill = skills_of(&next).join("specmint-tdd");
    let installed = fs::read_to_string(skill.join("SKILL.md"))?;
    assert!(
        installed.starts_with("---\nname: specmint-tdd\n"),
        "{installed}"
    );
    assert!(
        skill.join(TDD_REFERENCE).is_file(),
        "the references the skill names are beside it"
    );
    let role = role_of(&next);
    assert!(role.contains("## This project's specs"), "{role}");
    assert!(role.contains("`specmint-tdd`"), "{role}");
    assert!(
        role.contains("Where Specmint says `.specs`, it means `plans`"),
        "{role}"
    );
    assert!(
        role.contains(&skill.join("SKILL.md").to_string_lossy().replace('\\', "/")),
        "the role names the file that is there: {role}"
    );

    // A worker's launch is given none of it. Its task keeps its window at work for
    // some seconds, which is as long as the launch's files are there to be read:
    // they go with the window.
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP 8 Reply with exactly: WORKER_OK",
            added["member"]["tier"].as_str().unwrap_or_default()
        ),
    )?;
    let mut worker = None;
    rig.wait_for("the worker's window to be opened", secs(60), || {
        let prefix = format!("p{}-worker", project.id());
        worker = rig.open_frames().into_iter().find(|frame| {
            frame["id"]
                .as_str()
                .is_some_and(|id| id.starts_with(&prefix))
        });
        Ok(worker.is_some())
    })?;
    let worker = worker.expect("the worker's window");
    assert_eq!(
        skills_named(&worker),
        ["consensflow-worker"],
        "{:?}",
        worker["argv"]
    );
    assert!(
        !told_of_specs(&role_of(&worker)),
        "a member is told nothing of them"
    );
    assert!(
        !worker["argv"].to_string().contains("specmint"),
        "nothing of Specmint on its command line: {}",
        worker["argv"]
    );
    rig.wait_for("the worker's task to be done", secs(60), || {
        Ok(project.lane("worker")?["tasks"][0]["state"] == "done")
    })?;
    rig.close()?;
    Ok(())
}

#[test]
fn with_specs_on_for_every_project_a_project_that_says_nothing_follows_and_one_that_says_off_is_given_nothing(
) -> Outcome {
    let rig = rig()?;
    turn_specs_on_for_every_project(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let first = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    // It says nothing of its own: it follows the human's choice for every project.
    assert_eq!(skills_named(&first), ["consensflow-chief", "specmint-core"]);
    assert!(
        role_of(&first).contains("it means `.specs`"),
        "the documented folder"
    );

    // It says off: the choice for every project does not reach it.
    let said = rig.page(
        "project.specs",
        json!({ "project": project.id(), "on": false }),
    )?;
    assert_eq!(said["ok"], true, "{said}");
    let next = relaunch(&rig, &project, 2)?;
    assert_eq!(
        skills_named(&next),
        ["consensflow-chief"],
        "{:?}",
        next["argv"]
    );
    assert!(!told_of_specs(&role_of(&next)), "told nothing of them");
    rig.close()?;
    Ok(())
}

#[test]
fn a_variant_claude_has_installed_itself_is_the_one_the_chief_is_told_of_and_nothing_is_copied_beside_its_role(
) -> Outcome {
    let rig = rig()?;
    let installed = Path::new(rig.var("CLAUDE_CONFIG_DIR").unwrap_or_default())
        .join("skills")
        .join("specmint-core-html");
    fs::create_dir_all(&installed)?;
    fs::write(
        installed.join("SKILL.md"),
        "---\nname: specmint-core-html\n---\nthe human's own copy\n",
    )?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let said = rig.page(
        "project.specs",
        json!({ "project": project.id(), "on": true, "variant": "core-html" }),
    )?;
    assert_eq!(said["ok"], true, "{said}");
    let next = relaunch(&rig, &project, 2)?;
    assert_eq!(
        skills_named(&next),
        ["consensflow-chief"],
        "nothing is copied"
    );
    let role = role_of(&next);
    assert!(
        role.contains(
            &installed
                .join("SKILL.md")
                .to_string_lossy()
                .replace('\\', "/")
        ),
        "the role names the installed file: {role}"
    );
    assert_eq!(
        fs::read_to_string(installed.join("SKILL.md"))?,
        "---\nname: specmint-core-html\n---\nthe human's own copy\n",
        "the human's own is as it was"
    );
    // Another variant is chosen: the installed one is no copy of it, and is left alone.
    let said = rig.page(
        "project.specs",
        json!({ "project": project.id(), "on": true, "variant": "tdd-html" }),
    )?;
    assert_eq!(said["ok"], true, "{said}");
    let third = relaunch(&rig, &project, 3)?;
    assert_eq!(
        skills_named(&third),
        ["consensflow-chief", "specmint-tdd-html"]
    );
    assert!(installed.join("SKILL.md").is_file());
    rig.close()?;
    Ok(())
}
