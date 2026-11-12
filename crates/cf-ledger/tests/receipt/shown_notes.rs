//! Notes of ConsensFlow's own that ask a chief no decision (RS-25): to a chief they are
//! shown, not typed. Each is still a row of the ledger, the chief's inbox lists it, and
//! no delivery ever takes it; what it says is its task's line (`aside`) until the task
//! moves on. To anyone but a chief it is queued as any note.

use cf_ledger::{MessageView, NewNote, SHOWN};
use serde_json::json;

use crate::fixture::{world, World};

/// The end of the hold the tests name.
const UNTIL: &str = "2026-10-10T15:00:00.000Z";

/// T-1 for the standard workers, in zeus's session, its brief delivered.
fn with_zeus(w: &mut World) -> (i64, String) {
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    w.deliver(moved.message.expect("its brief").id);
    (task, session)
}

/// What task `number`'s line says of it instead of a note, while it holds.
fn aside(w: &World, number: i64) -> Option<String> {
    w.ledger
        .task(w.project, number)
        .expect("a read")
        .expect("the task")
        .task
        .aside
}

/// ConsensFlow's note about T-`number` to `to`, shown where it asks no decision.
fn shown(w: &mut World, to: &str, number: i64, body: &str, said: &str) -> MessageView {
    w.ledger
        .show(
            w.project,
            &NewNote {
                from: None,
                to: to.into(),
                body: body.into(),
                task: Some(number),
            },
            said,
        )
        .expect("a note")
}

/// A hold of the kinds a chief is told of once (quota, an unavailable account, a moved
/// model): the note of T-`number` with `session`, out of quota until `until` where it is one.
#[derive(Clone, Copy)]
enum Hold {
    Quota,
    Unavailable,
    Moved,
}

/// ConsensFlow's note of `hold` of T-`number` to the chief, as the engine writes it.
fn note_of(w: &mut World, hold: Hold, number: i64, session: &str, until: &str) -> MessageView {
    let ledger = &mut w.ledger;
    match hold {
        Hold::Quota => ledger.note_hold(w.project, "chief", number, session, until),
        Hold::Unavailable => {
            ledger.note_hold_unavailable(w.project, "chief", number, session, "signed out")
        }
        Hold::Moved => ledger.note_hold_moved(
            w.project,
            "chief",
            number,
            session,
            "its harness moved it from Opus to Sonnet",
        ),
    }
    .expect("the chief is told")
}

/// What T-1's line says of `hold` with `session`.
fn said_of(hold: Hold, session: &str) -> String {
    match hold {
        Hold::Quota => format!("waits with @{session}: out of quota until {UNTIL}"),
        Hold::Unavailable => format!(
            "waits with @{session}: its account is unavailable (signed out) until the human says it works"
        ),
        Hold::Moved => format!(
            "waits with @{session}: its harness moved it from Opus to Sonnet, until the human says it may go on"
        ),
    }
}

const HOLDS: [Hold; 3] = [Hold::Quota, Hold::Unavailable, Hold::Moved];

#[test]
fn the_first_note_of_a_hold_is_typed_to_the_chief_and_the_same_hold_said_again_is_shown() {
    for hold in HOLDS {
        let mut w = world();
        let (task, session) = with_zeus(&mut w);
        w.hold(task);
        // The first word of a hold is typed: a chief waits for nothing else, and only the
        // chief asks the human for an account or a model, or gives the work to another.
        let first = note_of(&mut w, hold, task, &session, UNTIL);
        assert_eq!(first.state, "queued");
        assert_eq!(w.next("chief"), Some(first.id));
        // Its task's line says it all the same.
        assert_eq!(aside(&w, task), Some(said_of(hold, &session)));
        // The same hold said again, before or after the first is typed: shown, not typed.
        let again = note_of(&mut w, hold, task, &session, UNTIL);
        assert_eq!(again.state, "read");
        assert_eq!(again.receipt, json!({ SHOWN: said_of(hold, &session) }));
        w.deliver(first.id);
        let and_again = note_of(&mut w, hold, task, &session, UNTIL);
        assert_eq!(and_again.state, "read");
        assert_eq!(w.next("chief"), None);
        assert_eq!(aside(&w, task), Some(said_of(hold, &session)));
        // The chief's inbox lists them all.
        let listed: Vec<i64> = w
            .ledger
            .inbox(w.id("chief"), 100)
            .expect("the inbox")
            .iter()
            .map(|message| message.id)
            .collect();
        assert!([first.id, again.id, and_again.id]
            .iter()
            .all(|id| listed.contains(id)));
    }
}

