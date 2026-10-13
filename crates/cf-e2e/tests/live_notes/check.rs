//! One chief's run: the project opened with the real harness as its chief and the
//! stand-in as its member, the chief put into a turn, the cancel that makes the two
//! notes, the notes given while each turn on them is held open by a command that ends
//! by itself, and the chief's own record read for what it was given.

use std::path::Path;
use std::time::{Duration, Instant};

use cf_e2e::live::ask::{self, Record};
use cf_e2e::live::members::{
    find, member_as, quiet_command, session_of, stand_in_worker, Account, Members, Plan,
    STAND_IN_WORKER,
};
use cf_e2e::live::{seconds, Harness, Log};
use cf_e2e::Result;
use serde_json::{json, Value};

use crate::turns::{held, worked_since, Quiet, HELD_S};
use crate::verdict::{enter_again, first_paste, lateness, seen, Note};

/// The stand-in for the member's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// How long the daemon of the run gives a window at rest to show a message pasted
/// into it, in milliseconds (`CONSENSFLOW_ARRIVAL_TIMEOUT_MS`): well under the time
/// the chief's command on a note takes ([`HELD_S`]), and over what a model takes to
/// its first word.
pub const ARRIVAL_MS: u64 = 30_000;

// A command takes twice the daemon's arrival window at least: a note whose record
// waits for the turn before it is later than the window.
const _: () = assert!(HELD_S * 1_000 >= 2 * ARRIVAL_MS);

/// How many seconds the command the human has the chief run first takes: it holds
/// the chief in a turn while the member's window starts, leaves its job and is
/// cancelled, and the notes are sent before it ends. Under the two minutes a Claude
/// Code shell command is given unless it asks for more.
const FIRST_S: u64 = 100;

/// How long the chief has to go into its first turn, the member's window to leave its
/// job, both notes to be sent once the task is cancelled, and the notes to be given
/// with the chief quiet after them (two turns held open, and what a model takes
/// between them).
const WORK: Duration = Duration::from_secs(120);
const JOB: Duration = Duration::from_secs(120);
const NOTES: Duration = Duration::from_secs(120);
const GIVEN: Duration = Duration::from_secs(900);

/// How often the board is looked at.
const LOOK: Duration = Duration::from_millis(500);

/// The command the chief runs on each note, and waits for.
pub fn note_command() -> String {
    quiet_command(HELD_S, cfg!(windows))
}

/// What the folder tells the chief: to do what a message asks, and to run the check's
/// command on each note and wait for it.
pub fn brief() -> String {
    format!(
        "This folder is an automated ConsensFlow test. Do exactly what each message asks and nothing more. When a ConsensFlow note arrives, run exactly this one shell command with your shell tool, wait for it to end, and then reply with exactly one line: NOTED\n{}\nWhen any other message arrives, do what it asks and reply with one line.\n",
        note_command()
    )
}

/// What the human asks the chief first: to run a long command and wait for it.
pub fn first_ask() -> String {
    format!(
        "Run exactly this one shell command with your shell tool, wait for it to end, and then reply with exactly one line: FINISHED\n{}",
        quiet_command(FIRST_S, cfg!(windows))
    )
}

/// What a real chief of `chief`'s harness is opened with: its cheap model, the
/// stand-in as its one member, the folder told what to do on a note (it is written
/// over the plan's own brief, which a text made at run time cannot be), and the
/// arrival window of the run.
pub fn plan(chief: Harness) -> Plan {
    let told = brief();
    Plan::new("notes", member_as(chief, "chief"))
        .with(stand_in_worker())
        .with_file("AGENTS.md", told.clone())
        .with_file("CLAUDE.md", told)
        .with_var("CONSENSFLOW_ARRIVAL_TIMEOUT_MS", ARRIVAL_MS.to_string())
}

/// The notes ConsensFlow sent the chief of the cancel of T-`number`, oldest first:
/// its own, and the closing of the member's window with its job running.
fn told(members: &Members, number: i64) -> Result<Vec<Note>> {
    Ok(notes_among(&members.inbox(Some("chief"))?, number))
}

