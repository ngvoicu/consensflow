//! The window of a task that is done stays open while its own extension says it asked its
//! harness to compact the conversation at the window's rest for the project's size, and has
//! not said it over (Pi's `compacting/<launch>.json`, T-1475): a Pi member whose task is one
//! turn is at rest as its result is taken, and its summary takes 39 to 80 seconds, which the
//! close, ten seconds after the result, cut (the signal aborting it: the ledger of the owner's
//! cached compaction says `cancelled`). The result is the chief's at once as ever; only the
//! close waits, for the word to be taken back, or for the bound ([`COMPACTION_HOLD_MS`]).
//! A pause, a cancel, a failure and the human ending the session close at once.
//!
//! Not held to a Node recording: Node's dispatcher had no such word.

use cf_engine::testing::Context;
use cf_harness::contract::Pane;
use cf_harness::seams::Time;
use serde_json::json;

use crate::fixtures::{hold_notes, native_of, Tiers};

/// The longest the window waits for a compaction from its word: `windows.rs`'
/// `COMPACTION_HOLD_MS`.
const BOUND_MS: i64 = 300_000;

/// Zeus has T-1, delivered and at work in a window of its own: its pane. Its screen has been
/// still for a minute, so that the screen alone holds the window no longer.
fn at_work(context: &Context, project: i64) -> Pane {
    context.give(project, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    context
        .host
        .set_snapshot(json!({ "screenQuietMs": 60_000 }));
    context.host.last("zeus").unwrap().pane
}

/// The extension says it asked for a compaction `ago` ms ago (a negative `ago` is a word
/// whose time is ahead of the engine's clock), or takes its word back (`None`).
fn word(context: &Context, ago: Option<i64>) {
    let at = ago.map(|ago| context.time.wall_ms() - ago);
    context
        .adapter
        .with("zeus", |agent| agent.compaction_asked = at);
}

/// Zeus's agent answers, the extension asks for its compaction in the same moment, and the
/// engine looks at the window twice.
fn answers_compacting(context: &Context) {
    word(context, Some(0));
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
}

fn open(context: &Context, window: &Pane) -> bool {
    !context.host.killed().contains(window)
}

fn has_result(context: &Context, project: i64, number: i64, words: &str) -> bool {
    context
        .task(project, number)
        .messages
        .iter()
        .any(|message| message.kind == "result" && message.body.contains(words))
}

#[test]
fn a_done_tasks_window_stays_open_while_its_extension_compacts_and_closes_in_the_step_the_word_is_taken_back(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    answers_compacting(&context);
    // The result is the chief's at once; only the close waits.
    assert_eq!(context.task(project.id, 1).task.state, "done");
    assert!(has_result(&context, project.id, 1, "Parser done"));
    assert!(open(&context, &window), "its compaction is at work");

    // Eighty seconds on, the longest the records show, and still open; the chief accepts.
    context.advance(80_000);
    context
        .ledger
        .borrow_mut()
        .accept_task(project.id, 1, "chief")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "accepted");
    assert!(open(&context, &window), "accepted, it still waits");

    // The compaction ends: the word is taken back, and the window closes, once.
    word(&context, None);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), std::slice::from_ref(&window));
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window], "closed once");
    context.close();
}

#[test]
fn a_word_that_is_never_taken_back_holds_the_window_for_the_bound_from_the_word_and_no_longer() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    answers_compacting(&context);
    assert!(open(&context, &window));

    context.advance(BOUND_MS - 1);
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(open(&context, &window), "a millisecond before the bound");
    assert_eq!(context.task(project.id, 1).task.state, "done");

    // The bound is over, the word standing: the window closes as it does without one.
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window]);
    assert!(has_result(&context, project.id, 1, "Parser done"));
    context.close();
}

