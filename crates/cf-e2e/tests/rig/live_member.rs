//! A Devin member's window as the live check `live_devin_resume` opens one, held to
//! what it says without spending a token (the stand-in Devin is the harness): the
//! adapter prepares the launch whole (the owner's config with the hooks ConsensFlow
//! adds, the first message in a prompt file, the wire log and the role in variables),
//! each window is a launch of its own, a window cut as the app's quit cuts it is
//! opened again on its conversation with the note that its turn was cut as the first
//! message of its launch, and the stand-in `cf` is found before any other. What the
//! stand-in cannot say (what a real Devin does with it) is what the live check is for.

use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::devin_store::{self, Message};
use cf_e2e::live::window::{Member, Open};
use cf_e2e::live::{ask, calls, until, Harness, Installed};
use cf_e2e::rig::Rig;
use cf_e2e::{cf, files};
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// The stand-in Devin, which the rig's windows find as `devin`, and the stand-in for
/// the `cf` of a member's window.
const FAKE_DEVIN: &str = env!("CARGO_BIN_EXE_fake-devin");
const FAKE_CF: &str = env!("CARGO_BIN_EXE_fake-cf");

/// The word after `flag` in a command line.
fn after<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    let at = argv.iter().position(|word| word == flag)?;
    argv.get(at + 1).map(String::as_str)
}

/// The conversation the stand-in says its window is on.
fn started_on(screen: &str) -> Option<String> {
    let after = screen.split("fake devin started on ").nth(1)?;
    after.split_whitespace().next().map(str::to_owned)
}

/// The messages of the session, once the user's message with `header` is in it and
/// the stand-in has answered it with something that starts `answer`.
fn answered(store: &Path, session: &str, header: &str, answer: &str) -> Option<Vec<Message>> {
    let found = devin_store::messages_of(store, session).ok()?;
    let at = found
        .iter()
        .position(|message| message.role == "user" && message.text.contains(header))?;
    found[at + 1..]
        .iter()
        .any(|message| message.role == "assistant" && message.text.starts_with(answer))
        .then_some(found)
}

/// The file a command line names after `flag`, or why it names none.
fn file_of(argv: &[String], flag: &str) -> Result<PathBuf, String> {
    after(argv, flag)
        .map(PathBuf::from)
        .ok_or_else(|| format!("the window was not opened with {flag}: {argv:?}"))
}