/// The notes of the cancel of T-`number` among the messages of `inbox`: those
/// ConsensFlow wrote itself (no window did), by their words.
fn notes_among(inbox: &[Value], number: i64) -> Vec<Note> {
    let cancelled = format!("cancelled T-{number} ");
    let closed = format!("T-{number}'s window closed with background work");
    let mut found: Vec<Note> = inbox
        .iter()
        .filter(|message| message["kind"] == "note" && message["sender"].is_null())
        .filter_map(|message| {
            let body = message["body"].as_str()?;
            let what = if body.contains(&cancelled) {
                "the cancel's note"
            } else if body.starts_with(&closed) {
                "the closed window's note"
            } else {
                return None;
            };
            Some(Note {
                id: message["id"].as_i64()?,
                what,
            })
        })
        .collect();
    found.sort_by_key(|note| note.id);
    found
}

/// Whether the message `id` of the chief's inbox was confirmed by its record.
fn delivered(members: &Members, id: i64) -> Result<bool> {
    Ok(members
        .inbox(Some("chief"))?
        .iter()
        .any(|message| message["id"] == id && message["state"] == "delivered"))
}

/// What the state of the chief's window is, as the board says it.
fn chief_state(members: &Members) -> Result<String> {
    Ok(members.lane("chief")?["activity"]["state"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

/// What the chief's own harness recorded of its conversation, as the daemon reads
/// it: Claude Code's conversation is named by its window's launch, Devin names its
/// own, and its store says which is the newest of the run's folder.
fn record_of(chief: Harness, members: &Members) -> Result<Option<Record>> {
    let env = members.rig().environment();
    let session = if chief == Harness::Devin {
        ask::newest_session(chief.kind(), members.folder(), 0, env)?
    } else {
        let pane = format!("p{}-chief", members.project().id());
        members
            .rig()
            .open_frames()
            .into_iter()
            .rev()
            .find(|frame| frame["id"] == pane.as_str())
            .and_then(|frame| session_of(&frame))
    };
    match session {
        Some(session) => ask::records(chief.kind(), &session, env),
        None => Ok(None),
    }
}

/// The notes the daemon pasted again, from what it wrote: a `delivery.retried` of
/// each, and why.
fn retried(events: &[Value], notes: &[Note]) -> Vec<String> {
    events
        .iter()
        .filter(|event| event["kind"] == "delivery.retried")
        .filter_map(|event| {
            let id = event["data"]["message"].as_i64()?;
            notes.iter().any(|note| note.id == id).then(|| {
                format!(
                    "m-{id}: {}",
                    event["data"]["reason"].as_str().unwrap_or("no reason")
                )
            })
        })
        .collect()
}

/// The notes in words, as the case says them.
fn named(notes: &[Note]) -> String {
    notes
        .iter()
        .map(|note| format!("m-{} ({})", note.id, note.what))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The chief's run: its first command, the cancel and its notes, the notes given,
/// and what its record says of them.
pub fn drive(chief: Harness, mut members: Members, log: &Log) -> Result<Account> {
    let mut case = Account::new();
    case.say(format!("trust: {}", members.trust().unwrap_or("none")));
    case.say(format!(
        "the chief's first command: {}; its command on a note: {}",
        quiet_command(FIRST_S, cfg!(windows)),
        note_command()
    ));
    members.add(STAND_IN_WORKER, &[])?;

    // The human asks the chief to run a long command: it is in a turn.
    let pane = members.lane("chief")?["pane"].clone();
    members.paste(&pane, &ask::window_text(&first_ask())?)?;
    let inside = find(
        log,
        "the chief to go into its first turn",
        WORK,
        LOOK,
        || Ok((chief_state(&members)? == "working").then_some(())),
    )?;
    if inside.is_none() {
        case.fail(format!(
            "the chief never went into a turn: {}",
            members.tail(pane["id"].as_str().unwrap_or_default())
        ));
        case.closed(members.close());
        return Ok(case);
    }

    // The chief gives the member a task whose window leaves a job running, and the
    // human cancels it: the notes are the chief's, and it is in a turn.
    let number = members.give(STAND_IN_WORKER, "BACKGROUND 300 started the job")?;
    let leaves = find(
        log,
        "the member's window to leave its job",
        JOB,
        LOOK,
        || {
            Ok(members.lane(STAND_IN_WORKER)?["activity"]["reason"]
                .is_string()
                .then_some(()))
        },
    )?;
    case.check(
        "the member's window leaves a job running",
        leaves.is_some(),
        format!("T-{number}: {}", members.lane(STAND_IN_WORKER)?["activity"]),
    );
    let cancelled = members.page("task.cancel", json!({ "task": number }))?;
    case.check(
        "the human cancels it",
        cancelled["ok"] == true,
        format!("{cancelled}"),
    );
    let found = find(
        log,
        "both notes to be sent to the chief",
        NOTES,
        LOOK,
        || {
            let notes = told(&members, number)?;
            Ok((notes.len() == 2).then_some(notes))
        },
    )?;
    let Some(notes) = found else {
        case.fail(format!(
            "the chief was not sent the two notes of the cancel: {:?}",
            told(&members, number)?
        ));
        case.closed(members.close());
        return Ok(case);
    };
    case.say(format!("the notes: {}", named(&notes)));
    case.check(
        "the chief is in a turn when the notes are sent",
        chief_state(&members)? == "working",
        format!(
            "the chief's lane reads {}: a chief at rest is handed them at once, and proves nothing",
            members.lane("chief")?["activity"]
        ),
    );

    // The first command ends by itself, and the notes go in one after the other, each
    // turn on them held open by the command the folder tells the chief to run.
    let gone = Instant::now();
    let mut quiet = Quiet::default();
    let over = find(
        log,
        "both notes to be given and the chief quiet",
        GIVEN,
        LOOK,
        || {
            let mut at_rest = chief_state(&members)? == "idle";
            for note in &notes {
                at_rest &= delivered(&members, note.id)?;
            }
            Ok(quiet.look(at_rest, Instant::now()).then_some(()))
        },
    )?;
    case.check(
        "both notes are given and the chief goes quiet",
        over.is_some(),
        format!(
            "after {} s: {}; the chief's lane reads {}",
            seconds(i64::try_from(gone.elapsed().as_millis()).unwrap_or(i64::MAX)),
            named(&notes),
            members.lane("chief")?["activity"]
        ),
    );
    // The turns on the notes begin with the first paste of one: what the chief did
    // before it was the first command's.
    let events = members.events();
    let worked = first_paste(&events, &notes).map_or(0, |pasted| {
        worked_since(
            &events,
            "chief",
            pasted,
            jiff::Timestamp::now().as_millisecond(),
        )
    });
    case.check(
        "a turn was held open on each note",
        held(worked, notes.len()),
        format!(
            "the chief worked {} s from the first paste of a note, and each note's command takes {HELD_S} s: a chief that did not run its command left no turn to hold, and proves nothing",
            seconds(worked)
        ),
    );

    // What the chief's own record says: each note once.
    match record_of(chief, &members)? {
        None => case.fail("the chief's own record could not be read"),
        Some(record) => {
            for found in seen(&notes, &record.items) {
                case.check(
                    &format!("{} went into the chief's window once", found.note.what),
                    found.held(),
                    found.said(),
                );
            }
        }
    }
    for late in lateness(&events, &notes) {
        case.say(late);
    }
    let again = retried(&events, &notes);
    case.say(if again.is_empty() {
        "the daemon pasted neither note again".to_owned()
    } else {
        format!("the daemon pasted again: {}", again.join("; "))
    });
    let entered = enter_again(&events, &notes);
    case.say(if entered.is_empty() {
        "the daemon pressed Enter again for neither note".to_owned()
    } else {
        format!("the daemon pressed Enter again for {}", entered.join(", "))
    });
    let lane = members.lane("chief")?;
    case.say(format!(
        "the chief's screen: {}",
        members.tail(lane["pane"]["id"].as_str().unwrap_or_default())
    ));
    case.closed(members.close());
    Ok(case)
}

/// Opens `plan` and `drive`s its case: what the case saw, which holds when every
/// check of it did.
pub fn run(chief: Harness, log: &Log) -> Account {
    Members::open(&plan(chief), Path::new(FAKE_AGENT), log)
        .and_then(|members| drive(chief, members, log))
        .unwrap_or_else(|failed| {
            let mut case = Account::new();
            case.fail(failed);
            case
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_tells_the_chief_to_run_a_command_that_ends_by_itself_on_each_note() {
        let told = brief();
        assert!(told.contains(&note_command()), "{told}");
        assert!(told.contains("When a ConsensFlow note arrives"));
        // The command it is first asked for is another, so that it holds the chief the longer.
        assert!(first_ask().contains(&quiet_command(FIRST_S, cfg!(windows))));
        assert!(!first_ask().contains(&note_command()));
    }

    #[test]
    fn the_command_of_a_note_is_the_platforms_own_and_takes_as_long_as_the_check_says() {
        assert_eq!(quiet_command(HELD_S, false), format!("sleep {HELD_S}"));
        assert_eq!(
            quiet_command(HELD_S, true),
            format!("ping -n {} 127.0.0.1", HELD_S + 1)
        );
        assert_eq!(note_command(), quiet_command(HELD_S, cfg!(windows)));
    }

    #[test]
    fn the_chief_is_a_real_harness_on_its_cheap_model_the_member_the_stand_in_and_the_arrival_window_the_runs(
    ) {
        for chief in [Harness::Claude, Harness::Devin] {
            let plan = plan(chief);
            assert_eq!(plan.folder, "notes");
            assert_eq!(plan.chief, "chief");
            assert_eq!(plan.roster[0]["kind"], chief.kind());
            assert_eq!(plan.roster[1]["model"], "fake", "no model for the member");
            assert_eq!(plan.harnesses(), [chief]);
            // Both harnesses' own files are told the same, over the plan's own brief.
            let files: Vec<(&str, &str)> = plan
                .files
                .iter()
                .map(|(path, text)| (path.as_str(), text.as_str()))
                .collect();
            let told = brief();
            assert_eq!(files, [("AGENTS.md", &*told), ("CLAUDE.md", &*told)]);
            assert_eq!(
                plan.vars,
                [(
                    "CONSENSFLOW_ARRIVAL_TIMEOUT_MS".to_owned(),
                    ARRIVAL_MS.to_string()
                )]
            );
        }
    }

    #[test]
    fn the_notes_the_check_looks_for_are_those_of_the_cancel_and_of_the_closed_window_by_their_words(
    ) {
        let note = |id: i64, body: &str, sender: Value| json!({ "id": id, "kind": "note", "sender": sender, "body": body, "state": "queued" });
        let inbox = vec![
            note(
                9,
                "T-3's window closed with background work still running: node job",
                Value::Null,
            ),
            note(
                7,
                "@human cancelled T-3 (the parser): @worker's window was stopped.",
                Value::Null,
            ),
            // Another task's, one a window wrote, and what is no note.
            note(
                5,
                "@human cancelled T-4 (the lexer): @worker's window was stopped.",
                Value::Null,
            ),
            note(6, "@human cancelled T-3 (copied)", json!("zeus")),
            json!({ "id": 8, "kind": "result", "sender": null, "body": "cancelled T-3 (x)" }),
        ];
        let ids = |number: i64| -> Vec<(i64, &'static str)> {
            notes_among(&inbox, number)
                .into_iter()
                .map(|note| (note.id, note.what))
                .collect()
        };
        assert_eq!(
            ids(3),
            [(7, "the cancel's note"), (9, "the closed window's note")]
        );
        assert_eq!(ids(4), [(5, "the cancel's note")]);
        assert!(ids(5).is_empty());
    }

    #[test]
    fn the_notes_pasted_again_are_those_the_daemon_retried_and_say_why() {
        let events = vec![
            json!({ "kind": "delivery.retried", "data": { "message": 186, "reason": "the harness record never showed it" } }),
            json!({ "kind": "delivery.retried", "data": { "message": 99, "reason": "other" } }),
            json!({ "kind": "delivery.begun", "data": { "message": 187 } }),
        ];
        let notes = [Note {
            id: 186,
            what: "the cancel's note",
        }];
        assert_eq!(
            retried(&events, &notes),
            ["m-186: the harness record never showed it"]
        );
        assert!(retried(&events, &[]).is_empty());
    }

    #[test]
    fn the_notes_are_named_by_id_and_what_they_are() {
        let notes = [
            Note {
                id: 186,
                what: "the cancel's note",
            },
            Note {
                id: 187,
                what: "the closed window's note",
            },
        ];
        assert_eq!(
            named(&notes),
            "m-186 (the cancel's note), m-187 (the closed window's note)"
        );
        assert_eq!(named(&[]), "");
    }
}