/// The bound is the engine's first sight of the word, on the clock that does not step, not the
/// word's own time: a word written 100 s before the result is taken is held the whole bound from
/// the look that found the window free.
#[test]
fn the_bound_runs_from_the_engines_first_sight_not_from_the_words_time() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    word(&context, Some(100_000));
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(open(&context, &window));
    context.advance(BOUND_MS - 1);
    context.pass().unwrap();
    assert!(open(&context, &window), "a millisecond before the bound");
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window]);
    context.close();
}

/// A word that is another (the extension asked again, its time another) is a new first sight, its
/// bound its own.
#[test]
fn a_new_word_is_a_new_first_sight_with_a_bound_of_its_own() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    answers_compacting(&context);
    context.advance(BOUND_MS - 50_000);
    context.pass().unwrap();
    assert!(open(&context, &window));
    // Another word, written now.
    word(&context, Some(0));
    context.pass().unwrap();
    context.advance(BOUND_MS - 1);
    context.pass().unwrap();
    assert!(
        open(&context, &window),
        "the first word's bound is over; the new word's is not"
    );
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window]);
    context.close();
}

/// The reviewer's case (T-1477): a word 120 s ahead of the clock held 420 s, the count switching
/// to its time once the clock reached it.
#[test]
fn a_word_ahead_of_the_clock_is_held_the_bound_from_the_first_sight_whenever_the_time_reaches_it() {
    for ahead in [120_000, 3_600_000] {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        let window = at_work(&context, project.id);
        word(&context, Some(-ahead));
        context.adapter.answer("zeus", "Parser done");
        context.pass().unwrap();
        context.pass().unwrap();
        assert!(open(&context, &window), "{ahead}");
        // Past the moment the word's own time is the clock's.
        context.advance(ahead.min(BOUND_MS - 100_000) + 10_000);
        context.pass().unwrap();
        assert!(open(&context, &window), "{ahead}: inside the bound");
        context.advance(BOUND_MS - 1 - (ahead.min(BOUND_MS - 100_000) + 10_000));
        context.pass().unwrap();
        assert!(
            open(&context, &window),
            "{ahead}: a millisecond before the bound"
        );
        context.advance(1);
        context.pass().unwrap();
        assert_eq!(context.host.killed(), [window], "{ahead}: at the bound");
        context.close();
    }
}

/// The wall clock set back during the hold lengthens nothing; set forward, shortens nothing: the
/// hold is 300 s on the clock that does not step.
#[test]
fn a_step_of_the_wall_clock_in_either_direction_leaves_the_hold_its_bound() {
    for step in [-3_600_000, -250_000, 250_000, 3_600_000] {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        let window = at_work(&context, project.id);
        answers_compacting(&context);
        assert!(open(&context, &window), "{step}");
        context.advance(100_000);
        context.pass().unwrap();
        context.step_wall(step);
        context.pass().unwrap();
        context.pass().unwrap();
        assert!(open(&context, &window), "{step}: the step ended nothing");
        context.advance(BOUND_MS - 100_000 - 1);
        context.pass().unwrap();
        assert!(
            open(&context, &window),
            "{step}: a millisecond before the bound"
        );
        context.advance(1);
        context.pass().unwrap();
        assert_eq!(context.host.killed(), [window], "{step}: at the bound");
        context.close();
    }
}

#[test]
fn a_window_whose_task_is_not_done_is_not_held_for_the_word_a_cancel_closes_it_at_once() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    word(&context, Some(0));
    context
        .ledger
        .borrow_mut()
        .cancel_task(project.id, 1, "human")
        .unwrap();
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window]);
    context.close();
}

#[test]
fn the_human_ending_the_session_closes_its_window_at_once_under_the_compaction() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    context
        .host
        .set_snapshot(json!({ "screenQuietMs": 60_000 }));
    let window = context.host.last("zeus").unwrap().pane;
    answers_compacting(&context);
    assert_eq!(tiers.task(1).task.state, "done");
    assert!(open(&context, &window), "its compaction is at work");
    context
        .end_session(tiers.project.id, "zeus-amber-pine")
        .unwrap();
    assert_eq!(context.host.killed(), [window]);
    context.close();
}