#[test]
fn a_quota_hold_with_another_end_is_another_hold_and_is_typed() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.hold(task);
    let first = note_of(&mut w, Hold::Quota, task, &session, UNTIL);
    w.deliver(first.id);
    let later = note_of(
        &mut w,
        Hold::Quota,
        task,
        &session,
        "2026-10-17T15:00:00.000Z",
    );
    assert_eq!(later.state, "queued", "a new end is news");
}

#[test]
fn a_hold_whose_note_was_withdrawn_before_it_was_typed_is_typed_when_it_comes_again() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.hold(task);
    let first = note_of(&mut w, Hold::Quota, task, &session, UNTIL);
    // The daemon resumes it before the chief's window takes the note: it is withdrawn.
    w.daemon_resumes(task);
    assert_eq!(w.message(first.id).state, "cancelled");
    w.hold(task);
    let again = note_of(&mut w, Hold::Quota, task, &session, UNTIL);
    assert_eq!(again.state, "queued", "the chief was never told");
}

#[test]
fn a_hold_after_the_task_went_on_is_typed_again() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.hold(task);
    let first = note_of(&mut w, Hold::Quota, task, &session, UNTIL);
    w.deliver(first.id);
    w.daemon_resumes(task);
    assert_eq!(aside(&w, task), None, "a move ends what the line said");
    w.went_on(task);
    assert_eq!(
        aside(&w, task).as_deref(),
        Some("goes on: its account has quota again."),
        "the news is shown"
    );
    w.hold(task);
    let again = note_of(&mut w, Hold::Quota, task, &session, UNTIL);
    assert_eq!(
        again.state, "queued",
        "a wait that ended and came back is news"
    );
    assert_eq!(aside(&w, task), Some(said_of(Hold::Quota, &session)));
}

#[test]
fn a_hold_of_a_task_for_anyone_but_a_chief_is_queued_each_time_and_says_nothing_on_its_line() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.hold(task);
    for _ in 0..2 {
        let note = w
            .ledger
            .note_hold(w.project, "human", task, &session, UNTIL)
            .expect("the human is told");
        assert_eq!(note.state, "queued");
    }
    assert_eq!(aside(&w, task), None);
}

#[test]
fn a_note_shown_to_anyone_but_a_chief_is_queued_as_any_note() {
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    let for_the_human = shown(&mut w, "human", task, "T-1 goes on.", "goes on");
    assert_eq!(for_the_human.state, "queued");
    assert_eq!(for_the_human.receipt, serde_json::Value::Null);
    assert_eq!(aside(&w, task), None);
}

