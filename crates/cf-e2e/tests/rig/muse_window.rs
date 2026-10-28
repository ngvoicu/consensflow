//! A Muse member's window as the daemon opens one, with the machine's own Muse in it on
//! Muse's built-in fake model (`--provider echo`: offline, no login, no quota), which
//! answers each message with its words, `echo: ` before them, after the turn's delay
//! ([`cf_e2e::muse`]): the real program, its TUI, its hooks and its session log, with a
//! model that never calls a tool, asks or works in the background, and whose timing is
//! no model's. What the real model does is the live kit's (`muse-window`).
//!
//! Each case is skipped, in plain words, on a machine with no Muse Code installed: CI's,
//! and the Windows machines, which have none yet (MUS-20).

use std::time::Duration;

use cf_e2e::muse::members::{
    echo_worker, in_its_turn, last_resumed, message_with, pane_of, project, the_session, worker,
};
use cf_e2e::muse::{self, deliveries, has, messages, terminals};
use cf_e2e::rig::{Config, Project, Rig};
use serde_json::json;

use crate::{config, secs, Outcome};

/// How long an echo turn of these cases takes, unless a case says otherwise: long
/// enough for the daemon to see it at work.
const TURN_MS: u64 = 4_000;

/// A rig whose Muse windows run the machine's Muse, or why there is none.
fn muse_rig() -> Option<Config> {
    match muse::installed() {
        Ok(install) => Some(config().muse(install, TURN_MS)),
        Err(why) => {
            #[allow(clippy::print_stderr)] // Said in the output of the case it skips.
            {
                eprintln!("skipped: {why}");
            }
            None
        }
    }
}

