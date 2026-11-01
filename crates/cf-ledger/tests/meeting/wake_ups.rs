//! Wake-ups and a meeting: none is queued while the human talks with the chief, and
//! the interval counts again from the meeting's end, so the Stop is not followed by
//! a nudge at once. One queued before the meeting began is held as any message is.

use cf_ledger::WakeUpUnit;

use crate::world::{world, MINUTE};
use crate::{head, held};

#[test]
fn no_wake_up_is_queued_during_a_meeting_and_the_calls_count_again_from_its_end() {
    let mut w = world();
    w.turn_on(5, WakeUpUnit::Minutes);
    w.ledger.set_meeting(w.project.id, true).expect("a meeting");
    w.advance(30 * MINUTE);
    assert!(w.due().is_none(), "none during the meeting");
    assert!(
        !w.kinds()
            .iter()
            .any(|kind| kind == "project.wake_up_skipped"),
        "and no skip: the meeting is the human's, no work on its way"
    );
    w.ledger.set_meeting(w.project.id, false).expect("stopped");
    assert!(w.due().is_none(), "the interval counts from the Stop");
    w.advance(5 * MINUTE - 1);
    assert!(w.due().is_none());
    w.advance(1);
    assert!(w.due().is_some(), "an interval after the Stop");
}

#[test]
fn a_wake_up_queued_before_the_meeting_is_held_by_it() {
    let mut w = world();
    w.turn_on(5, WakeUpUnit::Minutes);
    w.advance(5 * MINUTE);
    let sent = w.due().expect("due");
    w.ledger.set_meeting(w.project.id, true).expect("a meeting");
    assert_eq!(head(&w), None);
    assert_eq!(held(&w), vec![sent.id]);
}