/// A follow-up for the session, queued behind the done task, does not go into the window
/// that holds the compaction (no other task goes into a window a task's message went into,
/// `ownership`): it waits for the window to close, its requester told nothing, and goes
/// into the window that opens after, once, on the conversation the compaction ended in.
#[test]
fn the_next_task_of_the_session_waits_for_the_compaction_and_goes_in_once_after_it() {
    let context = Context::new();
    // The host proves each window's program gone, as the app's does: the session's next task
    // goes on with its conversation (`ownership`).
    context.host.exits_showing(Some(0), None, None);
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    answers_compacting(&context);
    context.give(project.id, "zeus", "Lexer");
    let opened = context.host.opened().len();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(open(&context, &window), "its compaction is at work");
    assert_eq!(context.host.opened().len(), opened);
    assert_eq!(context.task(project.id, 2).task.state, "queued");
    assert!(
        hold_notes(&context, project.id, 2).is_empty(),
        "told nothing while it waits"
    );
    context.advance(60_000);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(open(&context, &window));
    assert_eq!(context.task(project.id, 2).task.state, "queued");

    word(&context, None);
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(context.host.killed(), std::slice::from_ref(&window));
    assert_eq!(context.host.opened().len(), opened + 1);
    assert_ne!(context.host.last("zeus").unwrap().pane, window);
    assert_eq!(context.task(project.id, 2).task.state, "working");
    context.close();
}

/// How many times the words of the reopen went into the window: the user items that hold them.
fn pastes(context: &Context, words: &str) -> usize {
    context
        .adapter
        .agent("zeus")
        .items
        .iter()
        .filter(|item| item.text.contains(words))
        .count()
}

/// A reopen during the hold keeps the window (a task in hand again keeps its window) and its words
/// wait for the compaction: Pi's extension answers Busy while it compacts (no attempt spent), and the
/// words go in once, into the conversation as it is after, when the word is taken back.
#[test]
fn a_reopen_during_the_hold_waits_for_the_compaction_and_goes_into_the_window_once_after_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    answers_compacting(&context);
    context.advance(10_000);
    context
        .ledger
        .borrow_mut()
        .reopen_task(project.id, 1, "chief", "Fix the tests")
        .unwrap();
    for _ in 0..4 {
        context.pass().unwrap();
        context.advance(15_000);
    }
    assert_eq!(context.task(project.id, 1).task.state, "queued");
    assert_eq!(
        pastes(&context, "Fix the tests"),
        0,
        "nothing goes in while it compacts"
    );
    assert!(open(&context, &window), "a task in hand keeps its window");

    word(&context, None);
    for _ in 0..4 {
        context.pass().unwrap();
        context.advance(15_000);
    }
    assert_eq!(pastes(&context, "Fix the tests"), 1, "once, after it");
    assert_eq!(context.task(project.id, 1).task.state, "working");
    assert!(open(&context, &window));
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(pastes(&context, "Fix the tests"), 1, "and not again");
    context.close();
}

/// Past the bound with the word still standing, the engine closes nothing under a task in hand: the
/// extension's own limit of the same length takes its word back (and aborts a compaction that never
/// ended), and the words go in once, into the same window, as it does.
#[test]
fn a_reopen_past_the_bound_with_the_word_standing_closes_no_window_under_its_task_and_goes_in_once()
{
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    answers_compacting(&context);
    context
        .ledger
        .borrow_mut()
        .reopen_task(project.id, 1, "chief", "Fix the tests")
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
        context.advance((BOUND_MS + 60_000) / 3);
    }
    assert_eq!(pastes(&context, "Fix the tests"), 0);
    assert_eq!(context.task(project.id, 1).task.state, "queued");
    assert!(
        open(&context, &window),
        "no window is closed under a task in hand"
    );

    // The extension's limit ends it.
    word(&context, None);
    for _ in 0..4 {
        context.pass().unwrap();
        context.advance(15_000);
    }
    assert_eq!(pastes(&context, "Fix the tests"), 1);
    assert_eq!(context.task(project.id, 1).task.state, "working");
    assert!(open(&context, &window));
    context.close();
}

