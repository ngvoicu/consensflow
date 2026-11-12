//! A silent Devin window the human typed into keeps its delivery queued, and spends none of its
//! attempts (T-1474, the review T-1482's "last attempt": an attempt spent for each refusal gave
//! the message up on the third, and a later configuration could not bring it back).
//!
//! Devin 3000.12.43, opened again with no message on a session whose last turn a quit cut,
//! writes nothing of its mode until it is typed to, while every line of its replay names its
//! conversation (alpha-5's A5-23). The daemon takes such a window as on its conversation 10 s
//! after the first look that found it so, and gives it its deliveries behind `/bypass`
//! (T-1432). Nothing in its log tells it from a window a person took to a new conversation with
//! a `/new` that wrote no line, and only a person types into a member's window: so once the
//! pane host's count says a key of the human's reached it, the window is held as one that has
//! said nothing, until its log configures. The hold is decided at the readiness, before a
//! delivery is admitted: the message waits at the head of the window's queue, with no attempt
//! spent and nothing given up, however many looks it waits; once the log configures, it goes in
//! once.
//!
//! The case is the field's on the rig, on 3000.12's shape (the stand-in silent as the
//! recordings): T-1 is done, and its worker's window at work on a long message of the human's
//! when the app quits, daemon first; the app starts again on the same home, and the human opens
//! the worker's window with Show (opened on the cut session with no message); the human presses
//! Escape in it (the rig's human door, `pane.typed`); past the 10 s, the chief gives T-1 back to
//! it. The control, with no key, is the same window given it behind `/bypass`.

use std::path::PathBuf;

use cf_e2e::devin_store;
use cf_e2e::rig::{Project, Rig};
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

use crate::devin_notes_twice::store_of;
use crate::devin_pause::write_roster;
use crate::{config, secs, Outcome, FAKE_DEVIN};

/// What the chief gives T-1 back with, whose answer says it ran.
const TOLD: &str = "Reply with exactly: T1_TOLD";

/// How long the follow-up are watched waiting: the daemon looks at a window about once a
/// second, so many looks, each of which spent an attempt where the window refused it.
const WATCHED: u64 = 20;

fn a_rig() -> cf_e2e::rig::Config {
    config()
        .devin(FAKE_DEVIN)
        .var("FAKE_DEVIN_STAMPS", "3000.12")
}

/// The ids of the messages to `handle`.
fn ids(project: &Project, handle: &str) -> Result<Vec<i64>> {
    Ok(project
        .inbox(handle)?
        .iter()
        .filter_map(|message| message["id"].as_i64())
        .collect())
}

/// The messages to `handle` that are none of `before`: the follow-up'.
fn since(project: &Project, handle: &str, before: &[i64]) -> Result<Vec<Value>> {
    Ok(project
        .inbox(handle)?
        .into_iter()
        .filter(|message| {
            message["id"]
                .as_i64()
                .is_some_and(|id| !before.contains(&id))
        })
        .collect())
}

/// Whether the chief has T-1's answer to the follow-up.
fn answered(project: &Project) -> Result<bool> {
    Ok(project
        .inbox("chief")?
        .iter()
        .any(|message| message["kind"] == "result" && message["body"] == "T1_TOLD"))
}

/// The user's messages of Devin's store that hold the follow-up.
fn told_in(store: &std::path::Path) -> Result<Vec<String>> {
    Ok(devin_store::messages(store)?
        .into_iter()
        .filter(|message| message.role == "user" && message.text.contains(TOLD))
        .map(|message| message.text)
        .collect())
}

/// What a quit left and the next start opened (beside the rig started again, which goes back as
/// a `Rig` of its own so that the rig suite's scan sees it handed on): its project, the worker's
/// handle and window, and Devin's store.
struct Shown {
    project: i64,
    handle: String,
    pane: Value,
    store: PathBuf,
}

/// Opens `handle`'s window with Show where it has none, and waits for it to read at rest.
fn shown(rig: &Rig, project: &Project, handle: &str) -> Result<Value> {
    if !project.lane(handle)?["pane"].is_object() {
        let opened = rig.page(
            "session.open",
            json!({ "project": project.id(), "handle": handle }),
        )?;
        assert_eq!(opened["ok"], true, "{opened}");
    }
    rig.wait_for("the worker's window to be up", secs(60), || {
        Ok(project.lane(handle)?["pane"].is_object())
    })?;
    rig.wait_for("the worker's window to read at rest", secs(60), || {
        Ok(project.lane(handle)?["activity"]["state"] == "idle")
    })?;
    Ok(project.lane(handle)?["pane"].clone())
}

