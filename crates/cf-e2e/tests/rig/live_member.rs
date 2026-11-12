//! A Devin member's window as the live check `live_devin_resume` opens one, held to
//! what it says without spending a token (the stand-in Devin is the harness): the
//! adapter prepares the launch whole (the owner's config with the hooks ConsensFlow
//! adds, the first message in a prompt file, the wire log and the role in variables),
//! each window is a launch of its own, a window cut as the app's quit cuts it is
//! opened again on its conversation with the engine's note that its turn was cut as
//! the first message of its launch, behind `/bypass`, one cut in its first turn and
//! opened again with nothing comes back in the normal mode and is switched as the
//! daemon's readiness switches it, and the stand-in `cf` is found before any other.
//! What the stand-in cannot say (what a real Devin does with it) is what the live
//! check is for.

use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::devin_store::{self, Message};
use cf_e2e::files;
use cf_e2e::live::window::{Member, Open};
use cf_e2e::live::{ask, calls, until, Harness, Installed, Log};
use cf_e2e::rig::Rig;
use serde_json::{json, Value};

use crate::{config, devin_config, installed_devin, on_the_installed_devin, secs, Outcome};

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
    let rig = Rig::start(devin_config(FAKE_DEVIN).first_on_path(stand_in.path()))?;
    let home = rig.var("HOME").unwrap_or_default().to_owned();
    let install = Installed::at(Harness::Devin, FAKE_DEVIN, &home);
    let agent = json!({ "kind": "devin", "model": "fake" });
    let role = "# ConsensFlow worker\nRun cf ask to ask the chief.\n";
    let task = "[ConsensFlow m-1 · T-1 · task from @chief]\nReply with exactly: TASK_DONE";
    // The note is the engine's own, as the live check takes it.
    let note = &ask::cut_note(2, &[1], ask::For::new(rig.cf(), "devin"))?;
    assert_eq!(
        note,
        "[ConsensFlow m-2 · T-1 · note from ConsensFlow]\nConsensFlow stopped while you were working on T-1. Go on where you stopped."
    );

    // The window, as the daemon opens a member's.
    let first = Open::new(&rig, &install, "member", "member-devin")
        .agent(agent.clone())
        .member(Member::new("worker", role).first_message(task))
        .open(&Log::new("live member"))?;
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
    let real_cf = rig.cf().to_string_lossy().replace('\\', "/");
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
        .open(&Log::new("live member"))?;
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
    // Behind `/bypass`, which brings back in full permission a Devin that resumed the
    // session in its normal mode (B1, 2026-10-15).
    assert_eq!(
        files::read_string(&file_of(&argv, "--prompt-file")?)?,
        format!("/bypass {note}")
    );
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

/// A Devin window of `rig` the app's quit cut in its first turn (its message in the store and
/// nothing after it), and the window opened again on its conversation with nothing to say, as
/// the human's Show opens one.
struct Cut<'r> {
    first: cf_e2e::live::window::Window<'r>,
    again: cf_e2e::live::window::Window<'r>,
    /// The wire log of the window opened again.
    wire: PathBuf,
    /// Devin's store.
    store: PathBuf,
    session: String,
}