#[test]
fn the_newest_aside_holds_until_the_task_moves_and_is_said_anew_in_place() {
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    let first = shown(
        &mut w,
        "chief",
        task,
        "T-1 is still working: 1 command.",
        "in the background: 1 command",
    );
    assert_eq!(
        aside(&w, task).as_deref(),
        Some("in the background: 1 command")
    );
    // Said anew: the line, not the note, and no row more.
    assert!(w
        .ledger
        .renew_aside(w.project, first.id, "in the background: 2 commands")
        .expect("renewed"));
    assert_eq!(
        aside(&w, task).as_deref(),
        Some("in the background: 2 commands")
    );
    assert_eq!(w.message(first.id).body, "T-1 is still working: 1 command.");
    let note = |_: &World| NewNote {
        from: None,
        to: "chief".into(),
        body: "T-1 has been working in the background for 60 minutes.".into(),
        task: Some(task),
    };
    // One reminder, until the note is said anew.
    let sent = w
        .ledger
        .remind_of_aside(w.project, first.id, &note(&w))
        .expect("a write");
    assert_eq!(
        sent.as_ref().map(|sent| sent.state.as_str()),
        Some("queued")
    );
    assert_eq!(
        w.ledger
            .remind_of_aside(w.project, first.id, &note(&w))
            .expect("a write"),
        None
    );
    assert!(w
        .ledger
        .renew_aside(w.project, first.id, "in the background: 1 command")
        .expect("renewed"));
    assert!(w
        .ledger
        .remind_of_aside(w.project, first.id, &note(&w))
        .expect("a write")
        .is_some());
    // A move ends it, and it is neither said anew nor reminded of after.
    w.pause(task);
    assert_eq!(aside(&w, task), None, "a pause is a move");
    assert!(!w
        .ledger
        .renew_aside(w.project, first.id, "in the background")
        .expect("a write"));
    assert_eq!(aside(&w, task), None);
    w.ledger
        .renew_aside(w.project, first.id, "x")
        .expect("a write");
    assert!(w
        .ledger
        .remind_of_aside(w.project, first.id, &note(&w))
        .expect("a write")
        .is_none());
}

#[test]
fn a_note_whose_line_ended_with_a_move_is_shown_again_with_no_row_more() {
    // A busy model's wait: one note for the whole wait, each try a move of its own.
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    w.hold(task);
    let note = shown(
        &mut w,
        "chief",
        task,
        "T-1 waits with @zeus: its model is busy.",
        "waits with @zeus: its model is busy",
    );
    w.resume(task, "Go on.");
    assert_eq!(aside(&w, task), None, "the try is a move");
    w.hold(task);
    assert!(w.ledger.show_again(w.project, note.id).expect("a write"));
    assert_eq!(
        aside(&w, task).as_deref(),
        Some("waits with @zeus: its model is busy")
    );
    assert_eq!(w.message(note.id).state, "read", "still never typed");
    // Once shown, it is shown; and no note of anyone else's is.
    assert!(!w.ledger.show_again(w.project, note.id).expect("a write"));
}

#[test]
fn a_task_taken_back_with_nothing_bound_to_it_is_shown_to_the_chief_not_typed() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.ledger
        .release_task(w.project, task, "by @human")
        .expect("released");
    let note = w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("the inbox")
        .into_iter()
        .find(|message| message.body.starts_with("T-1 was taken back from"))
        .expect("the chief's inbox lists the note");
    assert_eq!(
        note.body,
        format!(
            "T-1 was taken back from @{session} (by @human) and waits for another standard worker."
        )
    );
    // It goes on by itself: the chief has nothing to decide, and nothing is typed.
    assert_eq!(note.state, "read");
    assert_eq!(
        w.next("chief"),
        None,
        "nothing is typed into the chief's window"
    );
    assert_eq!(
        aside(&w, task).as_deref(),
        Some(
            format!(
                "was taken back from @{session} (by @human) and waits for another standard worker."
            )
            .as_str()
        )
    );
}

#[test]
fn a_task_taken_back_with_a_follow_up_bound_to_its_window_is_typed_to_the_chief() {
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    let after = w
        .follow_up(task, "Part 2", &[])
        .expect("a follow-up")
        .task
        .number;
    w.ledger
        .release_task(w.project, task, "by @human")
        .expect("released");
    let note = w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("the inbox")
        .into_iter()
        .find(|message| message.body.starts_with("T-1 was taken back from"))
        .expect("the note");
    // The follow-up stays bound to a window that no longer has T-1: the chief decides.
    assert!(
        note.body.contains(&format!("T-{after} stays bound to @")),
        "{}",
        note.body
    );
    assert_eq!(note.state, "queued");
}

/// What task `number`'s row says it is held until.
fn held_until(w: &World, number: i64) -> Option<String> {
    w.ledger
        .task(w.project, number)
        .expect("a read")
        .expect("the task")
        .task
        .held_until
}