/// The word's file is there and cannot be read this time (or no word yet): `Some(true)`; the
/// word as it was: `Some(false)`.
fn unreadable(context: &Context, yes: bool) {
    context
        .adapter
        .with("zeus", |agent| agent.compaction_unreadable = yes);
}

/// A read that fails is not a word that is gone (T-1486): the first sight stays, the hold is not
/// restarted by the next good read, and the close does not cut the compaction at once.
#[test]
fn a_read_that_fails_keeps_the_first_sight_and_the_next_good_read_does_not_start_a_new_bound() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    answers_compacting(&context);
    context.advance(200_000);
    context.pass().unwrap();
    // The file is there and cannot be read for a look or two.
    unreadable(&context, true);
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(open(&context, &window), "a failed read ends nothing");
    unreadable(&context, false);
    context.advance(BOUND_MS - 200_000 - 1);
    context.pass().unwrap();
    assert!(
        open(&context, &window),
        "a millisecond before the first sight's bound"
    );
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(
        context.host.killed(),
        [window],
        "the bound is the first sight's, not a fresh one from the good read"
    );
    context.close();
}

/// With no first sight yet, a word that cannot be read holds the window as drawing for the look and
/// is read again at the next; the bound runs from the first look that met it.
#[test]
fn a_word_unreadable_at_the_first_look_holds_the_window_and_is_bounded_from_that_look() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    unreadable(&context, true);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(open(&context, &window), "held for the look");
    assert_eq!(context.task(project.id, 1).task.state, "done");
    context.advance(BOUND_MS - 1);
    context.pass().unwrap();
    assert!(open(&context, &window), "a millisecond before the bound");
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(
        context.host.killed(),
        [window],
        "a file that stays unreadable is bounded"
    );
    context.close();
}

/// A word unreadable at the first look and read at the next goes on from that first look.
#[test]
fn a_word_that_becomes_readable_after_an_unreadable_first_look_keeps_that_look_s_bound() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    unreadable(&context, true);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    context.advance(100_000);
    unreadable(&context, false);
    word(&context, Some(0));
    context.pass().unwrap();
    assert!(open(&context, &window));
    context.advance(BOUND_MS - 100_000 - 1);
    context.pass().unwrap();
    assert!(open(&context, &window));
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window]);
    context.close();
}

/// Only a file that is absent ends the hold: after a failed read the word is taken back, and the
/// window closes in that step.
#[test]
fn only_an_absent_word_ends_the_hold_after_a_failed_read() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    answers_compacting(&context);
    unreadable(&context, true);
    context.pass().unwrap();
    assert!(open(&context, &window));
    unreadable(&context, false);
    word(&context, None);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window]);
    context.close();
}

/// What the chief's reopen, a `cf tell` of its own or an answer to a question of the worker's
/// puts to the window of a task that was reopened during the compaction.
#[derive(Clone, Copy, Debug)]
enum Words {
    Reopen,
    Tell,
    Answer,
}

const REOPEN: &str = "Fix the tests";
const TELL: &str = "Use the v2 grammar";
const ANSWER: &str = "Use the strict grammar";

impl Words {
    fn text(self) -> &'static str {
        match self {
            Words::Reopen => REOPEN,
            Words::Tell => TELL,
            Words::Answer => ANSWER,
        }
    }
}