fn cut_and_opened_again<'r>(
    rig: &'r Rig,
    install: &'r Installed,
) -> Result<Cut<'r>, Box<dyn std::error::Error>> {
    let home = rig.var("HOME").unwrap_or_default().to_owned();
    let agent = json!({ "kind": "devin", "model": "fake" });
    let role = "# ConsensFlow worker\n";
    let task = "[ConsensFlow m-1 · T-1 · task from @chief]\nSLEEP 600 Reply with exactly: LATE";
    let store = devin_store::location(
        cfg!(windows),
        &home,
        rig.var("APPDATA"),
        rig.var("XDG_DATA_HOME"),
    );
    let step = Duration::from_millis(200);

    // Its first turn is at work: its message is in the store, and nothing after it.
    let first = Open::new(rig, install, "cut", "cut-devin")
        .agent(agent.clone())
        .member(Member::new("worker", role).first_message(task))
        .open(&Log::new("live member"))?;
    let session = until(secs(30), step, || started_on(&first.screen()))
        .ok_or("the stand-in named no conversation")?;
    until(secs(30), step, || {
        devin_store::messages_of(&store, &session)
            .ok()
            .filter(|kept| kept.len() == 1)
    })
    .ok_or("the first turn never began")?;
    first.kill();
    until(secs(30), step, || first.closed().then_some(())).ok_or("the window did not end")?;

    // Opened again on it with nothing to say, as the human's Show opens a window.
    let again = Open::new(rig, install, "cut", "cut-devin-again")
        .agent(agent)
        .member(Member::new("worker", role))
        .resume(&session)
        .open(&Log::new("live member"))?;
    let wire = file_of(again.argv(), "--config")?.with_file_name("wire.jsonl");
    Ok(Cut {
        first,
        again,
        wire,
        store,
        session,
    })
}

/// A window the app's quit cut in its first turn comes back in Devin's normal mode,
/// whatever its flag says (the stand-in does as Devin 3000.11.92 did, B1 of
/// 2026-10-15): the daemon's readiness, the product's own (`ask::devin_readiness`),
/// reads that mode on its wire and gives the window `/bypass` first, which switches it
/// and makes no turn, no item of its record and nothing the daemon's reader takes for
/// an answer, and the window is ready. The check gives the window nothing itself: one
/// that pasted the command passed whatever the product did (Astra and Sol on the fix).
/// On Devin 3000.12.43's shape the window says no mode at all (`silent_on_3000_12`), in the
/// case's twin `…_on_devin_3000_12`: each chooses its shape itself, not the environment.
#[test]
fn a_window_cut_in_its_first_turn_comes_back_in_the_normal_mode_and_the_daemons_readiness_switches_it(
) -> Outcome {
    ask::prepare()?;
    let rig = Rig::start(devin_config(FAKE_DEVIN))?;
    let install = Installed::at(
        Harness::Devin,
        FAKE_DEVIN,
        rig.var("HOME").unwrap_or_default(),
    );
    let Cut {
        first,
        again,
        wire,
        store,
        session,
    } = cut_and_opened_again(&rig, &install)?;
    let step = Duration::from_millis(200);
    if installed_devin() {
        silent_on_3000_12(&rig, &again, &wire, &store, &session)?;
        drop((first, again));
        rig.close()?;
        return Ok(());
    }
    let read = until(secs(30), step, || {
        ask::devin_ready(&wire)
            .ok()
            .filter(|read| read.mode.is_some())
    })
    .ok_or("its wire never said its mode")?;
    assert_eq!(read.shown.as_deref(), Some(session.as_str()));
    assert_eq!(
        read.mode.as_deref(),
        Some("normal"),
        "whatever its flag says"
    );
    assert_eq!(read.command.as_deref(), Some("/bypass"));

    let kept = devin_store::messages_of(&store, &session)?.len();
    let readiness = ask::devin_readiness(&rig, &again, &wire, secs(30))?;
    assert!(readiness.ready, "{readiness:?}");
    assert_eq!(readiness.given, ["/bypass"], "{readiness:?}");
    assert_eq!(
        readiness.answers.first().map(String::as_str),
        Some("held: Devin's window is in its normal permission mode, and /bypass was sent to switch it to full permission"),
        "{readiness:?}"
    );
    let read = ask::devin_ready(&wire)?;
    assert_eq!(read.mode.as_deref(), Some("bypass"));
    assert!(
        again.screen().contains("Switched to Bypass mode"),
        "{}",
        again.screen()
    );
    // No turn: nothing in the store, and the last of its record is still the cut turn's.
    assert_eq!(devin_store::messages_of(&store, &session)?.len(), kept);
    let record = again.record().ok_or("its record cannot be read")?;
    assert_eq!(
        record.items.last().map(|item| item.role.as_str()),
        Some("user")
    );
    drop((first, again));
    rig.close()?;
    Ok(())
}

