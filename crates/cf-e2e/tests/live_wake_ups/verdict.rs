//! What each check of a run holds the evidence to, as a pure judgment of what
//! was read (`evidence`): the daemon's event file, the harness's own record of
//! the chief's conversation, the board and the human's inbox. Each says whether
//! it held and what was seen, which the run puts in its account. Nothing here is
//! what an agent said it did.

use cf_e2e::live::ask::Item;

use crate::evidence::{answered, given, header, moves, WakeUp};
use crate::words::{EVERY_MS, FIRST_BY_MS};

/// What a check came to: whether it held, and what was seen.
pub type Judged = (bool, String);

/// Seconds, to a decimal, of a span of milliseconds.
fn secs(ms: i64) -> String {
    format!("{:.1} s", ms as f64 / 1000.0)
}

/// The first wake-up: the calls were turned on a minute apart, and one was
/// queued, no sooner than the minute and not long after it.
pub fn the_first(setting_ms: i64, wakes: &[WakeUp]) -> Judged {
    match wakes {
        [] => (false, "no wake-up was queued".to_owned()),
        [only] => {
            let after = only.at_ms - setting_ms;
            (
                (EVERY_MS - 5_000..=FIRST_BY_MS).contains(&after),
                format!(
                    "m-{} was queued {} after the calls were turned on, a minute apart",
                    only.message,
                    secs(after)
                ),
            )
        }
        more => (
            false,
            format!("{} wake-ups were queued, not one: {more:?}", more.len()),
        ),
    }
}

/// The wake-up reached the chief once: the harness's own record holds its words
/// in one user item, under the header of the note the log says was queued.
pub fn reached_once(items: &[Item], words: &str, message: i64) -> Judged {
    let at = given(items, words);
    match at.as_slice() {
        [only] => {
            let headed = items[*only].text.contains(&header(message));
            (
                headed,
                if headed {
                    format!(
                        "the record holds it once, in item {only}, under the header of m-{message}"
                    )
                } else {
                    format!("the record holds it once, but not under the header of m-{message}")
                },
            )
        }
        [] => (
            false,
            "the harness's record holds no item with the words".to_owned(),
        ),
        more => (
            false,
            format!(
                "the harness's record holds the words {} times: items {more:?}",
                more.len()
            ),
        ),
    }
}

/// The chief answered the wake-up it was given: the record holds words of its
/// own after it.
pub fn the_chief_answered(items: &[Item], words: &str) -> Judged {
    match given(items, words).last() {
        Some(at) if answered(items, *at) => (true, format!("an answer follows item {at}")),
        Some(at) => (false, format!("nothing of the chief's follows item {at}")),
        None => (
            false,
            "the record holds no such wake-up to answer".to_owned(),
        ),
    }
}

/// The reply stayed in the chief's terminal: after the first wake-up nothing
/// moved on the board, no task was made, and nothing came to the human.
pub fn gained_nothing(moved: &[String], tasks: usize, human_inbox: usize) -> Judged {
    (
        moved.is_empty() && tasks == 0 && human_inbox == 0,
        format!(
            "{} moved in the log, {tasks} tasks on the board, {human_inbox} messages in the human's inbox",
            if moved.is_empty() { "nothing".to_owned() } else { moved.join(", ") }
        ),
    )
}

/// No wake-up after the first came with nothing between: each one that did has
/// something that moved since the one before it (a task, or a message sent). A
/// chief that acted on the first may have moved the board, and is nudged again
/// for it; a quiet board is not.
pub fn quiet(events: &[serde_json::Value], wakes: &[WakeUp]) -> Judged {
    let mut unprompted = Vec::new();
    for pair in wakes.windows(2) {
        let between = moves(events, pair[0].n, Some(pair[1].n), wakes);
        if between.is_empty() {
            unprompted.push(format!("m-{} at event {}", pair[1].message, pair[1].n));
        }
    }
    (
        unprompted.is_empty(),
        if unprompted.is_empty() {
            format!(
                "{} wake-up{} in all, each after something moved",
                wakes.len(),
                if wakes.len() == 1 { "" } else { "s" }
            )
        } else {
            format!(
                "a wake-up came with nothing moved since the one before: {}",
                unprompted.join(", ")
            )
        },
    )
}

