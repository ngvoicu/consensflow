//! A field report of 2026-11-03, ConsensFlow 3.0.7-alpha.1, Gabriel's other board (board
//! task T-1199, his bugs 9 and 10): **a reopen, and then a quota wait, interrupted a window
//! and ConsensFlow closed it, leaving its task paused.**
//!
//! Bug 9, what the chief saw: T-21 had been held for quota once (m-444) and had gone on by
//! itself (m-446), and was done (its result m-447). `cf task reopen T-21 "…"` (a one-line
//! README fix) was answered by m-450, "T-21's window (@sepa-keen-quarry) did not stop: it
//! ignored the interrupt, so ConsensFlow closed it. T-21 stays paused." The window was still
//! on its turn after its result. The chief had to resend the words with `cf task resume`.
//!
//! Bug 10, right after: the resumed T-21 was refused for quota, and its requester was told
//! "T-21 waits … out of quota until 16:13:34Z … it goes on by itself" (m-453). Then m-455:
//! "T-21's window did not stop: it ignored the interrupt, so ConsensFlow closed it. T-21
//! stays paused." It went the same way a second time (m-457, m-458), and the follow-up was
//! never carried out.
//!
//! Why: a hold for quota was a pause that asked the window to stop (`cf_ledger`'s `pause`
//! counted it), though the refusal had ended the turn already. A window that read at work
//! while it waited (Claude Code's "continuing automatically · esc to cancel") was pressed
//! three rounds of Escape, ignored them, and was closed, as any window that ignored a
//! pause; and a hold's stop that no look paid stayed owed while the task was done
//! (`stop_of` counts no stop of a done task) and came due when the reopen put the task back
//! in the window's hands. The engine's tests hold that shape (`dispatcher/stops.rs`, a
//! harness at work while its member is out); the rig's stand-in refuses on a record its
//! reader reads at rest, so these cases are the field's sequence and assert what the owner
//! should have seen, and the stops they would have pressed are the dispatcher's to show.
//!
//! What should have happened: only a pause, a tell, a cancel or the human's own stop asks a
//! window to stop. A reopen's words wait for the turn at work to end and then go in; a quota
//! wait never interrupts, closes or pauses: when quota is back the words that wait go in and
//! the task goes on, as its note says. And a pause never closes a window (Gabriel: "pause
//! should never close the window").

use std::path::{Path, PathBuf};

