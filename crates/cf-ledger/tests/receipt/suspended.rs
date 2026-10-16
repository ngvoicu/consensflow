//! The pauses a project's suspension makes, and the human's Resume that ends them
//! (`tasks/pausing.rs`): the work whose window closed with the project goes on
//! when it opens again, at work as a quit leaves it, and every other pause stays,
//! however it was made; so does a pause a ledger made before the mark.

use cf_ledger::{migrate, open_ledger, Options, MIGRATIONS};
use serde_json::json;

use crate::fixture::{world, World};

/// What the daemon says of a window that closed with its project.
const CLOSED: &str = "@zeus's window closed";

/// Task `number`, given to a new session of `member` and at work there: its
/// brief pasted and proved.
fn at_work(w: &mut World, member: &str) -> i64 {
    let number = w.open("Work").task.number;
    let brief = w.assign(number, member).message.expect("its brief").id;
    w.deliver(brief);
    assert_eq!(w.state(number), "working");
    number
}

#[test]
fn the_resume_of_a_project_has_the_work_its_suspension_paused_at_work_again_and_no_other_pause() {
    let mut w = world();
    let closed = at_work(&mut w, "zeus");
    let human = at_work(&mut w, "zeus");
    let chief = at_work(&mut w, "diana");
    let crashed = at_work(&mut w, "diana");
    let held = at_work(&mut w, "zeus");
    let told = at_work(&mut w, "diana");
    let open = at_work(&mut w, "zeus");
    // While the project is open: the human's pause, the chief's, a window that
    // closed by itself, a hold, the chief's tell, and a pause with the project
    // that is no suspension's.
    w.ledger
        .pause_task(w.project, human, Some("human"), None)
        .expect("paused by the human");
    w.ledger
        .pause_task(w.project, chief, Some("chief"), None)
        .expect("paused by the chief");
    w.ledger
        .pause_task(w.project, crashed, None, Some(CLOSED))
        .expect("paused by ConsensFlow");
    w.hold(held);
    let session = w
        .ledger
        .task(w.project, told)
        .expect("a read")
        .expect("the task")
        .task
        .assignee
        .expect("its session");
    w.tell(&session, told, "Use the new style");
    w.ledger
        .pause_with_project(w.project, open, CLOSED)
        .expect("paused, the project open");
    // The human's Close: the project is suspended, and its window closes.
    w.ledger
        .set_project_state(w.project, "suspended")
        .expect("closed");
    w.ledger
        .pause_with_project(w.project, closed, CLOSED)
        .expect("paused with the project");
    let note = w
        .ledger
        .note_pause(w.project, "chief", closed, CLOSED)
        .expect("the chief is told");
    let others = [human, chief, crashed, held, told, open];
    assert!(others
        .iter()
        .chain([&closed])
        .all(|number| w.state(*number) == "paused"));

    w.ledger
        .set_project_state(w.project, "open")
        .expect("resumed");

    assert_eq!(w.state(closed), "working", "the Close's pause ends");
    for number in others {
        assert_eq!(w.state(number), "paused", "T-{number} stays paused");
    }
    let hold = w
        .ledger
        .task(w.project, held)
        .expect("a read")
        .expect("the task")
        .task;
    assert_eq!(
        hold.held_until.as_deref(),
        Some("2026-10-10T15:00:00.000Z"),
        "the hold is the daemon's to end"
    );
    let withdrawn = w.message(note.id);
    assert_eq!(
        (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
        ("cancelled", Some(format!("T-{closed} resumed").as_str())),
        "the news of the pause goes with it, never given"
    );
    let events = w.ledger.events(w.project, 0, 500).expect("the log");
    let went_on: Vec<_> = events
        .iter()
        .filter(|event| event.kind == "task.state" && event.data["from"] == "paused")
        .map(|event| event.data.clone())
        .collect();
    assert_eq!(
        went_on,
        [json!({
            "task": closed, "from": "paused", "to": "working",
            "by": null, "because": "the project was resumed",
        })]
    );
    // A Close and a Resume again leave every other pause as it is.
    w.ledger
        .set_project_state(w.project, "suspended")
        .expect("closed");
    w.ledger
        .set_project_state(w.project, "open")
        .expect("resumed");
    for number in others {
        assert_eq!(w.state(number), "paused", "T-{number}, again");
    }
}

#[test]
fn a_task_the_suspension_paused_while_its_window_waited_for_an_answer_waits_again() {
    let mut w = world();
    let number = at_work(&mut w, "zeus");
    let session = w
        .ledger
        .task(w.project, number)
        .expect("a read")
        .expect("the task")
        .task
        .assignee
        .expect("its session");
    let question = w.ask(&session, number, "Which dialect?");
    w.deliver(question.id);
    assert_eq!(w.state(number), "waiting");
    w.ledger
        .set_project_state(w.project, "suspended")
        .expect("closed");
    w.ledger
        .pause_with_project(w.project, number, CLOSED)
        .expect("paused with the project");

    w.ledger
        .set_project_state(w.project, "open")
        .expect("resumed");

    assert_eq!(
        w.state(number),
        "waiting",
        "its question is still to be answered"
    );
}

#[test]
fn a_task_the_suspension_paused_that_moved_since_is_no_longer_its_to_resume() {
    let mut w = world();
    let number = at_work(&mut w, "zeus");
    w.ledger
        .set_project_state(w.project, "suspended")
        .expect("closed");
    w.ledger
        .pause_with_project(w.project, number, CLOSED)
        .expect("paused with the project");
    // While it is closed, the chief resumes it, and the human pauses it.
    w.resume(number, "Go on");
    w.ledger
        .pause_task(w.project, number, Some("human"), None)
        .expect("paused by the human");

    w.ledger
        .set_project_state(w.project, "open")
        .expect("resumed");

    assert_eq!(w.state(number), "paused", "the human's pause is the last");
}

/// The session task `number` was given to.
fn session_of(w: &World, number: i64) -> String {
    w.ledger
        .task(w.project, number)
        .expect("a read")
        .expect("the task")
        .task
        .assignee
        .expect("its session")
}

/// The human's Close: the project is suspended, and the window of each of `numbers`
/// closes with it.
fn closed_with(w: &mut World, numbers: &[i64], because: &str) {
    w.ledger
        .set_project_state(w.project, "suspended")
        .expect("closed");
    for number in numbers {
        w.ledger
            .pause_with_project(w.project, *number, because)
            .expect("paused with the project");
    }
}

/// The mark a task's row holds, as the file has it: whether the Resume of its
/// project is to resume it.
fn mark_of(w: &mut World, number: i64) -> i64 {
    let mark = std::cell::Cell::new(-1);
    w.edit(|db| {
        mark.set(
            db.query_row(
                "SELECT paused_with_project FROM task WHERE number = ?",
                [number],
                |row| row.get(0),
            )
            .expect("its mark"),
        );
    });
    mark.get()
}

#[test]
fn a_tell_that_lands_after_the_close_takes_its_pause_over_and_the_resume_leaves_it_paused() {
    let mut w = world();
    let told = at_work(&mut w, "zeus");
    let other = at_work(&mut w, "diana");
    closed_with(&mut w, &[told, other], CLOSED);
    // The chief's `cf tell`, sent as the Close began, lands after it.
    let session = session_of(&w, told);
    w.tell(&session, told, "Stop: the schema changed");

    w.ledger
        .set_project_state(w.project, "open")
        .expect("resumed");

    assert_eq!(
        [w.state(told), w.state(other)],
        ["paused", "working"],
        "the tell made the pause the chief's"
    );
}

#[test]
fn a_pause_by_the_chief_or_the_human_of_a_task_the_close_paused_takes_the_pause_over() {
    let mut w = world();
    let by_chief = at_work(&mut w, "zeus");
    let by_human = at_work(&mut w, "diana");
    let other = at_work(&mut w, "zeus");
    closed_with(&mut w, &[by_chief, by_human, other], CLOSED);
    // Their pauses, sent as the Close began, land after it.
    w.ledger
        .pause_task(w.project, by_chief, Some("chief"), None)
        .expect("the chief's pause is taken");
    w.ledger
        .pause_task(w.project, by_human, Some("human"), None)
        .expect("the human's pause is taken");
    // A task paused by someone else is theirs already: pausing it again is refused, as it was.
    let again = w
        .ledger
        .pause_task(w.project, by_chief, Some("chief"), None)
        .expect_err("paused already");
    assert_eq!(again.code(), Some("invalid-transition"));

    w.ledger
        .set_project_state(w.project, "open")
        .expect("resumed");

    assert_eq!(
        [w.state(by_chief), w.state(by_human), w.state(other)],
        ["paused", "paused", "working"]
    );
}

#[test]
fn a_task_the_close_paused_that_someone_takes_from_its_session_keeps_no_mark() {
    let mut w = world();
    let reassigned = at_work(&mut w, "zeus");
    let ended = at_work(&mut w, "diana");
    closed_with(&mut w, &[reassigned, ended], CLOSED);
    // The human gives one to another member of its tier; the chief resumes the other,
    // whose session ended, and it goes back to the board for its tier.
    w.ledger
        .release_task(w.project, reassigned, "by @human")
        .expect("taken back to the board");
    let session = session_of(&w, ended);
    w.ledger
        .end_session(w.project, &session, "human")
        .expect("its session ends");
    w.resume(ended, "Go on");
    assert_eq!([w.state(reassigned), w.state(ended)], ["open", "open"]);

    assert_eq!(
        [mark_of(&mut w, reassigned), mark_of(&mut w, ended)],
        [0, 0],
        "taken over: no Resume of the project is theirs to resume"
    );
}

#[test]
fn the_resume_withdraws_the_pause_note_and_keeps_the_word_that_a_program_may_still_run() {
    let mut w = world();
    let alone = at_work(&mut w, "zeus");
    let joined = at_work(&mut w, "diana");
    let gone = at_work(&mut w, "zeus");
    let outlived = |who: &str| {
        format!(
            "@{who}'s window closed, but its program (process 4242) could not be ended and may still run"
        )
    };
    closed_with(&mut w, &[alone], &outlived("zeus"));
    closed_with(&mut w, &[joined], &outlived("diana"));
    closed_with(&mut w, &[gone], CLOSED);
    // One note of its own, and one note of two, as a burst of stalls joins them.
    let own = w
        .ledger
        .note_pause(w.project, "chief", alone, &outlived("zeus"))
        .expect("the chief is told");
    let several = w
        .ledger
        .note_pause(w.project, "chief", joined, &outlived("diana"))
        .expect("the chief is told");
    w.ledger
        .join_pause_note(several.id, gone, CLOSED)
        .expect("joined")
        .expect("the note takes it");

    w.ledger
        .set_project_state(w.project, "open")
        .expect("resumed");

    for note in [&own, &several] {
        assert_eq!(w.message(note.id).state, "cancelled", "the pause note goes");
    }
    let kept: Vec<String> = w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("the chief's inbox")
        .into_iter()
        .rev()
        .filter(|message| message.state == "queued" && message.body.contains("may still run"))
        .map(|message| message.body)
        .collect();
    assert_eq!(
        kept,
        [
            format!("T-{alone} resumed. Before that, {}.", outlived("zeus")),
            format!("T-{joined} resumed. Before that, {}.", outlived("diana")),
        ],
        "the warning stays for the chief, from a note of its own or of several, and only for the tasks it was said of"
    );
}

/// Both reviews of 2026-10-16, round 3: a cancel withdrew the note of a pause that
/// said its window's program may still run, before the warning was kept from it.
#[test]
fn a_cancel_withdraws_the_pause_note_and_keeps_the_word_that_a_program_may_still_run() {
    let mut w = world();
    let cancelled = at_work(&mut w, "zeus");
    let outlived =
        "@zeus's window closed, but its program (process 4242) could not be ended and may still run";
    closed_with(&mut w, &[cancelled], outlived);
    let note = w
        .ledger
        .note_pause(w.project, "chief", cancelled, outlived)
        .expect("the chief is told");

    w.ledger
        .cancel_task(w.project, cancelled, "human")
        .expect("called off");

    assert_eq!(w.message(note.id).state, "cancelled", "the pause note goes");
    let kept: Vec<String> = w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("the chief's inbox")
        .into_iter()
        .filter(|message| message.state == "queued" && message.body.contains(" Before that, "))
        .map(|message| message.body)
        .collect();
    assert_eq!(
        kept,
        [format!("T-{cancelled} cancelled. Before that, {outlived}.")]
    );
}

/// What the chief is told again of a task before it went, still queued.
fn said_again(w: &World) -> Vec<String> {
    w.ledger
        .inbox(w.id("chief"), 100)
        .expect("the chief's inbox")
        .into_iter()
        .filter(|message| message.state == "queued" && message.body.contains(" Before that, "))
        .map(|message| message.body)
        .collect()
}

/// The words of a pause that the Close could not end zeus's program.
const OUTLIVED: &str =
    "@zeus's window closed, but its program (process 4242) could not be ended and may still run";

/// T-`cancelled`'s pause note, which says [`OUTLIVED`], on its way to the chief
/// when the human cancels the task.
fn cancelled_on_its_way(w: &mut World) -> (i64, i64) {
    let cancelled = at_work(w, "zeus");
    closed_with(w, &[cancelled], OUTLIVED);
    let note = w
        .ledger
        .note_pause(w.project, "chief", cancelled, OUTLIVED)
        .expect("the chief is told");
    w.ledger.begin_delivery(note.id).expect("on its way");
    w.ledger
        .cancel_task(w.project, cancelled, "human")
        .expect("called off");
    (cancelled, note.id)
}

/// Both reviews of 2026-10-16, round 5: the cancel withdrew a pause note on its
/// way, and only some of the ways it ended without reaching the window said its
/// warning again (one whose arrival was uncertain, a launch that failed, a start
/// of the daemon did not). The note stays on its way, and its warning is owed
/// until its arrival is known: whatever sends it back to its queue says it again.
#[test]
fn a_pause_note_on_its_way_when_its_task_is_cancelled_says_its_warning_again_if_it_goes_back() {
    let mut w = world();
    let (cancelled, note) = cancelled_on_its_way(&mut w);
    assert_eq!(w.message(note).state, "delivering", "it may yet arrive");
    assert!(said_again(&w).is_empty());

    w.ledger
        .retry_delivery(note, "the harness record never showed it", false)
        .expect("tried again");

    assert_eq!(w.message(note).state, "cancelled", "it goes no more");
    assert_eq!(
        said_again(&w),
        [format!("T-{cancelled} cancelled. Before that, {OUTLIVED}.")]
    );
}

/// The same where its delivery is given up: the warning is said again, in a note
/// with attempts of its own, and nothing failed.
#[test]
fn a_pause_note_on_its_way_when_its_task_is_cancelled_says_its_warning_again_if_it_is_given_up() {
    let mut w = world();
    let (cancelled, note) = cancelled_on_its_way(&mut w);

    let given_up = w
        .ledger
        .fail_delivery(note, "the window never showed its first message")
        .expect("given up");

    assert_eq!(given_up.state, "cancelled");
    assert_eq!(
        said_again(&w),
        [format!("T-{cancelled} cancelled. Before that, {OUTLIVED}.")]
    );
}

/// A note that arrived has its warning in the window: nothing is said again of it.
#[test]
fn a_pause_note_on_its_way_when_its_task_is_cancelled_that_arrives_says_nothing_again() {
    let mut w = world();
    let (_, note) = cancelled_on_its_way(&mut w);

    w.ledger
        .confirm_delivery(note, None)
        .expect("in the chief's window");

    assert_eq!(w.message(note).state, "delivered");
    assert!(said_again(&w).is_empty(), "{:?}", said_again(&w));
}

/// A message on its way that says no warning goes with the cancel, as it always
/// did, and so does a warning of a task that was not cancelled when it goes back.
#[test]
fn a_message_on_its_way_that_says_no_warning_of_a_cancelled_task_goes_with_the_cancel() {
    let mut w = world();
    let first = at_work(&mut w, "zeus");
    let second = at_work(&mut w, "diana");
    closed_with(&mut w, &[first, second], OUTLIVED);
    // A warning whose task was not cancelled, tried again.
    let warning = w
        .ledger
        .note_pause(w.project, "chief", first, OUTLIVED)
        .expect("the chief is told");
    w.ledger.begin_delivery(warning.id).expect("on its way");
    w.ledger
        .retry_delivery(warning.id, "the window went", true)
        .expect("tried again");
    assert_eq!(w.message(warning.id).state, "queued");
    // A cancelled task's pause note that warns of nothing.
    let plain = w
        .ledger
        .note_pause(w.project, "chief", second, "@diana's window closed")
        .expect("the chief is told");
    w.ledger.begin_delivery(plain.id).expect("on its way");
    w.ledger
        .cancel_task(w.project, second, "human")
        .expect("called off");

    assert_eq!(w.message(plain.id).state, "cancelled");
    assert!(said_again(&w).is_empty(), "{:?}", said_again(&w));
}

/// A note its window was given has its warning in the window already: whatever
/// becomes of its task, nothing is said again of it.
#[test]
fn a_pause_note_its_window_was_given_keeps_nothing_when_its_task_is_cancelled() {
    let mut w = world();
    let cancelled = at_work(&mut w, "zeus");
    closed_with(&mut w, &[cancelled], OUTLIVED);
    let note = w
        .ledger
        .note_pause(w.project, "chief", cancelled, OUTLIVED)
        .expect("the chief is told");
    w.ledger.begin_delivery(note.id).expect("on its way");
    w.ledger
        .confirm_delivery(note.id, None)
        .expect("in the chief's window");
    w.ledger
        .cancel_task(w.project, cancelled, "human")
        .expect("called off");

    assert_eq!(w.message(note.id).state, "delivered");
    assert!(said_again(&w).is_empty(), "{:?}", said_again(&w));
}

/// One note naming T-`warned`, with the words that its window's program may still
/// run, and T-`plain`, on its way to the chief when the human cancels T-`warned`.
/// A cancel leaves a note of several as it is: it is on its way.
fn a_note_of_two_on_its_way(w: &mut World) -> (i64, i64, i64) {
    let warned = at_work(w, "zeus");
    let plain = at_work(w, "diana");
    closed_with(w, &[warned], OUTLIVED);
    closed_with(w, &[plain], CLOSED);
    let note = w
        .ledger
        .note_pause(w.project, "chief", warned, OUTLIVED)
        .expect("the chief is told");
    w.ledger
        .join_pause_note(note.id, plain, CLOSED)
        .expect("joined")
        .expect("the note takes it");
    w.ledger.begin_delivery(note.id).expect("on its way");
    w.ledger
        .cancel_task(w.project, warned, "human")
        .expect("called off");
    assert_eq!(w.message(note.id).state, "delivering");
    assert!(said_again(w).is_empty(), "it may yet arrive");
    (warned, plain, note.id)
}

/// The warning of each task the note names that was cancelled on its way, and
/// the words of the note once it came back.
fn kept(w: &World, note: i64) -> (Vec<String>, String) {
    (said_again(w), w.message(note).body)
}

/// Both reviews of 2026-10-16, round 6: a note naming T-1 and T-2, given up at
/// its last attempt after T-1 was cancelled, failed with T-1's warning in it, and
/// nothing said the warning again.
#[test]
fn a_note_of_several_given_up_after_one_of_its_tasks_was_cancelled_says_that_task_s_warning_again()
{
    let mut w = world();
    let (warned, plain, note) = a_note_of_two_on_its_way(&mut w);

    let given_up = w
        .ledger
        .fail_delivery(note, "the window refused it")
        .expect("given up");

    assert_eq!(given_up.state, "failed", "the rest of it failed");
    let alone = format!(
        "T-{plain} is paused: {CLOSED}. Resume it with: cf task resume T-{plain} \"…\"; its window comes back on its own conversation."
    );
    assert_eq!(
        kept(&w, note),
        (
            vec![format!("T-{warned} cancelled. Before that, {OUTLIVED}.")],
            alone
        )
    );
}

/// The same where it goes back to its queue: it names the rest, and the warning of
/// the one cancelled is said apart (Round 6's written limit: it went back whole).
#[test]
fn a_note_of_several_that_goes_back_after_one_of_its_tasks_was_cancelled_names_the_rest_and_says_the_warning_apart(
) {
    let mut w = world();
    let (warned, plain, note) = a_note_of_two_on_its_way(&mut w);

    w.ledger
        .retry_delivery(note, "the window went", true)
        .expect("tried again");

    assert_eq!(w.message(note).state, "queued");
    assert_eq!(
        w.message(note).task_number,
        Some(plain),
        "it is T-{plain}'s alone"
    );
    assert_eq!(
        said_again(&w),
        [format!("T-{warned} cancelled. Before that, {OUTLIVED}.")]
    );
}

/// And where every task it names was cancelled: it goes no more, and each warning
/// it said is said again.
#[test]
fn a_note_of_several_whose_tasks_were_all_cancelled_on_its_way_is_withdrawn_when_it_comes_back() {
    let mut w = world();
    let (warned, plain, note) = a_note_of_two_on_its_way(&mut w);
    w.ledger
        .cancel_task(w.project, plain, "human")
        .expect("called off");

    let given_up = w
        .ledger
        .fail_delivery(note, "the window refused it")
        .expect("given up");

    assert_eq!(given_up.state, "cancelled");
    assert_eq!(
        said_again(&w),
        [format!("T-{warned} cancelled. Before that, {OUTLIVED}.")]
    );
}

#[test]
fn a_ledger_from_before_the_mark_keeps_every_paused_task_paused_through_a_resume() {
    const AT: &str = "2026-10-15T00:00:00.000Z";
    let dir = tempfile::tempdir().expect("a home");
    let file = dir.path().join("consensflow.db");
    // 3.0.4's ledger: a project its Close suspended, its task paused by ConsensFlow
    // as the window closed, and the chief told in a note not yet given.
    {
        let db = rusqlite::Connection::open(&file).expect("the file");
        migrate(&db, &MIGRATIONS[..18]).expect("a version 18 ledger");
        db.execute_batch(&format!(
            "INSERT INTO project (id, directory, name, state, gate, created_at, updated_at) VALUES
               (1, '/work/app', 'app', 'suspended', 0, '{AT}', '{AT}');
             INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, created_at, designer) VALUES
               (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, NULL, '{AT}', 0),
               (2, 1, 'chief', 'chief', '[]', 'apollo', 'claude-code', NULL, NULL, '{AT}', 0),
               (3, 1, 'zeus', 'worker', '[\"worker\"]', 'zeus', 'claude-code', 'standard', NULL, '{AT}', 0);
             INSERT INTO task (id, project_id, number, title, body, requester_id, assignee_id, state, created_at, updated_at, paused_at, stop_seq)
               VALUES (1, 1, 1, 'Parser', 'Parser', 2, 3, 'paused', '{AT}', '{AT}', '{AT}', 1);
             INSERT INTO message (id, project_id, recipient_id, sender_id, kind, task_id, body, state, created_at) VALUES
               (1, 1, 3, 2, 'task', 1, 'Parser', 'delivered', '{AT}'),
               (2, 1, 2, NULL, 'note', 1, 'T-1 is paused: @zeus''s window closed. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation.', 'queued', '{AT}');
             INSERT INTO event (id, project_id, at, kind, data) VALUES
               (1, 1, '{AT}', 'task.state', '{{\"task\":1,\"from\":\"working\",\"to\":\"paused\",\"by\":null,\"because\":\"@zeus''s window closed\",\"stop\":1}}'),
               (2, 1, '{AT}', 'project.state', '{{\"from\":\"open\",\"to\":\"suspended\"}}');"
        ))
        .expect("its rows");
    }

    let mut ledger = open_ledger(&file, Options::default()).expect("this build opens it");
    ledger.set_project_state(1, "open").expect("resumed");

    let task = ledger.task(1, 1).expect("a read").expect("the task").task;
    assert_eq!(task.state, "paused", "a pause with no mark is not resumed");
    assert_eq!(
        ledger.message(2).expect("a read").expect("the note").state,
        "queued",
        "and the chief is still to be told of it"
    );
}