/// The window of the case above on Devin 3000.12.43's shape, which says nothing of its mode
/// opened again so (T-1417 run 3; T-1432 runs A, 2 and 3): its wire names the conversation on
/// its lines and configures none. The daemon's readiness holds it while a Devin that announces
/// would have, then finds it ready with nothing given, and its delivery goes behind `/bypass`,
/// which runs the turn in full permission and makes the window say its mode. What the window
/// is given is the product's.
fn silent_on_3000_12(
    rig: &Rig,
    again: &cf_e2e::live::window::Window,
    wire: &Path,
    store: &Path,
    session: &str,
) -> Outcome {
    let step = Duration::from_millis(200);
    until(secs(30), step, || {
        files::read_string(wire)
            .ok()
            .filter(|log| log.contains(&format!("\"sessionId\":\"{session}\"")))
    })
    .ok_or("its wire never named its conversation")?;
    let read = ask::devin_ready(wire)?;
    assert_eq!((read.shown, read.mode, read.command), (None, None, None));
    let note = "[ConsensFlow m-2 · T-1 · note from @chief]\nReply with exactly: GONE_ON";
    let began = std::time::Instant::now();
    let readiness = ask::devin_readiness_then(rig, again, wire, secs(40), Some(note))?;
    assert!(readiness.ready, "{readiness:?}");
    assert!(
        began.elapsed() >= Duration::from_secs(9),
        "{:?}",
        began.elapsed()
    );
    assert_eq!(
        readiness.answers,
        [
            "held: Devin has not said yet which conversation its window shows",
            "ready"
        ],
        "{readiness:?}"
    );
    assert_eq!(readiness.delivered, Some(true), "{readiness:?}");
    // As the app pastes it: Devin on Windows gets its marks in ASCII (`console_text`).
    assert_eq!(
        readiness.given,
        [format!("/bypass {}", again.given(note)?)],
        "{readiness:?}"
    );
    // The turn says the mode as it begins, and runs: its answer is in the store, and its
    // message is kept without the command.
    until(secs(30), step, || {
        ask::devin_ready(wire)
            .ok()
            .filter(|read| read.mode.as_deref() == Some("bypass"))
    })
    .ok_or("its wire never said full permission")?;
    assert_eq!(ask::devin_ready(wire)?.shown.as_deref(), Some(session));
    let answered = until(secs(30), step, || {
        devin_store::messages_of(store, session)
            .ok()
            .filter(|kept| {
                kept.iter()
                    .any(|message| message.role == "assistant" && message.text.contains("GONE_ON"))
            })
    })
    .ok_or("the note's turn never answered")?;
    assert!(
        answered
            .iter()
            .any(|message| message.role == "user" && message.text.starts_with("[ConsensFlow m-2 ")),
        "{answered:?}"
    );
    Ok(())
}