/// The extension compacts at a rest and Pi answers Busy to the words the chief sends, each
/// queued for the window (no attempt spent). `Reopen` and `Tell`: zeus answered, T-1 is done and
/// the chief reopens it (a task in hand keeps its window) and tells it. `Answer`: zeus asked the
/// chief a question (T-1 waits, a task that cannot be reopened) and is at rest compacting when
/// the chief answers.
fn queued_for_the_window(context: &Context, project: i64, words: Words) {
    if let Words::Answer = words {
        // The turn ends on a question to the chief, which the engine puts to it as one.
        word(context, Some(0));
        context.adapter.answer(
            "zeus",
            "Question for you: which grammar should I use? I'm waiting for your answer.",
        );
        context.pass().unwrap();
        context.pass().unwrap();
        assert_eq!(context.task(project, 1).task.state, "waiting");
        let question = context
            .inbox(context.id(project, "chief"))
            .into_iter()
            .find(|message| message.kind == "question")
            .expect("its question")
            .id;
        context.advance(10_000);
        context.answer(project, question, ANSWER);
    } else {
        answers_compacting(context);
        context.advance(10_000);
        context
            .ledger
            .borrow_mut()
            .reopen_task(project, 1, "chief", REOPEN)
            .unwrap();
        if let Words::Tell = words {
            context
                .ledger
                .borrow_mut()
                .tell_queued(project, 1, "chief", TELL)
                .unwrap();
        }
    }
    for _ in 0..4 {
        context.pass().unwrap();
        context.advance(15_000);
    }
}

/// Each of the three kinds of words, sent to a reopened task during the compaction, goes into
/// the window once after it, and not before.
#[test]
fn a_tell_and_an_answer_to_a_reopened_task_go_into_the_same_window_once_after_the_compaction() {
    for words in [Words::Tell, Words::Answer] {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        let window = at_work(&context, project.id);
        let opened = context.host.opened().len();
        queued_for_the_window(&context, project.id, words);
        let text = words.text();
        assert_eq!(
            pastes(&context, text),
            0,
            "{words:?}: nothing while it compacts"
        );
        assert!(
            open(&context, &window),
            "{words:?}: a task in hand keeps its window"
        );

        word(&context, None);
        for _ in 0..4 {
            context.pass().unwrap();
            context.advance(15_000);
        }
        assert_eq!(pastes(&context, text), 1, "{words:?}: once, after it");
        assert!(open(&context, &window), "{words:?}: the same window");
        assert_eq!(
            context.host.opened().len(),
            opened,
            "{words:?}: no other window"
        );
        for _ in 0..3 {
            context.pass().unwrap();
        }
        assert_eq!(pastes(&context, text), 1, "{words:?}: and not again");
        context.close();
    }
}

