//! What the check sees of the chief's turns. The chief is told by its folder to run a
//! command on each note and to wait for it ([`crate::check::note_command`]), which
//! ends by itself after [`HELD_S`] seconds: well over the daemon's arrival window (a
//! minute, its own), so that a harness that keeps what it is given in a turn shows it
//! later than the window, and the daemon is to wait for it. The turns are read from
//! what the daemon wrote of the chief's window (each change of what it was doing),
//! which no look of the check's can miss; and the run is over once the notes are given
//! and the chief has been at rest for a while, which a chief that was pasted a note
//! twice would not have: it would be in a turn on the second still.

use std::time::{Duration, Instant};

use cf_e2e::live::trace::at_ms;
use serde_json::Value;

/// How many seconds the command of a note's turn takes: twenty more than the
/// daemon's arrival window, and under the two minutes a Claude Code shell command is
/// given unless it asks for more.
pub const HELD_S: u64 = 80;

/// How long the chief has to be at rest, the notes all given, for them to be taken for
/// all it will be given: longer than a turn takes to begin once the one before it is
/// over.
pub const QUIET: Duration = Duration::from_secs(20);

/// Since when the chief has been at rest with the notes given.
#[derive(Debug, Default)]
pub struct Quiet {
    since: Option<Instant>,
}

impl Quiet {
    /// A look at `now`, at which the chief reads at rest with the notes all given or
    /// does not: whether it has been so for [`QUIET`].
    pub fn look(&mut self, at_rest: bool, now: Instant) -> bool {
        if at_rest {
            self.since.get_or_insert(now);
        } else {
            self.since = None;
        }
        self.since
            .is_some_and(|since| now.duration_since(since) >= QUIET)
    }
}

/// How many milliseconds `participant`'s window read at work from `since_ms` to
/// `until_ms`, among the daemon's `events` (their times are in milliseconds since the
/// epoch). A turn that began before `since_ms` counts from it, and one that has not
/// ended counts up to `until_ms`: a window that goes on to its next turn with no moment
/// at rest between them reads as one turn of both, and what it did before `since_ms`
/// is no part of what it did after.
pub fn worked_since(events: &[Value], participant: &str, since_ms: i64, until_ms: i64) -> i64 {
    let within = |began: i64, ended: i64| (ended - began.max(since_ms)).max(0);
    let mut began: Option<i64> = None;
    let mut worked = 0;
    for event in events {
        if event["kind"] != "window.activity" || event["participant"] != participant {
            continue;
        }
        let Some(at) = at_ms(event) else {
            continue;
        };
        match (event["state"].as_str(), began) {
            (Some("working"), None) => began = Some(at),
            (Some(state), Some(first)) if state != "working" => {
                began = None;
                worked += within(first, at);
            }
            _ => {}
        }
    }
    worked + began.map_or(0, |first| within(first, until_ms))
}

/// Whether `worked_ms` holds the time of a command for each of `notes` notes.
pub fn held(worked_ms: i64, notes: usize) -> bool {
    let command = i64::try_from(HELD_S)
        .unwrap_or(i64::MAX)
        .saturating_mul(1_000);
    worked_ms >= command.saturating_mul(i64::try_from(notes).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn at(start: Instant, seconds: u64) -> Instant {
        start + Duration::from_secs(seconds)
    }

    fn activity(participant: &str, state: &str, at: &str) -> Value {
        json!({ "at": at, "kind": "window.activity", "participant": participant, "state": state })
    }

    #[test]
    fn the_run_is_over_once_the_chief_has_been_at_rest_with_the_notes_given_for_the_quiet_time() {
        let start = Instant::now();
        let mut quiet = Quiet::default();
        assert!(!quiet.look(true, at(start, 0)));
        assert!(!quiet.look(true, at(start, 19)));
        assert!(quiet.look(true, at(start, 20)));
    }

    #[test]
    fn a_chief_that_goes_to_work_again_begins_the_quiet_time_again_and_one_not_at_rest_is_never_over(
    ) {
        let start = Instant::now();
        let mut quiet = Quiet::default();
        quiet.look(true, at(start, 0));
        // A turn on a note pasted again, 19 s on.
        assert!(!quiet.look(false, at(start, 19)));
        assert!(!quiet.look(true, at(start, 20)), "from 20, not from 0");
        assert!(!quiet.look(true, at(start, 39)));
        assert!(quiet.look(true, at(start, 40)));
        let mut waiting = Quiet::default();
        for seconds in [0, 30, 60, 600] {
            assert!(!waiting.look(false, at(start, seconds)));
        }
    }

    #[test]
    fn the_work_of_a_window_is_the_time_it_read_working_from_the_event_that_says_it_to_the_one_that_does_not(
    ) {
        let events = [
            // The human's own ask, begun before the notes were pasted.
            activity("chief", "working", "2026-10-13T10:00:00.000Z"),
            activity("chief", "idle", "2026-10-13T10:02:00.000Z"),
            // A note's turn, and the next's, with a moment at rest between them.
            activity("chief", "working", "2026-10-13T10:02:01.000Z"),
            activity("zeus", "working", "2026-10-13T10:02:05.000Z"),
            activity("zeus", "idle", "2026-10-13T10:02:06.000Z"),
            activity("chief", "idle", "2026-10-13T10:03:02.500Z"),
            activity("chief", "working", "2026-10-13T10:03:04.000Z"),
            activity("chief", "waiting", "2026-10-13T10:04:05.000Z"),
        ];
        let since = at_ms(&events[1]).unwrap();
        let until = at_ms(&events[7]).unwrap() + 60_000;
        assert_eq!(
            worked_since(&events, "chief", since, until),
            122_500,
            "the human's ask is before the notes, and what zeus did is its own"
        );
        assert_eq!(worked_since(&events, "chief", 0, until), 242_500);
        assert_eq!(worked_since(&events, "nobody", 0, until), 0);
    }

    #[test]
    fn a_turn_that_began_before_the_first_paste_counts_from_it_and_one_still_going_up_to_the_end() {
        // The first command's turn runs into the first note's with no moment at rest
        // between them: the window never read anything but working.
        let events = [activity("chief", "working", "2026-10-13T10:00:00.000Z")];
        let pasted = at_ms(&events[0]).unwrap() + 100_000;
        let end = pasted + 130_000;
        assert_eq!(worked_since(&events, "chief", pasted, end), 130_000);
        assert_eq!(worked_since(&events, "chief", pasted, pasted - 5_000), 0);
        // Only what comes after the time asked for is counted.
        assert_eq!(worked_since(&events, "chief", end, end + 1_000), 1_000);
    }

    #[test]
    fn a_turn_is_held_for_each_note_when_the_window_worked_the_time_of_the_command_for_each() {
        assert!(held(162_500, 2));
        assert!(!held(84_500, 2), "the second note was not waited on");
        assert!(
            !held(0, 1),
            "a chief that ran no command left no turn to hold"
        );
        assert!(held(80_000, 1));
        assert!(!held(79_999, 1));
    }
}
