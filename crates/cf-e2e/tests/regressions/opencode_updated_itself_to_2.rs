//! The owner's OpenCode updated itself to 2.0.29 (2026-10-15, ConsensFlow 3.0.4):
//! **every OpenCode window ConsensFlow opened hung.**
//!
//! What the owner saw: a 1.x OpenCode updates itself to the latest release when it
//! starts, and theirs went from 1.19.0 to 2.0.29. OpenCode 2 is a redesign. The
//! flags ConsensFlow launches it with (`--port`, `--hostname`, `--model`) are gone,
//! its `--help` lists `--standalone`, `--server`, `--auto`, `--continue`, `--session`
//! and `--prompt`, a shared `opencode service` stands where each window's server
//! was, and `--version` says `opencode v2.0.29` where 1.x said `1.19.0`. Every
//! OpenCode window the daemon opened then hung: the live suites `approval`, `specs`
//! and `resume-fresh` timed out "waiting for the chief's window to read idle", and
//! `finish` never saw the worker's window. Nothing told the human why. The report is
//! the issue `issues/opencode-2-breaks-every-window.md` in consensflow-sme.
//!
//! What the owner should have seen (their decision of the same day, for 3.0.5:
//! OpenCode 2 is not supported yet, and 1.x goes on as it was): wherever the human
//! meets an OpenCode 2, the board says it is not supported yet and to install the
//! last 1.x: a chief that cannot start says so in the note the human is given, and a
//! worker's task fails with it, which its requester is told. No server, conversation
//! or window is made on it, so nothing waits.
//!
//! The stand-in (`fake-opencode-2`) says `opencode v2.0.29` to `--version`, writes down
//! everything it is asked, and answers nothing else: it takes the flags OpenCode 2 no
//! longer takes and never listens, so a conversation made on its `serve` is never
//! made, and a window on it never says what it shows. It is no real OpenCode 2, whose
//! own answer to those flags is not known: the cases are the daemon's side, which is
//! the part that is code. Without the refusal the daemon asks for a conversation, waits
//! the fifteen seconds it gives one, and tells the human something else; so a case
//! waits ten for the words, and fails there. What proves it holds is the stand-in's own
//! word for what it was asked: the version, and nothing more.

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::{config, secs, Outcome, FAKE_OPENCODE_2};

/// What the human and the requester are told of an OpenCode that is 2 or more.
const WORDS: &str =
    "OpenCode 2 or newer is not supported yet. Install the last OpenCode 1.x release";

/// The model of the stand-in agents; OpenCode's own are `provider/model`.
const MODEL: &str = "opencode/muse-spark-1.3-contributor-free";

/// The roster of the case: `chief` is the agent the chief runs on, on the stand-in
/// OpenCode 2 (`"opencode"`) or the stand-in Claude (`"claude-code"`), and the
/// worker is the other.
fn write_roster(rig: &Rig, chief: &str, worker: &str) -> cf_e2e::Result {
    let agent = |id: &str, kind: &str| {
        let model = if kind == "opencode" { MODEL } else { "fake" };
        json!({ "id": id, "kind": kind, "model": model, "workTier": "standard" })
    };
    let roster = json!({
        "schemaVersion": 1,
        "agents": [agent("chief", chief), agent("worker", worker)],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))
}

/// The messages of `inbox`, whose body holds `words`.
fn saying<'a>(inbox: &'a [Value], words: &str) -> Vec<&'a Value> {
    inbox
        .iter()
        .filter(|message| {
            message["body"]
                .as_str()
                .is_some_and(|body| body.contains(words))
        })
        .collect()
}

/// The human's inbox of the project.
fn human(rig: &Rig, project: &Project) -> Vec<Value> {
    let inbox = rig.page("inbox.get", json!({ "project": project.id() }));
    inbox
        .ok()
        .and_then(|answer| answer["messages"].as_array().cloned())
        .unwrap_or_default()
}

/// What the stand-in OpenCode 2 was asked: only its version, and at least once.
fn assert_only_asked_its_version(rig: &Rig) {
    let asked = rig.opencode_asked();
    assert!(!asked.is_empty(), "the daemon never asked its version");
    assert!(
        asked.iter().all(|args| args == &["--version"]),
        "the daemon asked more than the version: {asked:?}"
    );
}

#[test]
fn a_chief_on_an_opencode_that_says_it_is_2_is_said_not_to_start_and_no_window_is_made_on_it(
) -> Outcome {
    let rig = Rig::start(config().opencode_2(FAKE_OPENCODE_2))?;
    write_roster(&rig, "opencode", "claude-code")?;
    let project = Project::open(&rig, "chief", json!({}))?;
    // Told at once, and not when a conversation that is never made is given up on.
    rig.wait_for(
        "the human to be told why the chief did not start",
        secs(10),
        || Ok(!saying(&human(&rig, &project), WORDS).is_empty()),
    )?;
    let inbox = human(&rig, &project);
    let told = saying(&inbox, WORDS);
    assert_eq!(told.len(), 1, "the human hears why once: {inbox:?}");
    let body = told[0]["body"].as_str().unwrap_or_default();
    assert!(
        body.starts_with("The chief could not start: the launch failed: OpenCode 2 or newer"),
        "{body}"
    );
    assert!(
        !body.contains(".."),
        "a refusal ends no sentence twice: {body}"
    );
    // No window was opened, and no server was started for a conversation: the
    // stand-in was asked its version, and nothing else.
    let windows = rig.open_frames();
    assert!(windows.is_empty(), "a window was opened: {windows:?}");
    assert_only_asked_its_version(&rig);
    rig.close()?;
    Ok(())
}

#[test]
fn a_task_for_a_worker_on_an_opencode_that_says_it_is_2_fails_in_those_words_and_its_requester_is_told(
) -> Outcome {
    let rig = Rig::start(config().opencode_2(FAKE_OPENCODE_2))?;
    write_roster(&rig, "claude-code", "opencode")?;
    let project = Project::open(&rig, "chief", json!({}))?;
    project.add_member("worker")?;
    rig.tell(
        project.id(),
        "DISPATCH --tier standard Reply with exactly: NEVER_SAID",
    )?;
    rig.wait_for("T-1 to fail", secs(10), || {
        Ok(project.task(1)?["state"] == "failed")
    })?;
    // The chief that gave it is told why, as it is of any task that cannot be given.
    let inbox = project.inbox("chief")?;
    let told = saying(&inbox, WORDS);
    assert_eq!(told.len(), 1, "the chief hears why once: {inbox:?}");
    let body = told[0]["body"].as_str().unwrap_or_default();
    assert!(
        body.starts_with("T-1 failed: the launch failed: OpenCode 2 or newer"),
        "{body}"
    );
    assert!(
        !body.contains(".."),
        "a refusal ends no sentence twice: {body}"
    );
    // No window was opened for the worker, the chief's is the only one there is, and
    // nothing else of OpenCode was run.
    let chief = format!("p{}-chief", project.id());
    let windows = rig.open_frames();
    assert!(
        windows.iter().all(|frame| frame["id"] == chief.as_str()),
        "a window was opened for the worker: {windows:?}"
    );
    assert_only_asked_its_version(&rig);
    rig.close()?;
    Ok(())
}