#[test]
fn a_devin_members_window_is_opened_whole_cut_and_opened_again_on_its_conversation_with_the_note_in_its_launch(
) -> Outcome {
    // Built before the first question, which a run asks for first of all.
    ask::prepare()?;
    let stand_in = tempfile::tempdir()?;
    calls::install(stand_in.path(), Path::new(FAKE_CF))?;
    let rig = Rig::start(config().devin(FAKE_DEVIN).first_on_path(stand_in.path()))?;
    let home = rig.var("HOME").unwrap_or_default().to_owned();
    let install = Installed::at(Harness::Devin, FAKE_DEVIN, &home);
    let agent = json!({ "kind": "devin", "model": "fake" });
    let role = "# ConsensFlow worker\nRun cf ask to ask the chief.\n";
    let task = "[ConsensFlow m-1 · T-1 · task from @chief]\nReply with exactly: TASK_DONE";
    let note = "[ConsensFlow m-2 · T-1 · note from ConsensFlow]\nConsensFlow quit while you were working on T-1. Go on where you stopped.";

    // The window, as the daemon opens a member's.
    let first = Open::new(&rig, &install, "member", "member-devin")
        .agent(agent.clone())
        .member(Member::new("worker", role).first_message(task))
        .open()?;
    let argv = first.argv().to_vec();
    assert!(
        argv[0].contains("devin"),
        "the program the adapter found: {argv:?}"
    );
    assert_eq!(after(&argv, "--model"), Some("fake"));
    assert_eq!(after(&argv, "--permission-mode"), Some("dangerous"));
    assert_eq!(after(&argv, "--respect-workspace-trust"), Some("false"));
    assert!(!argv.contains(&"--resume".to_owned()), "{argv:?}");
    // Its launch is a folder of its own below the window's folder, with its files: the
    // config with the hooks, the task in a prompt file, the role the session starts with.
    let launches = first
        .workspace()
        .join(".consensflow")
        .join("integrations")
        .join("devin");
    let config_file = file_of(&argv, "--config")?;
    let launch = config_file
        .parent()
        .ok_or("the config is in no folder")?
        .to_path_buf();
    assert_eq!(launch.parent(), Some(launches.as_path()));
    assert_eq!(files::read_string(&file_of(&argv, "--prompt-file")?)?, task);
    let configured: Value = serde_json::from_str(&files::read_string(&config_file)?)?;
    let real_cf = cf::binary()?.to_string_lossy().replace('\\', "/");
    assert_eq!(
        configured["hooks"]["SessionStart"][0]["hooks"][0]["command"],
        json!(format!("'{real_cf}' hook devin-session"))
    );
    assert_eq!(
        configured["hooks"]["PreToolUse"][0]["matcher"],
        "ask_user_question"
    );
    assert_eq!(configured["auto_update"], false);
    let role_file = launch
        .join("role")
        .join(".claude")
        .join("skills")
        .join("consensflow-worker")
        .join("SKILL.md");
    assert!(files::read_string(&role_file)?.starts_with(role));

    // It ran on the prompt file, and wrote the wire log of its launch where it was told to.
    assert!(first.still(secs(60)), "{}", first.screen());
    let session = started_on(&first.screen()).ok_or("the stand-in named no conversation")?;
    let store = devin_store::location(
        cfg!(windows),
        &home,
        rig.var("APPDATA"),
        rig.var("XDG_DATA_HOME"),
    );
    let step = Duration::from_millis(200);
    until(secs(30), step, || {
        answered(&store, &session, "[ConsensFlow m-1 ", "TASK_DONE")
    })
    .ok_or("the task never was answered in the stand-in's store")?;
    assert!(launch.join("wire.jsonl").is_file());

    // The quit: the window's process ends.
    first.kill();
    until(secs(30), Duration::from_millis(100), || {
        first.closed().then_some(())
    })
    .ok_or("the window did not end")?;

    // The daemon that starts again: a launch of its own on the conversation, with the note
    // as its first message, in its own prompt file.
    let again = Open::new(&rig, &install, "member", "member-devin-again")
        .agent(agent)
        .member(Member::new("worker", role).first_message(note))
        .resume(&session)
        .open()?;
    let argv = again.argv().to_vec();
    assert_eq!(after(&argv, "--resume"), Some(session.as_str()));
    let second = file_of(&argv, "--config")?;
    assert_ne!(
        second.parent(),
        Some(launch.as_path()),
        "a window is a launch of its own"
    );
    assert_eq!(
        second.parent().and_then(Path::parent),
        Some(launches.as_path())
    );
    assert_eq!(files::read_string(&file_of(&argv, "--prompt-file")?)?, note);
    assert!(again.still(secs(60)), "{}", again.screen());
    assert_eq!(
        started_on(&again.screen()).as_deref(),
        Some(session.as_str())
    );
    let kept = until(secs(30), step, || {
        answered(&store, &session, "[ConsensFlow m-2 ", "noted: ")
    })
    .ok_or("the note never was answered in the stand-in's store")?;
    // One conversation: the task and its answer, then the note and its answer.
    let at = |header: &str| {
        kept.iter()
            .position(|message| message.role == "user" && message.text.contains(header))
    };
    assert!(
        at("[ConsensFlow m-1 ") < at("[ConsensFlow m-2 "),
        "{kept:?}"
    );
    // What is read of one conversation is not what is read of another.
    assert!(devin_store::messages_of(&store, "another-conversation")?.is_empty());

    // The stand-in `cf` is found before the machine's: its folder is right after the one
    // of the stand-in Claude in the windows' PATH.
    let path = rig.var("PATH").unwrap_or_default();
    let delimiter = if cfg!(windows) { ';' } else { ':' };
    let folders: Vec<&str> = path.split(delimiter).collect();
    assert_eq!(Path::new(folders[1]), stand_in.path());
    drop((first, again));
    rig.close()?;
    Ok(())
}

#[test]
fn the_launch_of_a_member_is_prepared_for_devin_alone() -> Outcome {
    let rig = crate::rig()?;
    let home = rig.var("HOME").unwrap_or_default().to_owned();
    let claude = Installed::at(Harness::Claude, "/nowhere/claude", &home);
    let refused = Open::new(&rig, &claude, "member", "member-claude")
        .member(Member::new("worker", "# worker\n"))
        .open()
        .err()
        .map(|failed| failed.to_string());
    assert_eq!(
        refused.as_deref(),
        Some("a member's launch is prepared for Devin alone, not claude")
    );
    rig.close()?;
    Ok(())
}