/// T-1487 step 1: the Pi window's process ends by itself while the words wait for it (Pi
/// answered Busy). A reopened task (and a `cf tell` to it) is queued, in no window's hand: the
/// engine opens the window again on the same conversation with the words, once, and pauses
/// nothing. An answer's task waits in the window: it is paused, the chief is told once, and the
/// resume's words carry the answer into the new window, once, on the same conversation.
#[test]
fn a_window_that_ends_by_itself_under_queued_words_loses_and_doubles_none_of_them() {
    for words in [Words::Reopen, Words::Tell, Words::Answer] {
        let context = Context::new();
        context.host.exits_showing(Some(1), None, None);
        let project = context.with_staff(&["zeus"]);
        let window = at_work(&context, project.id);
        let native = native_of(&context, context.id(project.id, "zeus"));
        queued_for_the_window(&context, project.id, words);
        let text = words.text();
        let before = context.host.opened().len();
        assert_eq!(pastes(&context, text), 0, "{words:?}");
        let state = |context: &Context| context.task(project.id, 1).task.state;
        let told = |context: &Context| {
            context
                .inbox(context.id(project.id, "chief"))
                .into_iter()
                .filter(|message| message.kind == "note" && message.body.contains("is paused: "))
                .count()
        };

        // The process ends by itself; its extension's word goes with it.
        context.exit("zeus");
        word(&context, None);
        let paused = matches!(words, Words::Answer);
        if paused {
            for _ in 0..3 {
                context.pass().unwrap();
            }
            assert_eq!(state(&context), "paused", "{words:?}");
            assert_eq!(told(&context), 1, "{words:?}: the chief is told once");
            assert_eq!(
                context.host.opened().len(),
                before,
                "{words:?}: no window yet"
            );
            context.resume_task(project.id, 1, "Carry on");
        } else {
            assert_eq!(
                told(&context),
                0,
                "{words:?}: nothing is paused, nothing to tell"
            );
        }
        for _ in 0..4 {
            context.pass().unwrap();
            context.advance(15_000);
        }
        assert_eq!(state(&context), "working", "{words:?}");
        assert_eq!(
            context.host.opened().len(),
            before + 1,
            "{words:?}: one new window"
        );
        assert_ne!(context.host.last("zeus").unwrap().pane, window, "{words:?}");
        let launch = context
            .adapter
            .prepared()
            .into_iter()
            .rev()
            .find(|launch| {
                launch["participant"]["handle"]
                    .as_str()
                    .is_some_and(|handle| handle.starts_with("zeus"))
            })
            .expect("the new window's launch");
        assert_eq!(
            launch["resume"].as_str(),
            native.as_deref(),
            "{words:?}: on the same conversation"
        );
        assert_eq!(
            pastes(&context, text),
            1,
            "{words:?}: the words went in once"
        );
        assert_eq!(
            told(&context),
            usize::from(paused),
            "{words:?}: told no more"
        );
        for _ in 0..3 {
            context.pass().unwrap();
        }
        assert_eq!(pastes(&context, text), 1, "{words:?}: and not again");
        context.close();
    }
}

/// What the chief has been told, as notes and results, oldest first: the words.
fn chief_news(context: &Context, project: i64) -> Vec<String> {
    let mut news: Vec<String> = context
        .inbox(context.id(project, "chief"))
        .into_iter()
        .map(|message| format!("{}: {}", message.kind, message.body))
        .collect();
    news.reverse();
    news
}

/// T-1487 (a): the extension ends a Pi process that did not settle its compaction (T-1487's end of
/// the process), and the window's task is done with nothing queued: the engine finds the window
/// gone and writes nothing to the chief and pauses nothing, as for any window of a done task that
/// ends by itself (main writes none either: the same exit with no compaction in it is shown).
#[test]
fn a_done_tasks_window_whose_pi_ends_under_its_compaction_tells_the_chief_nothing_and_pauses_nothing(
) {
    let mut seen = Vec::new();
    for compacting in [true, false] {
        let context = Context::new();
        context.host.exits_showing(None, Some("SIGKILL"), None);
        let project = context.with_staff(&["zeus"]);
        let _window = at_work(&context, project.id);
        if compacting {
            answers_compacting(&context);
        } else {
            context.adapter.answer("zeus", "Parser done");
            context.pass().unwrap();
            context.pass().unwrap();
        }
        assert_eq!(context.task(project.id, 1).task.state, "done");
        let opened = context.host.opened().len();
        let before = chief_news(&context, project.id);
        context.exit("zeus");
        word(&context, None);
        for _ in 0..4 {
            context.pass().unwrap();
            context.advance(15_000);
        }
        assert_eq!(context.task(project.id, 1).task.state, "done");
        assert_eq!(
            context.host.opened().len(),
            opened,
            "no window opens for a done task"
        );
        let after = chief_news(&context, project.id);
        assert_eq!(
            after, before,
            "compacting: {compacting}: the chief is told nothing"
        );
        assert!(
            after.iter().all(|line| !line.contains("is paused")),
            "nothing is paused"
        );
        seen.push(after);
        context.close();
    }
    assert_eq!(
        seen[0], seen[1],
        "the same as for a window that ends with no compaction in it"
    );
}