/// The window of the case above on Devin 3000.12.43's shape, typed into by the human before the
/// daemon's while is over: an Escape, which changes nothing in the window and leaves nothing in
/// its box. Its log's silence says nothing now of where the window is (a `/new` that writes no
/// line would leave it naming the conversation the window left, T-1466's review), and the
/// daemon's readiness holds it as it did before T-1432, with nothing given, until its log
/// configures (T-1474). The pane host is the rig's: the Escape comes by the human's door
/// (`pane.typed`), the daemon's keys and pastes by its own. The case is 3000.12's shape whatever
/// the run says: on the default shape the window says its mode, and nothing waits on the human's
/// keys.
#[test]
fn a_window_cut_in_its_first_turn_that_the_human_typed_into_before_it_said_its_mode_is_held_with_nothing_given(
) -> Outcome {
    ask::prepare()?;
    let rig = Rig::start(
        config()
            .devin(FAKE_DEVIN)
            .var("FAKE_DEVIN_STAMPS", "3000.12"),
    )?;
    let install = Installed::at(
        Harness::Devin,
        FAKE_DEVIN,
        rig.var("HOME").unwrap_or_default(),
    );
    let Cut {
        first,
        again,
        wire,
        session,
        ..
    } = cut_and_opened_again(&rig, &install)?;
    let step = Duration::from_millis(200);
    until(secs(30), step, || {
        files::read_string(&wire)
            .ok()
            .filter(|log| log.contains(&format!("\"sessionId\":\"{session}\"")))
    })
    .ok_or("its wire never named its conversation")?;
    again.types(b"\x1b")?;
    let note = "[ConsensFlow m-2 · T-1 · note from @chief]\nReply with exactly: GONE_ON";
    let readiness = ask::devin_readiness_then(&rig, &again, &wire, secs(16), Some(note))?;
    assert!(!readiness.ready, "{readiness:?}");
    assert_eq!(
        readiness.answers,
        ["held: Devin has not said yet which conversation its window shows"],
        "{readiness:?}"
    );
    assert_eq!(
        (readiness.delivered, readiness.given.as_slice()),
        (None, [].as_slice()),
        "{readiness:?}"
    );
    // The window took the Escape and said nothing of its mode.
    let read = ask::devin_ready(&wire)?;
    assert_eq!((read.shown, read.mode), (None, None));
    let snapshot = again.snapshot(0)?;
    assert_eq!(
        (snapshot["typed"].as_u64(), snapshot["unsent"].as_bool()),
        (Some(1), Some(false)),
        "{snapshot}"
    );
    drop((first, again));
    rig.close()?;
    Ok(())
}

/// The live check finds and ends the command its window runs in the window's own tree,
/// what descends from the process the pane host opened it as: a process outside the
/// window that says the same words runs on. A check that ended every process whose
/// command line held its mark ended the worker that ran it, whose brief quoted the mark
/// (2026-10-16). The stand-in's command is a shell, which this rig's machine has at
/// `/bin/sh`.
#[cfg(unix)]
#[test]
fn the_check_ends_the_command_its_window_runs_and_nothing_outside_the_window_that_says_the_same(
) -> Outcome {
    use cf_e2e::live::members::{LongCommand, Machine, ThisMachine};
    use cf_e2e::process::Run;

    ask::prepare()?;
    let rig = Rig::start(devin_config(FAKE_DEVIN))?;
    let home = rig.var("HOME").unwrap_or_default().to_owned();
    let install = Installed::at(Harness::Devin, FAKE_DEVIN, &home);
    // What the window's command and the process outside it both say: the stand-in's command
    // is `sleep <seconds>; true` in a shell.
    let command = LongCommand::new("sleep 613");
    let mut outsider = Run::new("/bin/sh")
        .args(["-c", "sleep 613; true"])
        .inheriting_env()
        .spawn()?;
    let window = Open::new(&rig, &install, "work", "work-devin")
        .agent(json!({ "kind": "devin", "model": "fake" }))
        .member(Member::new("worker", "# ConsensFlow worker\n").first_message(
            "[ConsensFlow m-1 · T-1 · task from @chief]\nBACKGROUND 613 Reply with exactly: LEFT_IT",
        ))
        .open(&Log::new("live member"))?;
    // Read as the pane host answered the window's open, and kept.
    let own = window
        .process()
        .ok_or("the pane host said no process of the window, or it was gone when read")?;
    assert_eq!(
        ThisMachine.identity(own.pid)?,
        Some(own),
        "the window's process runs"
    );
    let step = Duration::from_millis(200);
    let found = until(secs(30), step, || {
        command
            .in_window(&ThisMachine, own)
            .ok()
            .filter(|found| !found.is_empty())
    })
    .ok_or("the window never ran its command")?;
    assert!(
        found
            .iter()
            .all(|process| process.identity.pid != outsider.id()),
        "{found:?}"
    );

    let ended = command.end_in(own)?;
    assert!(
        ended
            .ended
            .iter()
            .any(|process| process.command.contains("sleep 613")),
        "{ended:?}"
    );
    assert!(ended.failed.is_empty(), "{ended:?}");
    let gone = until(secs(10), step, || {
        command
            .in_window(&ThisMachine, own)
            .ok()
            .filter(Vec::is_empty)
    });
    assert!(gone.is_some(), "the window's command is gone");
    assert!(
        outsider.wait(Duration::from_millis(500))?.is_none(),
        "the process outside the window runs on"
    );
    drop(window);
    rig.close()?;
    Ok(())
}

