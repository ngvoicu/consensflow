//! One chief's run: the project opened with the real harness as its chief and the
//! stand-in as its member, the chief put into a turn, the cancel that makes the two
//! notes, the notes given while each turn on them is held open by a command that ends
//! by itself, and the chief's own record read for what it was given.

use std::path::Path;
use std::time::{Duration, Instant};

use cf_e2e::live::ask;
use cf_e2e::live::members::{
    find, member_as, quiet_command, stand_in_worker, Account, Members, Plan, STAND_IN_WORKER,
};
use cf_e2e::live::{seconds, Harness, Log};
use cf_e2e::Result;
use serde_json::json;

use crate::board::{chief_state, delivered, named, record_of, retried, told};
use crate::turns::{held, worked_since, Quiet, HELD_S};
use crate::verdict::{enter_again, first_paste, lateness, pasted_first, proofs, seen, Note, Proof};
use crate::wire::Watch;

/// The stand-in for the member's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The daemon's own arrival window, in milliseconds (`Limits::default`): how long a
/// window at rest has to show a message pasted into it before the paste is taken for
/// lost. A note's command takes twenty seconds more.
const ARRIVAL_MS: u64 = 60_000;
const _: () = assert!(HELD_S * 1_000 >= ARRIVAL_MS + 20_000);

/// How many seconds the command the human has the chief run first takes: it holds
/// the chief in a turn while the member's window starts, leaves its job and is
/// cancelled, and the notes are sent before it ends. Under the two minutes a Claude
/// Code shell command is given unless it asks for more.
const FIRST_S: u64 = 100;

/// How long the chief has to go into its first turn, the member's window to leave its
/// job, both notes to be sent to the chief once the task is cancelled, and the notes to
/// be given with the chief quiet after them (two turns held open, and what a model
/// takes between them).
const WORK: Duration = Duration::from_secs(120);
const JOB: Duration = Duration::from_secs(120);
const NOTES: Duration = Duration::from_secs(120);
const GIVEN: Duration = Duration::from_secs(900);

/// How often the board is looked at.
const LOOK: Duration = Duration::from_millis(500);

/// How many times the human presses Enter again for the first command, and the key.
const ENTERS: u32 = 3;
const ENTER: u8 = b'\r';