/// The next wake-up, once a task moved and the words were edited: queued a
/// minute after the edit, and the note holds the edited words as they were
/// written (the log's note, whose body is `body`).
pub fn the_next(edit_ms: i64, wake: Option<&WakeUp>, body: Option<&str>, words: &str) -> Judged {
    let Some(wake) = wake else {
        return (false, "no wake-up followed the move of a task".to_owned());
    };
    let after = wake.at_ms - edit_ms;
    let timely = (EVERY_MS - 5_000..=FIRST_BY_MS).contains(&after);
    let intact = body.is_some_and(|body| body.replace("\r\n", "\n") == words);
    (
        timely && intact,
        format!(
            "m-{} was queued {} after the words were edited; its body is {}",
            wake.message,
            secs(after),
            if intact {
                "the edited words, whole"
            } else {
                "not the edited words"
            }
        ),
    )
}

/// The edited words arrived intact: the record holds them whole in one user
/// item, which the chief answered.
pub fn edited_words_arrived(items: &[Item], words: &str, message: i64) -> Judged {
    let (reached, said) = reached_once(items, words, message);
    let (answered, answer) = the_chief_answered(items, words);
    (reached && answered, format!("{said}; {answer}"))
}

/// Nothing went into the chief's turn: no wake-up was queued while it worked,
/// from when its window read working until it read at rest (the wake-up queued
/// as it came to rest is the one that was held back for the turn).
pub fn not_while_busy(wakes: &[WakeUp], from_ms: i64, to_ms: i64) -> Judged {
    let inside: Vec<i64> = wakes
        .iter()
        .filter(|wake| (from_ms..to_ms).contains(&wake.at_ms))
        .map(|wake| wake.message)
        .collect();
    (
        inside.is_empty(),
        format!(
            "{} of wake-ups queued in the {} the chief worked",
            if inside.is_empty() {
                "none".to_owned()
            } else {
                format!("{inside:?}")
            },
            secs(to_ms - from_ms)
        ),
    )
}