use cf_e2e::files;
use cf_e2e::rig::{Config, Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::{config, secs, Outcome};

/// `cf` as the chief runs it in its window; a `cf` that fails is an error.
fn cf(rig: &Rig, chief: &Value, words: &[&str]) -> cf_e2e::Result<String> {
    let ran = rig.cf_in_window(chief, words)?;
    if ran.code == Some(0) {
        Ok(ran.stdout)
    } else {
        Err(Error::Daemon(format!("cf {}: {ran}", words.join(" "))))
    }
}

fn state(project: &Project, number: i64) -> cf_e2e::Result<String> {
    Ok(project.task(number)?["state"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

/// The windows of the project's worker the daemon opened so far, oldest first.
fn workers_windows(rig: &Rig, project: &Project) -> Vec<Value> {
    let chief = format!("p{}-chief", project.id());
    rig.open_frames()
        .into_iter()
        .filter(|frame| frame["id"] != chief.as_str())
        .collect()
}

/// Every note the human or the chief was given or is to be given that says a window did
/// not stop or was closed.
fn stop_notes(rig: &Rig, project: &Project) -> cf_e2e::Result<Vec<String>> {
    let human = rig.page("inbox.get", json!({ "project": project.id() }))?["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    Ok(human
        .into_iter()
        .chain(project.inbox("chief")?)
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .filter(|body| body.contains("did not stop") || body.contains("closed it"))
        .collect())
}

/// The bodies of the results the chief was given, oldest first.
fn results(project: &Project) -> cf_e2e::Result<Vec<String>> {
    let mut given: Vec<Value> = project
        .inbox("chief")?
        .into_iter()
        .filter(|m| m["kind"] == "result" && m["state"] == "delivered")
        .collect();
    given.sort_by_key(|m| m["id"].as_i64().unwrap_or_default());
    Ok(given
        .into_iter()
        .filter_map(|m| m["body"].as_str().map(str::to_owned))
        .collect())
}

/// How many notes the chief has that say T-1 waits and goes on by itself.
fn waits_told(project: &Project) -> cf_e2e::Result<usize> {
    Ok(project
        .inbox("chief")?
        .iter()
        .filter(|m| {
            m["kind"] == "note"
                && m["body"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("it goes on by itself")
        })
        .count())
}

/// T-1's moves as the ledger logged them, for a failure to say what happened.
fn moves(rig: &Rig) -> Vec<String> {
    rig.events()
        .into_iter()
        .filter(|event| event["kind"] == "task.state" && event["data"]["task"] == 1)
        .map(|event| {
            let data = &event["data"];
            format!(
                "{} {}→{} stop={} by={} because={}",
                event["at"], data["from"], data["to"], data["stop"], data["by"], data["because"]
            )
        })
        .collect()
}

/// An ISO time as the ledger writes one (`2026-11-03T17:11:52.249Z`), in ms since the
/// epoch.
fn ms_of(at: &str) -> Option<i64> {
    let (date, time) = at.strip_suffix('Z')?.split_once('T')?;
    let mut ymd = date.split('-').map(|part| part.parse::<i64>().ok());
    let (y, m, d) = (ymd.next()??, ymd.next()??, ymd.next()??);
    let (hms, ms) = time.split_once('.').unwrap_or((time, "0"));
    let mut parts = hms.split(':').map(|part| part.parse::<i64>().ok());
    let (h, min, sec) = (parts.next()??, parts.next()??, parts.next()??);
    // Days since the epoch of a Gregorian date (Howard Hinnant's `days_from_civil`).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + h) * 60 + min) * 60_000 + sec * 1_000 + ms.parse::<i64>().ok()?)
}

/// When T-1 moved, by the ledger's log: each move's time and the state it moved to,
/// oldest first.
fn moved_at(rig: &Rig) -> Vec<(i64, String)> {
    rig.events()
        .into_iter()
        .filter(|event| event["kind"] == "task.state" && event["data"]["task"] == 1)
        .filter_map(|event| {
            let at = ms_of(event["at"].as_str()?)?;
            Some((at, event["data"]["to"].as_str()?.to_owned()))
        })
        .collect()
}

/// Nothing stopped the worker: no Escape was pressed into any window of it, nobody was
/// told one did not stop or was closed, and none of its windows ended while T-1 was still
/// to be done in it (once T-1 is done, a window that holds nothing closes by itself, and
/// a reopen brings it back on its conversation).
fn never_stopped(rig: &Rig, project: &Project) -> cf_e2e::Result<()> {
    let worker = workers_windows(rig, project);
    for window in &worker {
        let id = window["id"].as_str().unwrap_or_default();
        assert_eq!(
            rig.keys_pressed(id, &[27]),
            0,
            "the daemon pressed Escape into the worker's window; T-1 moved: {:#?}",
            moves(rig)
        );
    }
    let notes = stop_notes(rig, project)?;
    assert!(
        notes.is_empty(),
        "told: {notes:?}; T-1 moved: {:#?}",
        moves(rig)
    );
    let moved = moved_at(rig);
    for (exit, at) in rig.timed_exits() {
        if !worker.iter().any(|window| window["id"] == exit["id"]) {
            continue;
        }
        let then = moved
            .iter()
            .take_while(|(when, _)| *when <= at)
            .last()
            .map(|(_, state)| state.as_str());
        assert!(
            matches!(then, Some("done" | "accepted")),
            "a window of the worker ended while T-1 was {then:?}: {exit}; T-1 moved: {:#?}",
            moves(rig)
        );
    }
    Ok(())
}

/// No pause but a hold's: every move of T-1 to paused says it is out of quota.
fn paused_only_for_quota(rig: &Rig) {
    let pauses: Vec<String> = rig
        .events()
        .into_iter()
        .filter(|event| {
            event["kind"] == "task.state"
                && event["data"]["task"] == 1
                && event["data"]["to"] == "paused"
        })
        .map(|event| event["data"]["because"].to_string())
        .collect();
    assert!(
        pauses
            .iter()
            .all(|because| because.contains("out of quota")),
        "T-1 was paused for {pauses:?}; it moved: {:#?}",
        moves(rig)
    );
}

/// The account the worker's harness refuses while its file is there.
fn account() -> cf_e2e::Result<(TempDir, PathBuf)> {
    let folder = tempfile::Builder::new()
        .prefix("cf-account-")
        .tempdir()
        .map_err(|source| Error::File {
            action: "make a folder in",
            path: std::env::temp_dir(),
            source,
        })?;
    let file = folder.path().join("refused");
    files::write(&file, "")?;
    Ok((folder, file))
}

/// The rig of both cases: the worker's provider refuses `QUOTA-OUT` while the account's
/// file is there; refused, the worker says it continues automatically, and does two
/// seconds after its account has quota again, as Claude Code does.
fn config_of(refused: &Path) -> Config {
    config()
        .var("CF_TEST_QUOTA_OUT", "worker")
        .var("CF_TEST_QUOTA_FILE", refused.to_string_lossy())
        .var("CF_TEST_AUTO_CONTINUE_MS", "worker 2000")
}

/// T-21's way to its reopen, on the rig: its brief is refused for quota and it is held,
/// its requester told it goes on by itself (m-444); quota is back and the worker goes on
/// with it by itself (m-446); its answer is T-1's result (m-447), given to the chief. The
/// project and the chief's window.
fn done_after_a_quota_wait<'a>(
    rig: &'a Rig,
    refused: &Path,
) -> cf_e2e::Result<(Project<'a>, Value)> {
    let project = Project::open(rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    cf(
        rig,
        &chief,
        &[
            "task",
            "add",
            "--tier",
            tier,
            "QUOTA-OUT Reply with exactly: FIRST_OK",
        ],
    )?;
    rig.wait_for("the chief to be told T-1 waits", secs(60), || {
        Ok(waits_told(&project)? == 1)
    })?;
    // Held a while, its window waiting to continue.
    std::thread::sleep(secs(15));
    let _ = std::fs::remove_file(refused);
    rig.wait_for("T-1's result to reach the chief", secs(90), || {
        Ok(state(&project, 1)? == "done" && results(&project)?.len() == 1)
    })?;
    never_stopped(rig, &project)?;
    Ok((project, chief))
}

#[test]
fn a_reopen_after_a_quota_wait_gives_the_window_its_words_and_stops_nothing() -> Outcome {
    let (_folder, refused) = account()?;
    let rig = Rig::start(config_of(&refused))?;
    let (project, chief) = done_after_a_quota_wait(&rig, &refused)?;

    // Bug 9: the chief sends T-1 back as soon as its result is in.
    cf(
        &rig,
        &chief,
        &["task", "reopen", "T-1", "Reply with exactly: AGAIN_OK"],
    )?;
    rig.wait_for("T-1 to be done again", secs(120), || {
        Ok(state(&project, 1)? == "done" && results(&project)?.len() == 2)
    })?;
    assert_eq!(
        results(&project)?.last().map(String::as_str),
        Some("AGAIN_OK"),
        "the reopen's words went in"
    );
    never_stopped(&rig, &project)?;
    paused_only_for_quota(&rig);
    rig.close()?;
    Ok(())
}

#[test]
fn a_reopened_task_refused_for_quota_waits_in_its_window_and_goes_on_by_itself_once_quota_is_back(
) -> Outcome {
    let (_folder, refused) = account()?;
    let rig = Rig::start(config_of(&refused))?;
    let (project, chief) = done_after_a_quota_wait(&rig, &refused)?;

    // Bug 10: the account is spent again, and the reopen's words are refused.
    files::write(&refused, "")?;
    cf(
        &rig,
        &chief,
        &[
            "task",
            "reopen",
            "T-1",
            "QUOTA-OUT Reply with exactly: AGAIN_OK",
        ],
    )?;
    rig.wait_for("the chief to be told T-1 waits again", secs(90), || {
        Ok(waits_told(&project)? == 2)
    })?;
    // A while held, its window waiting to continue: nothing interrupts, closes or pauses it
    // but the hold.
    std::thread::sleep(secs(15));
    never_stopped(&rig, &project)?;
    assert_eq!(state(&project, 1)?, "paused", "held");
    // Quota is back: it goes on by itself, as its note said, and ends.
    let _ = std::fs::remove_file(&refused);
    rig.wait_for("T-1 to be done again", secs(120), || {
        Ok(state(&project, 1)? == "done" && results(&project)?.len() == 2)
    })?;
    never_stopped(&rig, &project)?;
    paused_only_for_quota(&rig);
    rig.close()?;
    Ok(())
}
