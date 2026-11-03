//! A chief plans big work as specs where its project has them on, end to end
//! through the real daemon and pane host, the windows the stand-in `claude`:
//! the human says it on the page (`project.specs`) or for every project (the
//! agents file's preference, which Settings sets with `specs.set`, with the
//! folder and the variant a project that names none uses), and the chief's next
//! launch carries the Specmint
//! skill, ConsensFlow's copy kept for every launch in a folder of its own
//! (which Claude is given besides its role's) and named in its role text; a
//! project that has them off, and every worker, are given nothing, and a skill
//! Claude has installed itself is the one used. (A chief on the stand-in Codex,
//! whose thread keeps what it began with, is in `specs_codex`.)

use std::fs;
use std::path::{Path, PathBuf};

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::{after, rig, secs, session_of, Outcome};

/// What the tdd variant ships beyond its `SKILL.md`, which the copy must hold.
const TDD_REFERENCE: &str = "references/testing-knowledge.md";

/// The windows opened for the chief of `project`, oldest first.
pub(crate) fn chief_windows(rig: &Rig, project: &Project) -> Vec<Value> {
    let id = format!("p{}-chief", project.id());
    rig.open_frames()
        .into_iter()
        .filter(|frame| frame["id"] == id.as_str())
        .collect()
}