/// The wake-up that was due during the turn came after it, once. The turn is
/// the one the human typed `answer` into (the last user item that says it) and
/// the chief's answer to it (the last assistant item that says it); the record
/// holds, after what was typed, one user item with the wake-up's words, and it
/// comes after the answer: not in the turn, and no other. (A wake-up before the
/// turn is an earlier one, which has been judged.)
pub fn followed_the_turn(items: &[Item], words: &str, answer: &str) -> Judged {
    let said = |role: &str| {
        items
            .iter()
            .rposition(|item| item.role == role && item.text.contains(answer))
    };
    let (Some(typed), Some(turn)) = (said("user"), said("assistant")) else {
        return (
            false,
            format!("the record holds no turn that was told to say {answer}, and said it"),
        );
    };
    let since: Vec<usize> = given(items, words)
        .into_iter()
        .filter(|at| *at > typed)
        .collect();
    match since.as_slice() {
        [] => (
            false,
            format!("no wake-up follows the words typed at item {typed}, answered at item {turn}"),
        ),
        [inside, ..] if *inside < turn => (
            false,
            format!("a wake-up is item {inside}, in the turn of items {typed} to {turn}"),
        ),
        [after] => (
            true,
            format!("the wake-up is item {after}, after the turn of items {typed} to {turn}"),
        ),
        more => (
            false,
            format!(
                "{} wake-ups follow the words typed at item {typed}: items {more:?}",
                more.len()
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;

    fn wake(n: i64, at_ms: i64, message: i64) -> WakeUp {
        WakeUp { n, at_ms, message }
    }

    fn item(role: &str, text: &str) -> Item {
        Item {
            id: format!("{role}-{text}"),
            role: role.to_owned(),
            text: text.to_owned(),
            complete: true,
        }
    }

    fn event(n: i64, kind: &str, data: Value) -> Value {
        json!({ "n": n, "at": "2026-10-13T10:00:00.000Z", "kind": kind, "data": data })
    }

    #[test]
    fn the_first_wake_up_is_one_queued_a_minute_after_the_calls_were_turned_on_and_not_long_after()
    {
        let one = |after: i64| the_first(1_000, &[wake(5, 1_000 + after, 4)]);
        assert!(one(60_000).0);
        assert!(
            one(55_000).0,
            "a few seconds under, for the clock of a pass"
        );
        assert!(one(FIRST_BY_MS).0);
        assert!(
            !one(30_000).0,
            "before the minute: the interval was not kept"
        );
        assert!(
            !one(FIRST_BY_MS + 1).0,
            "long after it: the chief was never nudged on time"
        );
        assert!(!the_first(0, &[]).0);
        assert!(
            !the_first(0, &[wake(1, 60_000, 1), wake(2, 120_000, 2)]).0,
            "two is a burst"
        );
        assert!(one(60_000).1.contains("m-4 was queued 60.0 s after"));
    }

    #[test]
    fn a_wake_up_reached_the_chief_once_when_one_user_item_holds_its_words_under_its_header() {
        let words = "Do you still have work to do?";
        let headed = item(
            "user",
            "[ConsensFlow m-4 · note from @human]\nDo you still have work to do?",
        );
        assert!(reached_once(std::slice::from_ref(&headed), words, 4).0);
        assert!(
            !reached_once(&[headed.clone(), headed.clone()], words, 4).0,
            "twice"
        );
        assert!(!reached_once(&[], words, 4).0, "never");
        assert!(
            !reached_once(&[item("user", "Do you still have work to do?")], words, 4).0,
            "the words alone, not a delivery of the note"
        );
        assert!(
            !reached_once(&[headed], words, 5).0,
            "another note's header"
        );
    }

    #[test]
    fn the_chief_answered_when_words_of_its_own_follow_the_last_wake_up() {
        let words = "Anything left?";
        let asked = item("user", "[ConsensFlow m-4 ·\nAnything left?");
        assert!(the_chief_answered(&[asked.clone(), item("assistant", "No.")], words).0);
        assert!(!the_chief_answered(std::slice::from_ref(&asked), words).0);
        assert!(!the_chief_answered(&[], words).0);
    }

    #[test]
    fn the_reply_stayed_in_the_terminal_when_nothing_moved_no_task_was_made_and_the_human_got_nothing(
    ) {
        assert!(gained_nothing(&[], 0, 0).0);
        for (moved, tasks, inbox) in [(vec!["message.sent"], 0, 0), (vec![], 1, 0), (vec![], 0, 1)]
        {
            let moved: Vec<String> = moved.into_iter().map(str::to_owned).collect();
            let (ok, said) = gained_nothing(&moved, tasks, inbox);
            assert!(!ok, "{said}");
        }
        assert!(gained_nothing(&[], 0, 0).1.starts_with("nothing moved"));
    }

    #[test]
    fn a_wake_up_that_came_after_a_quiet_stretch_is_the_failure_and_one_after_a_move_is_not() {
        let events = [
            event(1, "message.sent", json!({ "message": 4 })),
            event(2, "project.wake_up", json!({ "message": 4 })),
            event(3, "task.created", json!({})),
            event(4, "message.sent", json!({ "message": 8 })),
            event(5, "project.wake_up", json!({ "message": 8 })),
            event(6, "window.activity", json!({})),
            event(7, "message.sent", json!({ "message": 9 })),
            event(8, "project.wake_up", json!({ "message": 9 })),
        ];
        let wakes = crate::evidence::wake_ups(&events);
        assert_eq!(wakes.len(), 3);
        let (ok, said) = quiet(&events, &wakes);
        assert!(!ok, "{said}");
        assert!(said.contains("m-9"), "{said}");
        let (ok, said) = quiet(&events, &wakes[..2]);
        assert!(ok, "{said}");
        assert!(
            quiet(&events, &wakes[..1]).0,
            "one wake-up and no more is quiet"
        );
    }

    #[test]
    fn the_next_wake_up_is_timely_and_carries_the_edited_words_as_they_were_written() {
        let words = "Anything left?\nIf not, say so.";
        let next = wake(9, 61_000, 8);
        assert!(the_next(1_000, Some(&next), Some(words), words).0);
        assert!(
            the_next(
                1_000,
                Some(&next),
                Some("Anything left?\r\nIf not, say so."),
                words
            )
            .0
        );
        assert!(
            !the_next(
                1_000,
                Some(&next),
                Some("Anything left? If not, say so."),
                words
            )
            .0
        );
        assert!(!the_next(1_000, Some(&next), None, words).0);
        assert!(!the_next(1_000, None, Some(words), words).0);
        assert!(
            !the_next(40_000, Some(&next), Some(words), words).0,
            "21 s after the edit"
        );
    }

    #[test]
    fn edited_words_arrived_when_the_record_holds_them_whole_under_their_header_and_they_were_answered(
    ) {
        let words = "Anything left?\nIf not, say so.";
        let items = [
            item(
                "user",
                "[ConsensFlow m-8 · note from @human]\nAnything left?\nIf not, say so.",
            ),
            item("assistant", "Nothing."),
        ];
        assert!(edited_words_arrived(&items, words, 8).0);
        assert!(!edited_words_arrived(&items[..1], words, 8).0, "unanswered");
        assert!(
            !edited_words_arrived(&items, "Anything left? If not, say so.", 8).0,
            "the line break was lost"
        );
    }

    #[test]
    fn nothing_was_queued_while_the_chief_worked_and_the_wake_up_came_after_the_turn() {
        let wakes = [wake(3, 10_000, 4), wake(9, 200_000, 8)];
        assert!(not_while_busy(&wakes, 50_000, 150_000).0);
        let (ok, said) = not_while_busy(&wakes, 150_000, 250_000);
        assert!(!ok && said.contains("[8]"), "{said}");
        assert!(
            !not_while_busy(&wakes, 10_000, 12_000).0,
            "the turn's start counts"
        );
        assert!(
            not_while_busy(&wakes, 150_000, 200_000).0,
            "the wake-up queued as the turn ended is the one held back for it"
        );
    }

    #[test]
    fn the_wake_up_that_fell_due_in_the_turn_is_the_one_after_it_though_an_earlier_one_holds_the_same_words(
    ) {
        let words = "Anything left?";
        let wake = |message: i64| {
            item(
                "user",
                &format!("[ConsensFlow m-{message} ·\nAnything left?"),
            )
        };
        // The record of a whole run: the edited words came once before the turn, and again after it.
        let items = [
            wake(4),
            item("assistant", "Nothing."),
            item("user", "Run the command, then reply: BUSY_DONE"),
            item("tool", "done"),
            item("assistant", "BUSY_DONE"),
            wake(9),
            item("assistant", "Nothing."),
        ];
        let (ok, said) = followed_the_turn(&items, words, "BUSY_DONE");
        assert!(ok, "{said}");
        assert!(said.contains("item 5"), "{said}");
        // Pasted into the turn: the one after the words were typed is before the answer.
        let inside = [
            wake(4),
            item("user", "Run the command, then reply: BUSY_DONE"),
            wake(9),
            item("assistant", "BUSY_DONE"),
        ];
        let (ok, said) = followed_the_turn(&inside, words, "BUSY_DONE");
        assert!(!ok && said.contains("in the turn"), "{said}");
        // Never came: the earlier one is no answer to what fell due.
        let (ok, said) = followed_the_turn(&items[..5], words, "BUSY_DONE");
        assert!(!ok && said.contains("no wake-up follows"), "{said}");
        // Two came.
        let twice = [&items[..6], &[wake(10)]].concat();
        let (ok, said) = followed_the_turn(&twice, words, "BUSY_DONE");
        assert!(!ok && said.contains("2 wake-ups follow"), "{said}");
        // No turn that said it.
        assert!(!followed_the_turn(&items[5..], words, "BUSY_DONE").0);
        assert!(!followed_the_turn(&items[..3], words, "BUSY_DONE").0);
    }
}