/// A number of milliseconds as the events count them.
fn ms(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

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
/// stand-in as its one member, and the folder told what to do on a note (written over
/// the plan's own brief, which a text made at run time cannot be).
pub fn plan(chief: Harness) -> Plan {
    let told = brief();
    Plan::new("notes", member_as(chief, "chief"))
        .with(stand_in_worker())
        .with_file("AGENTS.md", told.clone())
        .with_file("CLAUDE.md", told)
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
    // A window that has just gone idle can take the paste and lose its Enter (a Devin
    // chief kept the command in its input, twice on 2026-10-14): the human presses
    // Enter again while the chief has not gone to work, as every live check that types
    // to a chief does (`Human::tells`).
    let mut inside = None;
    for again in 0..=ENTERS {
        inside = find(
            log,
            "the chief to go into its first turn",
            WORK / (ENTERS + 1),
            LOOK,
            || Ok((chief_state(&members)? == "working").then_some(())),
        )?;
        if inside.is_some() || again == ENTERS {
            break;
        }
        log.say(
            "the chief has not gone to work on what the human typed: the human presses Enter again",
        );
        members.press(&pane, &[ENTER])?;
    }
    if inside.is_none() {
        case.fail(format!(
            "the chief never went into a turn: {}",
            members.tail(pane["id"].as_str().unwrap_or_default())
        ));
        case.closed(members.close());
        return Ok(case);
    }

    // The chief gives the member a task whose window starts a job its turn goes on
    // working beside, and the human cancels it in the turn: the notes are the chief's,
    // and it is in a turn. (A turn that ended with its answer is its task's result and
    // ends the job with it: nothing would be left to cancel.)
    let number = members.give(
        STAND_IN_WORKER,
        "JOB 300 SLEEP 300 Reply with exactly: WORKER_OK",
    )?;
    let leaves = find(
        log,
        "the member's window to start its job",
        JOB,
        LOOK,
        || Ok((members.rig().jobs("job").len() == 1).then_some(())),
    )?;
    case.check(
        "the member's window has a job running",
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
    // Only a Devin chief has a queue the run is about. A Claude chief may run its
    // command in the background and be at rest by now, which its run only says: a
    // note handed to it once is the verdict there (2026-10-14).
    let busy = chief_state(&members)? == "working";
    let lane = format!(
        "the chief's lane reads {}: a chief at rest is handed them at once, and proves nothing",
        members.lane("chief")?["activity"]
    );
    if chief == Harness::Devin {
        case.check("the chief is in a turn when the notes are sent", busy, lane);
    } else if !busy {
        case.say(lane);
    }

    // The first command ends by itself, and the notes go in one after the other, each
    // turn on them held open by the command the folder tells the chief to run.
    let gone = Instant::now();
    let mut quiet = Quiet::default();
    let mut wire = Watch::default();
    let over = find(
        log,
        "both notes to be given and the chief quiet",
        GIVEN,
        LOOK,
        || {
            if chief == Harness::Devin {
                let now = jiff::Timestamp::now().as_millisecond();
                wire.look(&members.rig().home(), &notes, now);
            }
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
    conclude(&mut case, chief, &members, &notes, &wire)?;
    case.closed(members.close());
    Ok(case)
}

/// What the run came to, once the notes were given: what the chief's own record
/// says of each, what the daemon's events prove of its way with a record that is late
/// or a turn that is begun, and what was said besides.
fn conclude(
    case: &mut Account,
    chief: Harness,
    members: &Members,
    notes: &[Note],
    wire: &Watch,
) -> Result<()> {
    let events = members.events();
    // The turns on the notes begin with the first paste of one: what the chief did
    // before it was the first command's. A chief may run its command in the
    // background, which holds no turn: it is told, and no check.
    let worked = first_paste(&events, notes).map_or(0, |pasted| {
        worked_since(
            &events,
            "chief",
            pasted,
            jiff::Timestamp::now().as_millisecond(),
        )
    });
    case.say(format!(
        "the chief worked {} s from the first paste of a note, and each note's command takes {HELD_S} s{}",
        seconds(worked),
        if held(worked, notes.len()) {
            ""
        } else {
            ": it did not wait for each, or ran it in the background"
        }
    ));

    // What the chief's own record says: each note once.
    match record_of(chief, members)? {
        None => case.fail("the chief's own record could not be read"),
        Some(record) => {
            for found in seen(notes, &record.items) {
                case.check(
                    &format!("{} went into the chief's window once", found.note.what),
                    found.held(),
                    found.said(),
                );
            }
        }
    }

    // What it proves: a Devin chief queues what it is handed in a turn, and a run in
    // which neither note was late nor was held has not met what the daemon is for. A
    // chief that queues nothing has no such run to meet: each note is in its record once.
    let found = proofs(&events, notes, ms(ARRIVAL_MS), ms(HELD_S * 500));
    let proved = found.iter().map(Proof::said).collect::<Vec<_>>().join("; ");
    if chief == Harness::Devin {
        case.check(
            "the run met a record that was late or a turn that had begun",
            !found.is_empty(),
            if found.is_empty() {
                format!(
                    "neither note went unconfirmed longer than the daemon's {} s, and none was handed over after the turn on the note before it began: both were confirmed promptly, which proves nothing",
                    ARRIVAL_MS / 1_000
                )
            } else {
                proved
            },
        );
    } else {
        case.say(if found.is_empty() {
            "neither note met a record that was late or a turn that had begun: a chief of this harness queues nothing the daemon waits on".to_owned()
        } else {
            proved
        });
    }
    for late in lateness(&events, notes) {
        case.say(late);
    }
    if chief == Harness::Devin {
        for named in wire.said(notes, |id| pasted_first(&events, id)) {
            case.say(named);
        }
    }
    let again = retried(&events, notes);
    case.say(if again.is_empty() {
        "the daemon pasted neither note again".to_owned()
    } else {
        format!("the daemon pasted again: {}", again.join("; "))
    });
    let entered = enter_again(&events, notes);
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
    Ok(())
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
    fn the_chief_is_a_real_harness_on_its_cheap_model_the_member_the_stand_in_and_no_variable_the_daemons(
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
            assert!(
                plan.vars.is_empty(),
                "the daemon runs on the limits it ships with"
            );
        }
    }
}