#[test]
fn a_held_task_taken_back_to_the_board_holds_with_nobody_and_its_line_says_no_hold() {
    // A quota-held task with a follow-up bound to its window, taken back: its note is typed
    // (a decision), so no shown note says its line; the hold went with the member it was taken
    // from, and its line must not say it still waits with that member.
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.follow_up(task, "Part 2", &[]).expect("a follow-up");
    w.hold(task);
    let typed = note_of(&mut w, Hold::Quota, task, &session, UNTIL);
    w.deliver(typed.id);
    assert_eq!(aside(&w, task), Some(said_of(Hold::Quota, &session)));
    w.ledger
        .release_task(w.project, task, "by @human")
        .expect("released");
    let view = w
        .ledger
        .task(w.project, task)
        .expect("a read")
        .expect("the task")
        .task;
    assert_eq!(view.state, "open");
    assert_eq!(view.aside, None, "an open task says no hold");
    assert_eq!(view.held_until, None, "it holds with nobody");
}

#[test]
fn a_held_task_resumed_after_its_session_ended_holds_no_more_and_its_line_says_no_hold() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.hold(task);
    let typed = note_of(&mut w, Hold::Unavailable, task, &session, UNTIL);
    w.deliver(typed.id);
    assert_eq!(aside(&w, task), Some(said_of(Hold::Unavailable, &session)));
    w.ledger
        .end_session(w.project, &session, "human")
        .expect("a session that holds only a paused task ends");
    w.resume(task, "Go on");
    assert_eq!(
        w.ledger
            .task(w.project, task)
            .expect("a read")
            .expect("the task")
            .task
            .state,
        "open"
    );
    assert_eq!(aside(&w, task), None, "an open task says no hold");
    assert_eq!(held_until(&w, task), None, "it holds with nobody");
}

#[test]
fn what_waits_on_a_chief_that_takes_over_names_each_task_it_gave_that_is_held_and_its_hold() {
    // A switched chief is a new conversation: the first note of a hold was typed to the old
    // one, and the same hold said again is only shown. Its handoff names each held task with
    // what its line says (T-1500's finding 2); a busy model goes on by itself and is not one.
    for hold in HOLDS {
        let mut w = world();
        let (task, session) = with_zeus(&mut w);
        w.hold(task);
        let first = note_of(&mut w, hold, task, &session, UNTIL);
        w.deliver(first.id);
        note_of(&mut w, hold, task, &session, UNTIL);
        let open = w.ledger.chief_open_work(w.project).expect("the open work");
        let held: Vec<(i64, Option<String>)> = open
            .held
            .iter()
            .map(|task| (task.number, task.aside.clone()))
            .collect();
        assert_eq!(held, [(task, Some(said_of(hold, &session)))]);
    }
    // A hold that became a busy model's wait is one no more.
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.hold(task);
    let first = note_of(&mut w, HOLDS[0], task, &session, UNTIL);
    w.deliver(first.id);
    w.ledger
        .note_hold_busy(w.project, "chief", task, &session, "overloaded", true)
        .expect("the chief is told");
    let open = w.ledger.chief_open_work(w.project).expect("the open work");
    assert!(open.held.is_empty(), "{:?}", open.held);
}

/// What a chief that takes over is told of each task it gave that is held or whose member left.
fn told_held(w: &World) -> Vec<(i64, Option<String>)> {
    w.ledger
        .chief_open_work(w.project)
        .expect("the open work")
        .held
        .iter()
        .map(|task| (task.number, task.aside.clone()))
        .collect()
}

/// What a chief that takes over is told of a paused task whose member left the staff.
const MEMBER_GONE: &str = "is paused, and its member @zeus has left the staff";