#[test]
fn a_task_goes_from_its_brief_to_its_result_in_a_muse_window_launched_whole() -> Outcome {
    let Some(config) = muse_rig() else {
        return Ok(());
    };
    let rig = Rig::start(config)?;
    let project = project(&rig, &echo_worker())?;
    rig.tell(
        project.id(),
        "DISPATCH --tier standard Reply with exactly: T1_DONE",
    )?;
    rig.wait_for("T-1 to be answered", secs(120), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    // Its result is the window's last word: echo's, of the brief it was given.
    let task = project.task(1)?;
    let result = task.to_string();
    assert!(
        result.contains("echo: [ConsensFlow m-") && result.contains("T1_DONE"),
        "{task}"
    );
    // The window: the pinned binary, its model and effort, full permission, the owner's
    // contributor mode off, the hook, and nothing of the brief: it is pasted once the
    // window is up, never on a command line every process reads.
    let launches = rig.muse_launches();
    assert_eq!(launches.len(), 1, "{launches:?}");
    let argv = &launches[0];
    for words in [
        ["--model", "muse-spark-1.3"],
        ["--reasoning-effort", "minimal"],
        ["--contributor-mode", "off"],
    ] {
        assert!(
            argv.windows(2).any(|pair| pair == words),
            "{words:?} in {argv:?}"
        );
    }
    assert!(argv.iter().any(|word| word == "--yolo"), "{argv:?}");
    assert!(argv
        .iter()
        .any(|word| word.starts_with("hooks=") && word.contains("hook muse-session")));
    assert!(
        !argv.iter().any(|word| word.contains("T1_DONE")),
        "{argv:?}"
    );
    // Its session: one turn, its role text given by the hook, ended.
    let (_, log) = the_session(&rig);
    assert_eq!(terminals(&log), ["completed"]);
    assert!(log.iter().any(|record| {
        muse::kind(record) == "run/context_block_updated"
            && record["payload"]["event"]["id"] == "hook:session_start:session:0"
            && record["payload"]["event"]["text"]
                .as_str()
                .is_some_and(|text| text.contains("ConsensFlow"))
    }));
    rig.close()?;
    Ok(())
}

/// A rig whose Muse turns are `turn_ms` long.
fn muse_rig_of(turn_ms: u64) -> Option<Config> {
    muse_rig().map(|config| config.var("CF_TEST_MUSE_DELAY_MS", turn_ms.to_string()))
}

#[test]
fn an_interrupt_before_the_first_word_takes_the_brief_back_and_the_resume_goes_in_alone() -> Outcome
{
    let Some(config) = muse_rig_of(20_000) else {
        return Ok(());
    };
    let rig = Rig::start(config)?;
    let project = project(&rig, &echo_worker())?;
    rig.tell(
        project.id(),
        "DISPATCH --tier standard Reply with exactly: T1_DONE",
    )?;
    in_its_turn(&rig, &project, 1, secs(90))?;
    let pane = pane_of(&project)?;
    let paused = rig.page("task.pause", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(paused["ok"], true, "{paused}");
    rig.wait_for("T-1 to be paused", secs(60), || {
        Ok(project.task(1)?["state"] == "paused")
    })?;
    // One Escape stopped the turn before its first word, and Muse put the brief back.
    let id = pane["id"].as_str().unwrap_or_default();
    rig.wait_for("Muse to write the turn taken back", secs(30), || {
        Ok(has(&the_session(&rig).1, "run/run_retracted"))
    })?;
    assert_eq!(rig.keys_pressed(id, &[27]), 1);
    assert_eq!(terminals(&the_session(&rig).1), ["cancelled"]);
    // The resume pastes the brief again, alone: the box was emptied of the one put back.
    let resumed = rig.page("task.resume", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    rig.wait_for("T-1 to be answered", secs(120), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let (_, log) = the_session(&rig);
    let words = messages(&log);
    assert_eq!(words.len(), 2, "{words:?}");
    assert_eq!(
        words[1].matches("Reply with exactly: T1_DONE").count(),
        1,
        "{words:?}"
    );
    assert_eq!(terminals(&log), ["cancelled", "completed"]);
    assert_eq!(
        rig.keys_pressed(id, &[3]),
        1,
        "the box emptied once, of the brief put back"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_tell_while_the_turn_works_stops_it_takes_the_brief_back_and_goes_in_alone_once() -> Outcome {
    let Some(config) = muse_rig_of(20_000) else {
        return Ok(());
    };
    let rig = Rig::start(config)?;
    let project = project(&rig, &echo_worker())?;
    rig.tell(
        project.id(),
        "DISPATCH --tier standard Reply with exactly: T1_DONE",
    )?;
    in_its_turn(&rig, &project, 1, secs(90))?;
    let worker = worker(&project)?;
    // The chief's tell into a task at work pauses it for the question: the daemon stops
    // the turn, Muse puts the brief back before a word of its answer, and the question
    // goes in on its own once the window rests.
    rig.tell(project.id(), "CF tell T-1 :: Also say HANDED_OVER.")?;
    let mut told = None;
    rig.wait_for("the tell to be received", secs(90), || {
        told = message_with(&project, &worker, "HANDED_OVER")?
            .filter(|message| message["state"] == "delivered");
        Ok(told.is_some())
    })?;
    let told = told.unwrap_or_default();
    assert_eq!(told["attempts"], 1, "{told}");
    let brief = message_with(&project, &worker, "T1_DONE")?.unwrap_or_default();
    assert_eq!(
        brief["state"], "queued",
        "taken back, for the resume: {brief}"
    );
    let pane = pane_of(&project)?;
    assert_eq!(
        rig.keys_pressed(pane["id"].as_str().unwrap_or_default(), &[27]),
        1
    );
    rig.wait_for("the question's turn to end", secs(60), || {
        Ok(terminals(&the_session(&rig).1).len() == 2)
    })?;
    let (_, log) = the_session(&rig);
    assert!(has(&log, "run/run_retracted"));
    assert_eq!(terminals(&log), ["cancelled", "completed"]);
    assert_eq!(
        deliveries(&log),
        ["top_level_turn_started", "top_level_turn_started"]
    );
    let words = messages(&log);
    assert_eq!(
        words
            .iter()
            .filter(|words| words.contains("HANDED_OVER"))
            .count(),
        1,
        "{words:?}"
    );
    assert!(
        !words[1].contains("T1_DONE"),
        "the brief put back was cleared first: {words:?}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn words_that_land_in_a_turn_at_work_are_the_turn_s_at_once_and_its_task_ends_once() -> Outcome {
    let Some(config) = muse_rig_of(20_000) else {
        return Ok(());
    };
    let rig = Rig::start(config)?;
    let project = project(&rig, &echo_worker())?;
    rig.tell(
        project.id(),
        "DISPATCH --tier standard Reply with exactly: T1_DONE",
    )?;
    in_its_turn(&rig, &project, 1, secs(90))?;
    // A paste that reaches the window while its turn works, as one the daemon began at a
    // look that found it at rest does: Muse steers the running turn with it, and writes it
    // at once, while that turn still works, so the record shows the hand-over.
    let pane = pane_of(&project)?;
    let typed = "\u{1b}[200~Steer: say MID_TURN too.\u{1b}[201~\r";
    let answer = rig.host(
        "pane.input",
        json!({ "id": pane["id"], "generation": pane["generation"], "bytes": typed.as_bytes() }),
    )?;
    assert_eq!(answer["ok"], true, "{answer}");
    rig.wait_for("Muse to take the words in", secs(30), || {
        Ok(deliveries(&the_session(&rig).1).len() == 2)
    })?;
    let (_, log) = the_session(&rig);
    assert_eq!(
        deliveries(&log),
        ["top_level_turn_started", "active_run_delivery"]
    );
    assert!(
        terminals(&log).is_empty(),
        "the first turn still works: {:?}",
        terminals(&log)
    );
    rig.wait_for("T-1 to be answered", secs(120), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    assert_eq!(rig.muse_launches().len(), 1);
    rig.close()?;
    Ok(())
}

#[test]
fn a_muse_worker_cut_by_the_app_s_quit_is_resumed_on_its_session_with_the_note_pasted_once_up(
) -> Outcome {
    let Some(config) = muse_rig_of(30_000) else {
        return Ok(());
    };
    let rig = Rig::start(config.clone())?;
    let project = project(&rig, &echo_worker())?;
    let id = project.id();
    rig.tell(id, "DISPATCH --tier standard Reply with exactly: T1_DONE")?;
    in_its_turn(&rig, &project, 1, secs(90))?;
    let worker = worker(&project)?;
    let (session, _) = the_session(&rig);
    let root = rig.close_keeping_the_root()?;
    let rig = Rig::start(config.existing_root(root))?;
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    let project = Project::new(&rig, id);
    let mut received = None;
    rig.wait_for("the worker's note to be received", secs(120), || {
        received = message_with(&project, &worker, "Go on where you stopped.")?
            .filter(|message| message["state"] == "delivered");
        Ok(received.is_some())
    })?;
    assert_eq!(received.unwrap_or_default()["attempts"], 1);
    rig.wait_for("T-1 to be answered", secs(120), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    // The window opened again with `muse resume` on the session, the note not on its
    // command line but pasted once it was up, into the same conversation.
    let launches = rig.muse_launches();
    let again = launches.last().unwrap();
    assert!(
        again
            .windows(2)
            .any(|pair| pair == ["resume", session.as_str()]),
        "{again:?}"
    );
    assert!(
        !again
            .iter()
            .any(|word| word.contains("Go on where you stopped.")),
        "{again:?}"
    );
    let (same, log) = the_session(&rig);
    assert_eq!(same, session);
    assert!(messages(&log)
        .iter()
        .any(|words| words.contains("Go on where you stopped.")));
    rig.close()?;
    Ok(())
}

#[test]
fn a_task_put_after_another_opens_the_worker_again_on_the_same_conversation() -> Outcome {
    let Some(config) = muse_rig_of(2_000) else {
        return Ok(());
    };
    let rig = Rig::start(config)?;
    let project = project(&rig, &echo_worker())?;
    rig.tell(
        project.id(),
        "DISPATCH --tier standard Reply with exactly: T1_DONE",
    )?;
    rig.wait_for("T-1 to be answered", secs(120), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let (session, _) = the_session(&rig);
    rig.tell(
        project.id(),
        "CF task accept T-1\nDISPATCH --tier standard --after T-1 Reply with exactly: T2_DONE",
    )?;
    rig.wait_for("T-2 to be answered", secs(120), || {
        Ok(project.task(2)?["state"] == "done")
    })?;
    // The window went with T-1; T-2's opened again on its conversation, its brief
    // pasted once up, as `muse resume` takes none.
    assert_eq!(rig.muse_launches().len(), 2);
    assert!(last_resumed(&rig, &session), "{:?}", rig.muse_launches());
    let (same, log) = the_session(&rig);
    assert_eq!(same, session);
    let words = messages(&log);
    assert_eq!(words.len(), 2, "{words:?}");
    assert!(
        words[1].contains("T2_DONE") && !words[1].contains("T1_DONE"),
        "{words:?}"
    );
    assert_eq!(terminals(&log), ["completed", "completed"]);
    rig.close()?;
    Ok(())
}

#[test]
fn a_conversation_the_app_quit_at_rest_is_gone_on_with_after_the_restart() -> Outcome {
    let Some(config) = muse_rig_of(2_000) else {
        return Ok(());
    };
    let rig = Rig::start(config.clone())?;
    let project = project(&rig, &echo_worker())?;
    let id = project.id();
    rig.tell(id, "DISPATCH --tier standard Reply with exactly: T1_DONE")?;
    rig.wait_for("T-1 to be answered", secs(120), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    // Its window closes with its task, and the pane host proves its Muse gone: the app
    // quits with nothing of the conversation's running.
    let handle = worker(&project)?;
    rig.wait_for("the worker's window to close", secs(60), || {
        Ok(project.lane(&handle)?["pane"].is_null())
    })?;
    let (session, _) = the_session(&rig);
    let root = rig.close_keeping_the_root()?;
    let rig = Rig::start(config.existing_root(root))?;
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    let project = Project::new(&rig, id);
    rig.tell(
        id,
        "CF task accept T-1\nDISPATCH --tier standard --after T-1 Reply with exactly: T2_DONE",
    )?;
    rig.wait_for("T-2 to be answered", secs(120), || {
        Ok(project.task(2)?["state"] == "done")
    })?;
    // The Muse the quit ended held the session's lock: the next one took it, and went on.
    assert!(last_resumed(&rig, &session), "{:?}", rig.muse_launches());
    let (same, log) = the_session(&rig);
    assert_eq!(same, session);
    let words = messages(&log);
    assert!(
        words.last().is_some_and(|last| last.contains("T2_DONE")),
        "{words:?}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_stop_that_finds_the_muse_window_at_rest_is_paid_with_no_key() -> Outcome {
    let Some(config) = muse_rig_of(20_000) else {
        return Ok(());
    };
    let rig = Rig::start(config)?;
    let project = project(&rig, &echo_worker())?;
    rig.tell(
        project.id(),
        "DISPATCH --tier standard Reply with exactly: T1_DONE",
    )?;
    in_its_turn(&rig, &project, 1, secs(90))?;
    // The human's own Escape stops the turn before its first word: the window rests.
    let pane = pane_of(&project)?;
    rig.types(&pane, &[27])?;
    rig.wait_for("Muse to write the turn taken back", secs(30), || {
        Ok(has(&the_session(&rig).1, "run/run_retracted"))
    })?;
    std::thread::sleep(Duration::from_secs(3));
    let paused = rig.page("task.pause", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(paused["ok"], true, "{paused}");
    rig.wait_for("T-1 to be paused", secs(60), || {
        Ok(project.task(1)?["state"] == "paused")
    })?;
    std::thread::sleep(Duration::from_secs(5));
    let id = pane["id"].as_str().unwrap_or_default();
    assert_eq!(rig.keys_pressed(id, &[27]), 0, "the daemon pressed nothing");
    assert_eq!(
        terminals(&the_session(&rig).1),
        ["cancelled"],
        "no second stop"
    );
    assert!(pane_of(&project)?.is_object(), "the window is open");
    rig.close()?;
    Ok(())
}