/// A window the case opened itself (not the daemon: the rig has no request of its) is watched by
/// the folder the case gives, from the process the pane host named, and what its tree ran is seen
/// while it runs: what the live cases that open windows do to find what a closed window left
/// (`Watch::of_rig`).
#[test]
fn a_window_the_case_opened_is_watched_with_what_its_tree_runs_and_found_clean_once_it_is_closed(
) -> Outcome {
    use cf_e2e::live::members::Watch;

    ask::prepare()?;
    let rig = Rig::start(devin_config(FAKE_DEVIN))?;
    let home = rig.var("HOME").unwrap_or_default().to_owned();
    let install = Installed::at(Harness::Devin, FAKE_DEVIN, &home);
    let window = Open::new(&rig, &install, "work", "work-devin")
        .agent(json!({ "kind": "devin", "model": "fake" }))
        .member(Member::new("worker", "# ConsensFlow worker\n").first_message(
            "[ConsensFlow m-1 · T-1 · task from @chief]\nBACKGROUND 614 Reply with exactly: LEFT_IT",
        ))
        .open(&Log::new("live member"))?;
    // Looked at until the window's command is in its tree: the watch keeps what it saw.
    let mut watch = Watch::of_rig(&rig, Some(window.workspace()));
    let saw = until(secs(30), Duration::from_millis(200), || {
        watch.look_now();
        (watch.seen() >= 2).then_some(())
    });
    assert!(
        saw.is_some(),
        "the window's tree was never seen to run its command"
    );
    assert_eq!(watch.windows(), 1, "the case's own window is watched");
    drop(window);
    rig.close()?;
    let swept = watch.sweep();
    assert!(swept.problems().is_empty(), "{}", swept.said());
    // Whatever the window's close did not end is found and said, never ended, and the watch is of
    // the one window.
    assert_eq!(swept.watched.0, 1, "{}", swept.said());
    assert!(swept.watched.1 >= 2, "{}", swept.said());
    Ok(())
}

/// The folder Devin keeps its config in, in the home the rig gives its windows.
fn config_folder(rig: &Rig, home: &str) -> PathBuf {
    let base = if cfg!(windows) {
        rig.var("APPDATA").map_or_else(
            || Path::new(home).join("AppData").join("Roaming"),
            PathBuf::from,
        )
    } else {
        rig.var("XDG_CONFIG_HOME")
            .map_or_else(|| Path::new(home).join(".config"), PathBuf::from)
    };
    base.join("devin")
}

