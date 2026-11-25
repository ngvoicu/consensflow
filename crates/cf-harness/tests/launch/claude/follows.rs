//! A look a follow overtook gives no answer (P22-1, T-2052 and T-2081): what the worker says of
//! the statuses, the holders or the transcript is of the conversation it was asked for, and a
//! window that has followed another since is told none of it. The next look asks afresh, and a
//! look that follows keep overtaking still ends.

use std::task::{Context, Poll, Waker};

use cf_harness::contract::{Observed, Records};
use cf_harness::seams::Time;
use cf_harness::testing::{LocalRecords, ManualTime};

use super::*;

const A: &str = "4b6e2a91-7c3d-4f5e-9a8b-1c2d3e4f5a6b";
const B: &str = "9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d";

/// A window of a conversation, and the records it is served by, whose looks and holders'
/// answers the test can hold.
fn held_window(home: &Home) -> (Rc<LocalRecords>, Rc<dyn Window>) {
    let env = home.env();
    let time: Rc<dyn Time> = Rc::new(ManualTime::new(0));
    let records = Rc::new(LocalRecords::new(env.clone(), Rc::clone(&time)));
    let fakes = Fakes::new(&env);
    let services = cf_harness::seams::Services {
        records: Rc::clone(&records) as Rc<dyn Records>,
        ..fakes.services(&env, Path::new(&home.root))
    };
    let window = window(&ClaudeAdapter::new(&services), &Request::resumed(A));
    window.opened(Some(me()));
    (records, window)
}

/// What the pass does with a look that says the window shows another conversation.
fn pass_follows(window: &Rc<dyn Window>, expected: &str) {
    let confirmed = finished(window.observe()).unwrap().switched;
    assert_eq!(confirmed.as_deref(), Some(expected));
    window.follow(expected);
}

/// What the exit collector asks of a look before it takes its answer for the task's result
/// (`Dispatcher::collect_at_exit`).
fn collectable(observed: &Observed) -> bool {
    observed.settled
        && !observed.failed
        && !observed.shared
        && observed.switched.is_none()
        && observed.pending().total() == 0
}

#[test]
fn a_transcript_read_across_a_follow_is_no_answer_and_never_conversation_a_s_result_for_b() {
    let home = Home::new();
    let (records, window) = held_window(&home);
    let outside = OtherProcess::start();
    home.status(A, &json!({ "status": "idle" }), outside.pid());
    home.status(A, &json!({ "status": "idle" }), me());
    home.transcript(
        A,
        &[
            user_line(A, 1, TASK),
            answer_line(A, 2, "A-only answer"),
            stop_line(A, 3),
        ],
    );
    home.transcript(B, &[user_line(B, 1, "B-only unfinished turn")]);
    let first = finished(window.observe()).unwrap();
    assert!(first.shared && first.settled, "A is shared and answered");
    // The pass has read the switch; before it applies the follow, another look (the exit
    // collector's, the trust watch's) asks for A's transcript, and the worker holds it.
    home.status(B, &json!({ "status": "idle" }), me());
    let switch = finished(window.observe()).unwrap().switched.unwrap();
    assert_eq!(switch, B);
    records.hold(true);
    let mut late = window.observe();
    let mut context = Context::from_waker(Waker::noop());
    assert!(late.as_mut().poll(&mut context).is_pending());
    records.hold(false);
    window.follow(&switch);
    assert!(records.release(), "the look was waiting for a transcript");
    let answer = match late.as_mut().poll(&mut context) {
        Poll::Ready(answer) => answer,
        Poll::Pending => panic!("released"),
    };
    assert!(
        !answer.as_ref().is_ok_and(collectable),
        "A's answer passed the exit collector's checks after the window followed B: {:?}",
        answer.as_ref().map(|seen| seen.items().len())
    );
    assert!(answer.is_err(), "a look a follow overtook gives no answer");
    let fresh = finished(window.observe()).unwrap();
    assert!(
        !collectable(&fresh),
        "the next look asks afresh: B's turn is open"
    );
}

#[test]
fn a_look_that_follows_keep_overtaking_ends_without_asking_again() {
    let home = Home::new();
    let (records, window) = held_window(&home);
    home.status(A, &json!({ "status": "idle" }), me());
    records.hold_holders();
    let mut look = window.observe();
    let mut context = Context::from_waker(Waker::noop());
    assert!(look.as_mut().poll(&mut context).is_pending());
    // The worker's answer goes out, and the pass follows the window to another conversation
    // before the look is polled; a look that asks again has its next answer held, and the
    // follows go on.
    let mut ended = None;
    for turn in 0..64 {
        records.release_holders();
        let next = if turn % 2 == 0 { B } else { A };
        home.status(next, &json!({ "status": "idle" }), me());
        pass_follows(&window, next);
        records.hold_holders();
        if let Poll::Ready(answer) = look.as_mut().poll(&mut context) {
            ended = Some((turn, answer));
            break;
        }
    }
    records.release_holders();
    let (turn, answer) = ended.expect("the look never ended over 64 follows");
    assert_eq!(turn, 0, "it ends at the first follow that overtook it");
    assert!(
        answer.is_err(),
        "no answer for the pass it was overtaken in"
    );
    assert!(
        finished(window.observe()).is_ok(),
        "the next pass asks afresh and is answered"
    );
}