/// T-1487 (b): after the extension ends a Pi that did not settle, the reopen's new window is on
/// the conversation as it was left (the same `resume`), a compaction hung again at its own rest is
/// ended the same way, and nothing is opened but once for each end.
#[test]
fn a_second_compaction_that_hangs_in_the_new_window_ends_it_the_same_way_once_a_rest() {
    let context = Context::new();
    context.host.exits_showing(None, Some("SIGKILL"), None);
    let project = context.with_staff(&["zeus"]);
    let _window = at_work(&context, project.id);
    let native = native_of(&context, context.id(project.id, "zeus"));
    let opened = context.host.opened().len();
    for (round, words) in ["Fix the tests", "Fix the lint"].into_iter().enumerate() {
        // The window compacts (its extension's word stands) and answers; the chief reopens.
        answers_compacting(&context);
        assert_eq!(
            context.task(project.id, 1).task.state,
            "done",
            "round {round}"
        );
        context
            .ledger
            .borrow_mut()
            .reopen_task(project.id, 1, "chief", words)
            .unwrap();
        for _ in 0..3 {
            context.pass().unwrap();
            context.advance(15_000);
        }
        assert_eq!(
            pastes(&context, words),
            0,
            "round {round}: Busy while it hangs"
        );
        assert_eq!(
            context.host.opened().len(),
            opened + round,
            "round {round}: no window yet"
        );
        // Its limit, its grace, the end of its process.
        context.advance(BOUND_MS + 15_000);
        context.exit("zeus");
        word(&context, None);
        for _ in 0..6 {
            context.pass().unwrap();
            context.advance(1_000);
        }
        assert_eq!(
            context.host.opened().len(),
            opened + round + 1,
            "round {round}: one window"
        );
        assert_eq!(pastes(&context, words), 1, "round {round}: the words once");
        assert_eq!(context.task(project.id, 1).task.state, "working");
        let launch = context
            .adapter
            .prepared()
            .into_iter()
            .rev()
            .find(|launch| {
                launch["participant"]["handle"]
                    .as_str()
                    .is_some_and(|handle| handle.starts_with("zeus"))
            })
            .expect("the new window's launch");
        assert_eq!(
            launch["resume"].as_str(),
            native.as_deref(),
            "round {round}: same conversation"
        );
        // The new window's own word is its own: none until it rests again, so nothing loops.
        assert_eq!(context.adapter.agent("zeus").compaction_asked, None);
        for _ in 0..4 {
            context.pass().unwrap();
            context.advance(1_000);
        }
        assert_eq!(
            context.host.opened().len(),
            opened + round + 1,
            "round {round}: no loop"
        );
    }
    context.close();
}

/// T-1487 (c): the note the chief gets when a window ends under an answer to a question: a pause
/// note that says what to run, and what ended it (the extension's kill, a signal ConsensFlow did
/// not send; or the exit of Pi's own graceful shutdown, which ends with code 0).
#[test]
fn the_chief_is_told_what_to_do_when_a_window_ends_under_an_answer_to_a_question() {
    for (code, signal, said) in [
        (
            None,
            Some("SIGKILL"),
            " (ended by signal SIGKILL, which ConsensFlow did not send: it had sent no kill)",
        ),
        (Some(0), None, " (exit code 0)"),
    ] {
        let context = Context::new();
        context.host.exits_showing(code, signal, None);
        let project = context.with_staff(&["zeus"]);
        let _window = at_work(&context, project.id);
        queued_for_the_window(&context, project.id, Words::Answer);
        context.exit("zeus");
        word(&context, None);
        for _ in 0..3 {
            context.pass().unwrap();
        }
        let notes: Vec<String> = chief_news(&context, project.id)
            .into_iter()
            .filter(|line| line.contains("is paused"))
            .collect();
        assert_eq!(
            notes,
            [format!(
                "note: T-1 is paused: @zeus's window closed{said}. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
            )]
        );
        context.close();
    }
}
