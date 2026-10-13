//! A chief on the stand-in Codex, whose resumed thread keeps the instructions it
//! began with: told of the specs its project turned on by a message when it is
//! resumed, once, and of nothing at the Resume after, for its role text names the
//! same copy of Specmint at every launch, kept where no launch's folder holds it.
//! (The windows run the stand-in `codex`, a script that runs it, so Unix's; the
//! cases of the stand-in `claude` are in `specs`.)

use std::path::{Path, PathBuf};

use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::rollout;
use serde_json::{json, Value};

use crate::specs::{is_kept, relaunch};
use crate::{after, config, secs, Outcome};

/// The stand-in for Codex, which the windows of Codex's agents run.
const FAKE_CODEX: &str = env!("CARGO_BIN_EXE_fake-codex");

/// What the section of specs in a chief's text begins with.
const SECTION: &str = "## This project's specs";

/// What a message that gives a role again says, which no other note does.
const REPLACES: &str = "replace the ones you were given then";

/// The role text a Codex window was launched with: the JSON string of the
/// `developer_instructions` setting on its command line.
fn codex_role_of(frame: &Value) -> String {
    frame["argv"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(|word| word.strip_prefix("developer_instructions="))
        .and_then(|setting| serde_json::from_str(setting).ok())
        .unwrap_or_default()
}

/// The file of the skill a role text names: where its section says the
/// `SKILL.md` is.
fn skill_named_in(text: &str) -> Option<String> {
    let (_, rest) = text.split_once("its SKILL.md is at ")?;
    let (file, _) = rest.split_once(", and the files it names are beside it")?;
    Some(file.to_owned())
}

/// The last word of a window's command line: a Codex window's first message.
fn last_word(frame: &Value) -> String {
    frame["argv"]
        .as_array()
        .and_then(|argv| argv.last())
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The chief's turn is over: its lane reads idle.
fn chief_is_idle(project: &Project) -> Result<bool, cf_e2e::Error> {
    Ok(project.lane("chief")?["activity"]["state"] == "idle")
}

#[test]
fn a_codex_chief_resumed_after_its_specs_were_turned_on_is_told_the_section_by_a_message_and_once_for_its_thread_keeps_what_it_began_with(
) -> Outcome {
    // The human has no Specmint installed for Codex: the skill is ConsensFlow's copy.
    let rig = Rig::start(config().codex(FAKE_CODEX))?;
    let project = Project::open(&rig, "maia", json!({}))?;
    let added = project.add_member("worker")?;
    let first = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    assert!(
        first["argv"]
            .as_array()
            .is_some_and(|argv| argv.iter().any(|word| word == "codex-session")),
        "a window on Codex: {}",
        first["argv"]
    );
    assert!(codex_role_of(&first).contains("Your ConsensFlow role is chief"));
    assert!(!codex_role_of(&first).contains(SECTION), "specs are off");

    // The chief has a turn, which is what gives its thread the instructions it
    // begins with (and what lets Codex resume it): a worker's result comes to it.
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    let ran = rig.cf_in_window(
        &first,
        [
            "task",
            "add",
            "--tier",
            tier,
            "Reply with exactly: WORKER_OK",
        ],
    )?;
    assert_eq!(ran.code, Some(0), "{ran}");
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let home = PathBuf::from(rig.var("CODEX_HOME").unwrap_or_default());
    let thread_text = || -> Result<String, cf_e2e::Error> {
        match rollout::rollouts(&home).as_slice() {
            [only] => rollout::read(only),
            others => Ok(format!("{} rollouts", others.len())),
        }
    };
    // The result is in its thread and the daemon has seen it arrive: a window that is
    // closed before it has is given the message again.
    rig.wait_for("the worker's result to reach the chief", secs(60), || {
        let arrived = rollout::said_by(&thread_text()?, "user")
            .iter()
            .any(|said| said.contains("WORKER_OK"));
        let delivered = project
            .inbox("chief")?
            .iter()
            .any(|message| message["kind"] == "result" && message["state"] == "delivered");
        Ok(arrived && delivered)
    })?;
    rig.wait_for("the chief to be idle", secs(60), || chief_is_idle(&project))?;
    let began_with = rollout::said_by(&thread_text()?, "developer");
    assert_eq!(
        began_with.len(),
        1,
        "its thread has the one set of instructions it began with"
    );
    assert!(began_with[0].contains("Your ConsensFlow role is chief"));
    assert!(!began_with[0].contains(SECTION));
    let thread = rollout::thread_of(&thread_text()?).expect("its thread");

    // The human turns specs on, and Closes and Resumes: the window resumes the
    // thread, and carries the section on its command line as every launch does,
    // which the thread reads and does not use.
    let said = rig.page(
        "project.specs",
        json!({ "project": project.id(), "on": true, "folder": "plans", "variant": "core" }),
    )?;
    assert_eq!(said["ok"], true, "{said}");
    let next = relaunch(&rig, &project, 2)?;
    assert_eq!(after(&next, "resume"), thread, "{}", next["argv"]);
    assert!(codex_role_of(&next).contains(SECTION));

    // Its first message is the role, whole, said to replace the one it began with:
    // the chief's window is told the section.
    let message = last_word(&next);
    assert!(message.contains(REPLACES), "{message}");
    assert!(message.contains(SECTION), "{message}");
    assert!(message.contains("it means `plans`"), "{message}");
    // It names ConsensFlow's copy of the skill, in the folder that is kept for the
    // harness and the variant, and that file is there.
    let skill = skill_named_in(&message).expect("the section names the skill's file");
    let skill = Path::new(&skill);
    let kept = skill.ancestors().nth(4).expect("the copy's folder");
    assert!(
        is_kept(&rig, kept, "codex", "core"),
        "{} is no folder of the copy",
        kept.display()
    );
    assert!(
        skill.is_file(),
        "{} is where the role says",
        skill.display()
    );
    assert!(skill.with_file_name("references").is_dir());
    rig.wait_for(
        "the chief's thread to be told of the specs",
        secs(60),
        || {
            Ok(rollout::said_by(&thread_text()?, "user")
                .iter()
                .any(|said| said.contains(SECTION)))
        },
    )?;
    let record = thread_text()?;
    assert_eq!(
        rollout::said_by(&record, "developer"),
        began_with,
        "the thread keeps the instructions it began with: no section in them"
    );
    let told: Vec<String> = rollout::said_by(&record, "user")
        .into_iter()
        .filter(|said| said.contains(SECTION))
        .collect();
    assert_eq!(told.len(), 1, "told once: {told:?}");
    assert!(told[0].contains(REPLACES));

    // The next Resume has nothing to tell: the role is what it was told, and names
    // the same copy of the skill, which the window before it left where it was.
    rig.wait_for("the chief to be idle", secs(60), || chief_is_idle(&project))?;
    let third = relaunch(&rig, &project, 3)?;
    assert_eq!(after(&third, "resume"), thread);
    assert!(
        !third["argv"].to_string().contains(REPLACES),
        "no message of a role: {}",
        third["argv"]
    );
    assert_eq!(
        codex_role_of(&third),
        codex_role_of(&next),
        "the role is the same text at every launch"
    );
    assert!(
        skill.is_file(),
        "the copy outlasts the window that was told of it"
    );
    rig.wait_for("the chief to be idle", secs(60), || chief_is_idle(&project))?;
    let record = thread_text()?;
    assert_eq!(
        rollout::said_by(&record, "user")
            .iter()
            .filter(|said| said.contains(SECTION))
            .count(),
        1,
        "given once, not at every launch"
    );
    assert_eq!(rollout::said_by(&record, "developer"), began_with);
    rig.close()?;
    Ok(())
}