/// The human closes the project and resumes it, and the chief's next window is
/// waited for: `windows` of them by then.
pub(crate) fn relaunch(
    rig: &Rig,
    project: &Project,
    windows: usize,
) -> Result<Value, cf_e2e::Error> {
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

/// The folders Claude is given to read skills from (`--add-dir`), in the order of
/// its command line: its role's first, and where its chief is given a copy of
/// Specmint of ConsensFlow's, the folder that copy is kept in after it. Their
/// skills are under `.claude/skills`.
fn add_dirs(frame: &Value) -> Vec<PathBuf> {
    let argv = frame["argv"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    argv.windows(2)
        .filter(|pair| pair[0] == "--add-dir")
        .filter_map(|pair| pair[1].as_str().map(PathBuf::from))
        .collect()
}

/// The names of the skills the `at`th folder Claude is given holds.
fn skills_in(frame: &Value, at: usize) -> Vec<String> {
    let folder = add_dirs(frame)
        .get(at)
        .map(|root| root.join(".claude").join("skills"));
    let mut names: Vec<String> = folder
        .and_then(|folder| fs::read_dir(folder).ok())
        .map(|entries| {
            entries
                .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The names of the skills a window's role folder holds: the role's own.
fn skills_named(frame: &Value) -> Vec<String> {
    skills_in(frame, 0)
}

/// The names of the skills in the folder a copy of Specmint of ConsensFlow's is
/// kept in, which a window is given besides its role's where its chief has one.
fn specmint_named(frame: &Value) -> Vec<String> {
    skills_in(frame, 1)
}

/// Whether `root` is where ConsensFlow keeps its copy of `variant` for `harness`
/// (`claude`, `codex`) in the rig's home: `integrations/specmint/<harness>`, in a
/// folder named by the variant and the commit it is vendored at, with the hash of the
/// project's folder after them where that is not `.specs` (T-662), and in no launch's
/// folder.
pub(crate) fn is_kept(rig: &Rig, root: &Path, harness: &str, variant: &str) -> bool {
    let beside = rig
        .home()
        .join("integrations")
        .join("specmint")
        .join(harness);
    let commit = root
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix(&format!("{variant}-")));
    let hex = |text: &str, len: usize| {
        text.len() == len && text.bytes().all(|byte| byte.is_ascii_hexdigit())
    };
    root.parent() == Some(beside.as_path())
        && commit.is_some_and(|rest| {
            hex(rest, 40)
                || rest
                    .split_once('-')
                    .is_some_and(|(commit, key)| hex(commit, 40) && hex(key, 12))
        })
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

    // Its next launch has the skill, in the folder it is kept in, which Claude is
    // given after its role's, and names it.
    let next = relaunch(&rig, &project, 2)?;
    assert_eq!(
        skills_named(&next),
        ["consensflow-chief"],
        "{:?}",
        next["argv"]
    );
    assert_eq!(
        specmint_named(&next),
        ["specmint-tdd"],
        "{:?}",
        next["argv"]
    );
    let kept = add_dirs(&next).remove(1);
    assert!(
        is_kept(&rig, &kept, "claude", "tdd"),
        "{} is no folder of the copy: {:?}",
        kept.display(),
        next["argv"]
    );
    let skill = kept.join(".claude").join("skills").join("specmint-tdd");
    let installed = fs::read_to_string(skill.join("SKILL.md"))?;
    assert!(
        installed.starts_with("---\nname: specmint-tdd\n"),
        "{installed}"
    );
    // The project keeps its specs in `plans`, and the skill's files call `.specs` the
    // folder: the copy the chief is given names the project's (T-662).
    assert!(
        !installed.contains(".specs") && installed.contains("plans/registry.md"),
        "the copy sends the chief to .specs: {installed}"
    );
    assert!(
        skill.join(TDD_REFERENCE).is_file(),
        "the references the skill names are beside it"
    );
    let role = role_of(&next);
    assert!(role.contains("## This project's specs"), "{role}");
    assert!(role.contains("`specmint-tdd`"), "{role}");
    assert!(
        role.contains("Your specs and their registry are in `plans`"),
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
    assert_eq!(
        add_dirs(&worker).len(),
        1,
        "no folder of the copy: {:?}",
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

/// What a conversation of the stand-in `claude` was given: the user's turns, oldest first.
fn given_texts(rig: &Rig, session: &str) -> Vec<String> {
    rig.transcript(session)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["type"] == "user")
        .filter_map(|record| record["message"]["content"].as_str().map(str::to_owned))
        .collect()
}

/// The turns of a conversation that say `words`.
fn given_saying(rig: &Rig, session: &str, words: &str) -> Vec<String> {
    given_texts(rig, session)
        .into_iter()
        .filter(|text| text.contains(words))
        .collect()
}

/// The file a chief's section of specs names as its skill's `SKILL.md`.
fn skill_named_in(text: &str) -> Option<String> {
    let (_, rest) = text.split_once("its SKILL.md is at ")?;
    let (file, _) = rest.split_once(", and the files it names are beside it")?;
    Some(file.to_owned())
}

/// Waits until the conversation has been given exactly `count` turns that say `words`.
fn until_told(
    rig: &Rig,
    session: &str,
    words: &str,
    count: usize,
) -> Result<Vec<String>, cf_e2e::Error> {
    let mut found = Vec::new();
    rig.wait_for(&format!("the chief to be told {words:?}"), secs(60), || {
        found = given_saying(rig, session, words);
        Ok(found.len() >= count)
    })?;
    Ok(found)
}

#[test]
fn a_chief_whose_window_is_open_is_told_when_its_specs_are_turned_on_moved_and_off_once_each_and_is_given_no_new_window(
) -> Outcome {
    let rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let first = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let session = session_of(&first);
    rig.wait_for("the chief to be idle", secs(60), || {
        Ok(project.lane("chief")?["activity"]["state"] == "idle")
    })?;
    let page = |specs: Value| -> Result<(), cf_e2e::Error> {
        let mut body = json!({ "project": project.id() });
        body.as_object_mut()
            .expect("an object")
            .extend(specs.as_object().cloned().unwrap_or_default());
        let said = rig.page("project.specs", body)?;
        assert_eq!(said["ok"], true, "{said}");
        Ok(())
    };

    // Turned on while its window is open: told at its next rest, in the conversation it
    // has, and not by a new window.
    page(json!({ "on": true, "folder": "plans", "variant": "tdd" }))?;
    let on = until_told(&rig, &session, "Specs are now on in this project: ", 1)?;
    assert_eq!(on.len(), 1, "{on:?}");
    assert!(
        on[0].contains("Your specs and their registry are in `plans`"),
        "{}",
        on[0]
    );
    // The skill the note names is there, with the folder in its paths, in the folder that is
    // kept for Claude: put in place before the chief was told where it is.
    let skill = skill_named_in(&on[0]).expect("the note names the skill's file");
    let skill = Path::new(&skill);
    let installed = fs::read_to_string(skill)?;
    assert!(
        installed.starts_with("---\nname: specmint-tdd\n")
            && !installed.contains(".specs")
            && installed.contains("plans/registry.md"),
        "{installed}"
    );
    assert!(skill.with_file_name("references").is_dir());
    let kept = skill.ancestors().nth(4).expect("the copy's folder");
    assert!(is_kept(&rig, kept, "claude", "tdd"), "{}", kept.display());

    // The folder moves: told with the section anew and the copy for the new folder.
    page(json!({ "on": true, "folder": "docs/specs", "variant": "tdd" }))?;
    let moved = until_told(
        &rig,
        &session,
        "the specs are now kept in `docs/specs`, not `plans`",
        1,
    )?;
    let moved_skill = skill_named_in(&moved[0]).expect("the note names the skill's file");
    assert!(Path::new(&moved_skill).is_file(), "{moved_skill}");
    assert_ne!(moved_skill, skill.to_string_lossy(), "a copy of its own");

    // Turned off: told to plan as tasks, and that the specs there stay.
    page(json!({ "on": false }))?;
    let off = until_told(&rig, &session, "Specs are now off in this project: ", 1)?;
    assert!(
        off[0]
            .contains("Plan new work as tasks, not specs; the specs already in `docs/specs` stay."),
        "{}",
        off[0]
    );

    // Each was told once, whatever the looks that came between, and no window was opened.
    rig.wait_for("the chief to be idle", secs(60), || {
        Ok(project.lane("chief")?["activity"]["state"] == "idle")
    })?;
    std::thread::sleep(secs(3));
    for words in [
        "Specs are now on in this project: ",
        "the specs are now kept in `docs/specs`, not `plans`",
        "Specs are now off in this project: ",
    ] {
        assert_eq!(given_saying(&rig, &session, words).len(), 1, "{words}");
    }
    assert_eq!(chief_windows(&rig, &project).len(), 1, "no new window");
    // The next launch has the role as it is, and the chief is told nothing of it again.
    let next = relaunch(&rig, &project, 2)?;
    assert!(!told_of_specs(&role_of(&next)), "specs are off now");
    rig.close()?;
    Ok(())
}

#[test]
fn a_project_opened_with_specs_on_has_its_chief_given_the_skill_beside_its_role_at_its_very_first_window(
) -> Outcome {
    let rig = rig()?;
    // The human says it as the project opens, in the one request New project makes.
    let project = Project::open(
        &rig,
        "chief",
        json!({ "specs": { "on": true, "folder": "plans", "variant": "tdd" } }),
    )?;
    let first = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    // Its first window, which nothing relaunched, has the skill in the folder it is
    // kept in, and its role names it and the folder the specs are kept in.
    assert_eq!(chief_windows(&rig, &project).len(), 1);
    assert_eq!(
        skills_named(&first),
        ["consensflow-chief"],
        "{:?}",
        first["argv"]
    );
    assert_eq!(
        specmint_named(&first),
        ["specmint-tdd"],
        "{:?}",
        first["argv"]
    );
    let kept = add_dirs(&first).remove(1);
    assert!(
        is_kept(&rig, &kept, "claude", "tdd"),
        "{} is no folder of the copy: {:?}",
        kept.display(),
        first["argv"]
    );
    let role = role_of(&first);
    assert!(role.contains("## This project's specs"), "{role}");
    assert!(
        role.contains("Your specs and their registry are in `plans`"),
        "{role}"
    );
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
    assert_eq!(skills_named(&first), ["consensflow-chief"]);
    assert_eq!(specmint_named(&first), ["specmint-core"]);
    assert!(
        role_of(&first).contains("Your specs and their registry are in `.specs`"),
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
    assert_eq!(add_dirs(&next).len(), 1, "{:?}", next["argv"]);
    assert!(!told_of_specs(&role_of(&next)), "told nothing of them");
    rig.close()?;
    Ok(())
}

#[test]
fn what_settings_says_for_every_project_is_the_folder_and_the_variant_a_chief_is_given_where_its_project_names_none(
) -> Outcome {
    let rig = rig()?;
    // Settings, Specs: on for every project, in this folder, with this variant.
    let said = rig.page(
        "specs.set",
        json!({ "on": true, "folder": "docs/specs", "variant": "core-html" }),
    )?;
    assert_eq!(said["ok"], true, "{said}");
    assert_eq!(
        said["applied"],
        json!({ "folder": "docs/specs", "variant": "core-html" })
    );
    // A project that says nothing: its chief's first window has the app's variant and folder.
    let project = Project::open(&rig, "chief", json!({}))?;
    let first = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    assert_eq!(
        specmint_named(&first),
        ["specmint-core-html"],
        "{:?}",
        first["argv"]
    );
    let kept = add_dirs(&first).remove(1);
    assert!(is_kept(&rig, &kept, "claude", "core-html"), "{kept:?}");
    let role = role_of(&first);
    assert!(
        role.contains("Your specs and their registry are in `docs/specs`"),
        "{role}"
    );
    // It names its own variant: that is the one, in the app's folder still.
    let said = rig.page(
        "project.specs",
        json!({ "project": project.id(), "variant": "tdd" }),
    )?;
    assert_eq!(said["ok"], true, "{said}");
    let next = relaunch(&rig, &project, 2)?;
    assert_eq!(
        specmint_named(&next),
        ["specmint-tdd"],
        "{:?}",
        next["argv"]
    );
    assert!(
        role_of(&next).contains("Your specs and their registry are in `docs/specs`"),
        "{}",
        role_of(&next)
    );
    // Settings turns specs off for every project: the project, which says nothing of whether, follows.
    let said = rig.page("specs.set", json!({ "on": false }))?;
    assert_eq!(said["ok"], true, "{said}");
    let third = relaunch(&rig, &project, 3)?;
    assert_eq!(add_dirs(&third).len(), 1, "{:?}", third["argv"]);
    assert!(!told_of_specs(&role_of(&third)), "told nothing of them");
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
    assert_eq!(
        add_dirs(&next).len(),
        1,
        "nor is a folder of it given: {:?}",
        next["argv"]
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
    assert_eq!(skills_named(&third), ["consensflow-chief"]);
    assert_eq!(specmint_named(&third), ["specmint-tdd-html"]);
    assert!(installed.join("SKILL.md").is_file());
    rig.close()?;
    Ok(())
}