#[test]
fn a_member_launch_without_the_owners_hooks_keeps_the_ones_consensflow_adds_and_nothing_else(
) -> Outcome {
    ask::prepare()?;
    let rig = Rig::start(devin_config(FAKE_DEVIN))?;
    let home = rig.var("HOME").unwrap_or_default().to_owned();
    // The owner's own config of Devin, in the rig's home: a hook of theirs on an event of
    // its own, and one on an event ConsensFlow adds its own to.
    let owner = json!({
        "theme_mode": "dark",
        "hooks": {
            "PermissionRequest": [{ "hooks": [{ "type": "command", "command": "notify permission" }] }],
            "SessionStart": [{ "matcher": "", "hooks": [{ "type": "command", "command": "notify session" }] }],
        },
    });
    files::write(
        &config_folder(&rig, &home).join("config.json"),
        owner.to_string(),
    )?;
    let install = Installed::at(Harness::Devin, FAKE_DEVIN, &home);
    let agent = json!({ "kind": "devin", "model": "fake" });
    let launched = |id: &str, member: Member| -> Result<Value, Box<dyn std::error::Error>> {
        let window = Open::new(&rig, &install, "member", id)
            .agent(agent.clone())
            .member(member)
            .open(&Log::new("live member"))?;
        let file = file_of(window.argv(), "--config")?;
        let config: Value = serde_json::from_str(&files::read_string(&file)?)?;
        Ok(config)
    };
    let events = |config: &Value| -> Vec<String> {
        let mut names: Vec<String> = config["hooks"]
            .as_object()
            .map(|hooks| hooks.keys().cloned().collect())
            .unwrap_or_default();
        names.sort();
        names
    };
    let commands = |config: &Value, event: &str| -> Vec<String> {
        config["hooks"][event]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|entry| entry["hooks"].as_array().into_iter().flatten())
            .filter_map(|hook| hook["command"].as_str().map(str::to_owned))
            .collect()
    };

    // As the daemon launches it: the owner's hooks first and ConsensFlow's after (a
    // member's also has the hook that answers its questions from the board).
    let whole = launched("member-whole", Member::new("worker", "# worker\n"))?;
    assert_eq!(
        events(&whole),
        ["PermissionRequest", "PreToolUse", "SessionStart"]
    );
    let started = commands(&whole, "SessionStart");
    assert_eq!(started.len(), 2, "{started:?}");
    assert_eq!(started[0], "notify session");
    assert!(started[1].ends_with(" hook devin-session"), "{started:?}");

    // Alone: the owner's are not in the launch, ConsensFlow's are, and the rest of the
    // config stays.
    let alone = launched(
        "member-alone",
        Member::new("worker", "# worker\n").without_the_owners_hooks(),
    )?;
    assert_eq!(events(&alone), ["PreToolUse", "SessionStart"]);
    // The question hook runs the rig's own `cf` by its whole path, as the session's does.
    let real_cf = rig.cf().to_string_lossy().replace('\\', "/");
    assert_eq!(
        commands(&alone, "PreToolUse"),
        [format!("'{real_cf}' hook devin")]
    );
    let started = commands(&alone, "SessionStart");
    assert_eq!(started.len(), 1, "{started:?}");
    assert!(started[0].ends_with(" hook devin-session"), "{started:?}");
    assert_eq!(alone["theme_mode"], "dark");
    assert_eq!(alone["auto_update"], false);
    // The owner's own config is never touched.
    let kept: Value = serde_json::from_str(&files::read_string(
        &config_folder(&rig, &home).join("config.json"),
    )?)?;
    assert_eq!(kept, owner);
    rig.close()?;
    Ok(())
}

#[test]
fn the_launch_of_a_member_is_prepared_for_devin_and_pi_alone() -> Outcome {
    let rig = crate::rig()?;
    let home = rig.var("HOME").unwrap_or_default().to_owned();
    let claude = Installed::at(Harness::Claude, "/nowhere/claude", &home);
    let refused = Open::new(&rig, &claude, "member", "member-claude")
        .member(Member::new("worker", "# worker\n"))
        .open(&Log::new("live member"))
        .err()
        .map(|failed| failed.to_string());
    assert_eq!(
        refused.as_deref(),
        Some("a member's launch is prepared for Devin and Pi alone, not claude")
    );
    rig.close()?;
    Ok(())
}

/// The same on the wire log of Devin 3000.12.43, the installed Devin (T-1423).
#[test]
fn a_devin_members_window_is_opened_whole_cut_and_opened_again_on_its_conversation_with_the_note_in_its_launch_on_devin_3000_12(
) -> Outcome {
    on_the_installed_devin(
        a_devin_members_window_is_opened_whole_cut_and_opened_again_on_its_conversation_with_the_note_in_its_launch,
    )
}

/// The case of a window cut in its first turn on the wire log of Devin 3000.12.43, which opened
/// again so says no mode until it is typed to (T-1417's run 3, A5-23): the daemon's readiness
/// holds it a while and gives its delivery behind `/bypass` (`silent_on_3000_12`).
#[test]
fn a_window_cut_in_its_first_turn_comes_back_in_the_normal_mode_and_the_daemons_readiness_switches_it_on_devin_3000_12(
) -> Outcome {
    on_the_installed_devin(
        a_window_cut_in_its_first_turn_comes_back_in_the_normal_mode_and_the_daemons_readiness_switches_it,
    )
}
