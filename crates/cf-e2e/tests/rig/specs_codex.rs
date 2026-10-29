//! A chief on the stand-in Codex, whose resumed thread keeps the instructions it
//! began with: told of the specs its project turned on by a message when it is
//! resumed (by paste, once the window is up: its command line has the role once),
//! once, and of nothing at the Resume after, for its role text names the same copy
//! of Specmint at every launch, kept where no launch's folder holds it. The role is
//! on no command line: it is a file of the launch's, which the window's broker gives
//! its thread as it starts.
//! (The windows run the stand-in `codex`, a script that runs it, so Unix's; the
//! cases of the stand-in `claude` are in `specs`.)

use std::fs;
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

/// The role text a Codex window was launched with: the file its environment names,
/// which its broker gives its main thread as it starts. It goes with its window, so
/// it is read while the window is up.
fn codex_role_of(frame: &Value) -> String {
    frame["env"]["CF_CODEX_ROLE_FILE"]
        .as_str()
        .and_then(|file| fs::read_to_string(file).ok())
        .unwrap_or_default()
}

/// That a window's command line holds nothing of its role.
fn no_role_on_the_line(frame: &Value) {
    let line = frame["argv"].to_string();
    assert!(
        !line.contains("Your ConsensFlow role is") && !line.contains(SECTION),
        "the role is on the command line: {line}"
    );
}

/// The file of the skill a role text names: where its section says the
/// `SKILL.md` is.
fn skill_named_in(text: &str) -> Option<String> {
    let (_, rest) = text.split_once("its SKILL.md is at ")?;
    let (file, _) = rest.split_once(", and the files it names are beside it")?;
    Some(file.to_owned())
}

/// The chief's turn is over: its lane reads idle.
fn chief_is_idle(project: &Project) -> Result<bool, cf_e2e::Error> {
    Ok(project.lane("chief")?["activity"]["state"] == "idle")
}

/// How many notes the daemon wrote to tell the chief its role again. A launch
/// writes one before its window opens, so this says at once whether a Resume had
/// anything to tell: the chief's own record would say it only once the window had
/// been given the note, which is after the chief reads idle.
fn notes_of_the_role(project: &Project) -> Result<usize, cf_e2e::Error> {
    Ok(project
        .inbox("chief")?
        .iter()
        .filter(|note| note["body"].to_string().contains(REPLACES))
        .count())
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
    let first_role = codex_role_of(&first);
    assert!(first_role.contains("Your ConsensFlow role is chief"));
    assert!(!first_role.contains(SECTION), "specs are off");
    no_role_on_the_line(&first);

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
    // thread, and its role's file holds the section as every launch's does, which
    // the thread is not given (its broker gives the role to a thread that starts).
    // It has no first message: the note that says the role again goes in by paste
    // once the window is up. Neither is on the command line.
    let said = rig.page(
        "project.specs",
        json!({ "project": project.id(), "on": true, "folder": "plans", "variant": "core" }),
    )?;
    assert_eq!(said["ok"], true, "{said}");
    let next = relaunch(&rig, &project, 2)?;
    assert_eq!(after(&next, "resume"), thread, "{}", next["argv"]);
    let next_role = codex_role_of(&next);
    assert!(next_role.contains(SECTION));
    no_role_on_the_line(&next);
    let line = next["argv"].to_string();
    assert!(!line.contains(REPLACES), "no message of a role: {line}");

    // The chief's thread is told the section, by the role, whole, said to replace the
    // one it began with, and keeps the instructions it began with.
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
    let message = &told[0];
    assert!(message.contains(REPLACES), "{message}");
    assert!(message.contains("it means `plans`"), "{message}");
    // It names ConsensFlow's copy of the skill, in the folder that is kept for the
    // harness and the variant, and that file is there: the one the window's role
    // names too.
    let skill = skill_named_in(message).expect("the section names the skill's file");
    assert_eq!(
        skill_named_in(&next_role).as_deref(),
        Some(skill.as_str()),
        "one copy, named alike by the window's role and by the message"
    );
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
    // The daemon has seen it arrive, and what it was told is recorded: the
    // Resume after has nothing to say.
    rig.wait_for("the note to be delivered", secs(60), || {
        Ok(project.inbox("chief")?.iter().any(|note| {
            note["state"] == "delivered" && note["body"].to_string().contains(REPLACES)
        }))
    })?;

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
    no_role_on_the_line(&third);
    assert_eq!(
        codex_role_of(&third),
        next_role,
        "the role is the same text at every launch"
    );
    assert_eq!(
        notes_of_the_role(&project)?,
        1,
        "the Resume wrote nothing to tell it the role again"
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