#[test]
fn a_held_task_whose_member_left_the_staff_is_told_to_a_new_chief_as_paused_and_not_held() {
    // T-1514's finding A: removal leaves a paused task for its resumption, its hold kept: at
    // its end the daemon resumes it in a fresh window, or tells the chief it stays paused
    // (`out_of_quota`'s test of a member given a task by name). A chief that takes over is
    // told the task is paused with its member gone, not that it waits with that member.
    for hold in HOLDS {
        let mut w = world();
        let (task, session) = with_zeus(&mut w);
        w.hold(task);
        let first = note_of(&mut w, hold, task, &session, UNTIL);
        w.deliver(first.id);
        w.ledger
            .remove_member(w.project, "zeus")
            .expect("zeus leaves the staff");
        assert_eq!(w.state(task), "paused", "left for its resumption");
        assert_eq!(
            held_until(&w, task).as_deref(),
            Some(UNTIL),
            "its hold ends as it would"
        );
        assert_eq!(told_held(&w), [(task, Some(MEMBER_GONE.to_owned()))]);
    }
}

#[test]
fn a_paused_task_that_never_held_whose_member_left_the_staff_is_told_to_a_new_chief() {
    // The handoff lists every paused task the chief gave whose member has left the staff, held
    // or not: nothing else wakes a new chief for it (T-1517's note).
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    w.pause(task);
    w.ledger
        .remove_member(w.project, "zeus")
        .expect("zeus leaves the staff");
    assert_eq!(w.state(task), "paused", "left for its resumption");
    assert_eq!(held_until(&w, task), None, "it never held");
    assert_eq!(told_held(&w), [(task, Some(MEMBER_GONE.to_owned()))]);
}

/// What the line of a paused task whose member left the staff says beside its state.
const MEMBER_GONE_LINE: &str = "its member @zeus has left the staff";

#[test]
fn a_held_task_whose_member_left_the_staff_says_on_its_line_it_is_paused_with_its_member_gone() {
    // The hold is kept to its end (`out_of_quota`'s test of a member given a task by name), but
    // `cf task list` and `cf task get` say what the handoff says, not a hold with a member that
    // is gone (T-1517's note): typed first, then said again and shown.
    for hold in HOLDS {
        let mut w = world();
        let (task, session) = with_zeus(&mut w);
        w.hold(task);
        let first = note_of(&mut w, hold, task, &session, UNTIL);
        w.deliver(first.id);
        let again = note_of(&mut w, hold, task, &session, UNTIL);
        assert_eq!(again.state, "read", "the same hold said again is shown");
        assert_eq!(aside(&w, task), Some(said_of(hold, &session)));
        w.ledger
            .remove_member(w.project, "zeus")
            .expect("zeus leaves the staff");
        assert_eq!(held_until(&w, task).as_deref(), Some(UNTIL), "its hold is kept");
        assert_eq!(aside(&w, task).as_deref(), Some(MEMBER_GONE_LINE));
    }
    // Paused and never held, the same.
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    w.pause(task);
    w.ledger
        .remove_member(w.project, "zeus")
        .expect("zeus leaves the staff");
    assert_eq!(aside(&w, task).as_deref(), Some(MEMBER_GONE_LINE));
}

#[test]
fn a_hold_after_its_task_left_its_member_is_another_hold_and_is_typed() {
    // T-1514's finding B: a hold is its member's. Taken back and given to another member, or
    // to a new window of the same member, the task's first refusal of the same kind and end is
    // typed: the chief's rescue failed, and only a typed note wakes it.
    for hold in HOLDS {
        for to in ["diana", "zeus"] {
            let mut w = world();
            let (task, session) = with_zeus(&mut w);
            w.hold(task);
            let first = note_of(&mut w, hold, task, &session, UNTIL);
            w.deliver(first.id);
            w.ledger
                .release_task(w.project, task, "by @human")
                .expect("released");
            let moved = w.assign(task, to);
            let other = moved.task.assignee.clone().expect("a session");
            w.deliver(moved.message.expect("its brief").id);
            w.hold(task);
            let again = note_of(&mut w, hold, task, &other, UNTIL);
            assert_eq!(
                again.state, "queued",
                "@{other}'s first hold after @{session}'s is typed"
            );
            assert_eq!(w.next("chief"), Some(again.id));
            // Said again by the same window, it is the same hold: shown.
            let same = note_of(&mut w, hold, task, &other, UNTIL);
            assert_eq!(same.state, "read");
        }
    }
}
