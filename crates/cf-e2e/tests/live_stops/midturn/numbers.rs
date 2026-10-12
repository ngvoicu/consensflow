//! The numbers case's reading of a member at work: it is asked to write the whole
//! numbers as its reply, and is taken to be writing them when its numbers are seen, on
//! the text of its screen (a line of them each) or on the screen its pane host draws
//! (how far they have come), or when it has been at work for long enough to be, seen or
//! not (a TUI may draw them as something no text reads as numbers: its turn is at work
//! all the same).

use std::time::Duration;

use cf_e2e::live::members::{plural, MemberWindow, Members};

use super::begin::Doing;
use super::printed::{numbers_on, progress};
use super::{screen_drawn, NUMBERS};

/// What the member is asked to do, in the words the case says it in.
pub const DOING: Doing = Doing {
    to: "write its numbers",
    did: "wrote its numbers",
    doing: "writing its numbers",
    shown: "numbers on its screen",
};

/// How many numbers say the reply is being written: lines of numbers in the text of
/// the window's screen, or the highest number on the screen the pane host draws.
const WRITING: usize = 40;

/// How long a window at work is given to show its numbers before the case goes on
/// without having seen them, in a live run.
pub const UNSEEN: Duration = Duration::from_secs(90);

/// Whether the member whose lane reads working is writing its numbers: the text of
/// its screen has lines of them (`in_text`), or the screen the pane host draws shows
/// that far (`drawn`, asked only when the text is not enough), or it has been at work
/// for `worked`, which is `unseen` or more: long enough to be, seen or not.
fn is_writing(
    worked: Duration,
    unseen: Duration,
    in_text: usize,
    drawn: impl FnOnce() -> usize,
) -> bool {
    in_text >= WRITING || worked >= unseen || drawn() >= WRITING
}

/// How far the numbers have come on the screen the pane host draws for `window`.
fn drawn_by(members: &Members, window: &MemberWindow) -> usize {
    progress(&screen_drawn(members, window).join("\n"), NUMBERS)
}

/// The reading of a member at work: whether it is writing its numbers ([`is_writing`],
/// with `unseen` as the time it is given to show them), and what was seen if it is.
pub fn writing(
    unseen: Duration,
) -> impl FnMut(&Members, &MemberWindow, Duration) -> Option<String> {
    move |members, window, worked| {
        let in_text = numbers_on(&members.rig().screen(window.id()));
        let mut drawn = None;
        let ready = is_writing(worked, unseen, in_text, || {
            *drawn.insert(drawn_by(members, window))
        });
        ready.then(|| {
            let drawn = drawn.unwrap_or_else(|| drawn_by(members, window));
            format!(
                "{} in the text of its screen, and as far as {drawn} on the screen the pane host draws",
                plural(in_text, "line")
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_is_writing_when_its_text_shows_numbers_its_screen_does_or_it_worked_long_enough() {
        let not_asked = || -> usize { panic!("the pane host is not asked") };
        let unseen = UNSEEN;
        // Lines of numbers in the text of the screen are enough, and the host is not asked.
        assert!(is_writing(Duration::ZERO, unseen, WRITING, not_asked));
        // So is a long time at work, whatever the screen shows.
        assert!(is_writing(unseen, unseen, 0, not_asked));
        // Neither: the screen the host draws decides, as far as the numbers have come.
        let a_while = Duration::from_secs(10);
        assert!(!is_writing(a_while, unseen, WRITING - 1, || WRITING - 1));
        assert!(is_writing(a_while, unseen, 3, || WRITING));
        assert!(!is_writing(
            unseen - Duration::from_millis(1),
            unseen,
            0,
            || 0
        ));
    }
}