/// T-1 is done; the human sends a long message in its worker's window, and the app quits, daemon
/// first, with that turn at work. The app starts again on the same home, and the human opens the
/// worker's window with Show: Devin opens it on the cut session with no message, silent.
fn cut_by_a_quit_and_shown() -> Result<(Rig, Shown)> {
    let mut rig = Rig::start(a_rig())?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    project.add_member("devin-worker")?;
    rig.tell(
        project.id(),
        "DISPATCH --tier standard Reply with exactly: T1_DONE",
    )?;
    rig.wait_for("T-1 to be done", secs(120), || {
        let state = project.task(1)?["state"].clone();
        Ok(state == "done" || state == "accepted")
    })?;
    let handle = project.task(1)?["assignee"]
        .as_str()
        .ok_or_else(|| Error::Daemon("T-1 has no assignee".to_owned()))?
        .to_owned();
    let pane = shown(&rig, &project, &handle)?;
    rig.types(
        &pane,
        "\u{1b}[200~SLEEP 600 Reply with exactly: LATE\u{1b}[201~\r".as_bytes(),
    )?;
    let store = store_of(&rig);
    rig.wait_for("the human's turn to be at work", secs(60), || {
        Ok(devin_store::messages(&store)
            .unwrap_or_default()
            .iter()
            .any(|message| message.role == "user" && message.text.contains("SLEEP 600")))
    })?;
    let id = project.id();
    rig.kill_daemon();
    rig.wait_for_daemon_exit(secs(10))?;
    let root = rig.close_keeping_the_root()?;

    let rig = Rig::start(a_rig().existing_root(root))?;
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    let pane = shown(&rig, &Project::new(&rig, id), &handle)?;
    Ok((
        rig,
        Shown {
            project: id,
            handle,
            pane,
            store,
        },
    ))
}

/// The chief gives T-1 back to its worker with `TOLD`, as `cf` does in its window.
fn tell_t1(rig: &Rig, project: &Project) -> Result<()> {
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), secs(30))?;
    let given = if project.task(1)?["state"] == "done" {
        rig.cf_in_window(&chief, ["task", "reopen", "T-1", TOLD])?
    } else {
        rig.cf_in_window(&chief, ["task", "add", "--after", "T-1", TOLD])?
    };
    if given.code == Some(0) {
        Ok(())
    } else {
        Err(Error::Daemon(format!(
            "the tell was refused: {}",
            given.output()
        )))
    }
}

#[test]
fn a_silent_devin_window_the_human_typed_into_keeps_what_it_is_told_queued_with_no_attempt_spent_until_its_log_configures(
) -> Outcome {
    let (
        rig,
        Shown {
            project,
            handle,
            pane,
            store,
        },
    ) = cut_by_a_quit_and_shown()?;
    let project = Project::new(&rig, project);
    let before = ids(&project, &handle)?;

    // The human presses Escape in it, and the window's while passes.
    rig.types(&pane, b"\x1b")?;
    std::thread::sleep(secs(12));
    tell_t1(&rig, &project)?;
    rig.wait_for("the follow-up to be made", secs(30), || {
        Ok(!since(&project, &handle, &before)?.is_empty())
    })?;

    // Look after look, they wait: queued, no attempt spent, nothing given up or given.
    for second in 0..WATCHED {
        std::thread::sleep(secs(1));
        let messages = since(&project, &handle, &before)?;
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert_eq!(
            (&messages[0]["state"], &messages[0]["attempts"]),
            (&json!("queued"), &json!(0)),
            "after {second} s: {}; the lane: {}",
            messages[0],
            project.lane(&handle)?
        );
        assert!(told_in(&store)?.is_empty(), "after {second} s");
    }
    assert!(!answered(&project)?);

    // The human sends a message there: Devin writes its configuration as the turn begins, and
    // once it has answered, the follow-up go in, once.
    rig.types(
        &pane,
        "\u{1b}[200~Reply with exactly: HUMAN_DONE\u{1b}[201~\r".as_bytes(),
    )?;
    rig.wait_for("T-1 to answer the follow-up", secs(120), || {
        answered(&project)
    })?;
    let messages = since(&project, &handle, &before)?;
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(
        (&messages[0]["state"], &messages[0]["attempts"]),
        (&json!("delivered"), &json!(1)),
        "{messages:?}"
    );
    assert_eq!(told_in(&store)?.len(), 1, "{:?}", told_in(&store)?);
    rig.close()?;
    Ok(())
}

/// The control: nobody typed into the window, and what it is told goes in behind `/bypass`,
/// once.
#[test]
fn a_silent_devin_window_nobody_typed_into_is_given_what_it_is_told_behind_bypass_once() -> Outcome
{
    let (
        rig,
        Shown {
            project,
            handle,
            store,
            ..
        },
    ) = cut_by_a_quit_and_shown()?;
    let project = Project::new(&rig, project);
    let before = ids(&project, &handle)?;
    std::thread::sleep(secs(12));
    tell_t1(&rig, &project)?;
    rig.wait_for("T-1 to answer the follow-up", secs(120), || {
        answered(&project)
    })?;
    let messages = since(&project, &handle, &before)?;
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(
        (&messages[0]["state"], &messages[0]["attempts"]),
        (&json!("delivered"), &json!(1)),
        "{messages:?}"
    );
    let told = told_in(&store)?;
    assert_eq!(told.len(), 1, "{told:?}");
    let given: Vec<String> = devin_store::history(&store)?
        .into_iter()
        .map(|(_, words)| words)
        .filter(|words| words.contains(TOLD))
        .collect();
    assert!(
        !given.is_empty() && given.iter().all(|words| words.starts_with("/bypass ")),
        "{given:?}"
    );
    rig.close()?;
    Ok(())
}
