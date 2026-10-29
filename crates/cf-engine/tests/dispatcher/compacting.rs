//! A window at work is no window that lost what it was given (T-901). A Codex window
//! opened again on a long conversation compacted it for minutes before it took its first
//! message ("• Compacting context (2m 57s • esc to interrupt) / └ Making room to
//! continue."), and the first message's time ran out under it: the window was closed as
//! one that "never showed its first message", and the chief's tell in it was lost
//! (field, 3.0.6-alpha.2, 2026-10-28). So no delivery's time runs out while the window
//! shows it is at work, by its record or by its screen's compaction: it waits for the
//! harness to finish, and a compaction that never does is told once, to whoever sent the
//! message and to the human, by name. A window that shows neither work nor the prompt its
//! first message waits for is given up within its launch's time, as before, in words that
//! name what was waited for (T-863). And words told to a window that fail come back to
//! whoever told them, word for word.

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::{Context, COMPACTING, TRUST_PROMPT};
use cf_harness::contract::FirstMessage;
use cf_harness::records::Role;
use serde_json::json;

use crate::chiefs::to_human;
use crate::fixtures::notes;
use crate::withheld_enter::{handed, times_handed};

const SEAM: &str = "adapter:claude-code";

/// The launch's time a first message is given in these tests' context.
const LAUNCH_MS: i64 = 120_000;

/// A context whose windows are delivered their first message once up.
fn delivered() -> Context {
    let context = Context::new();
    context.adapter.first.set(FirstMessage::Delivered);
    context
}

/// Passes of the dispatcher, each settled.
fn passes(context: &Context, count: usize) {
    for _ in 0..count {
        context.pass().unwrap();
        context.settle();
    }
}

/// A project with zeus on its staff, T-1 given to zeus, and its brief handed to zeus's
/// window, whose record does not show it yet: the window's harness took it into a
/// queue of its own, as Codex's broker does. The window is on a conversation that holds
/// what was said before, as the field's was, opened again on its thread.
fn handed_its_brief(context: &Context) -> (i64, i64) {
    let project = context.with_staff(&["zeus"]).id;
    let brief = context
        .give(project, "zeus", "Parser")
        .message
        .expect("its brief")
        .id;
    context.pass().unwrap();
    context.settle();
    let before = context
        .adapter
        .item(Role::Assistant, "What the thread said before");
    context.adapter.with("zeus", |agent| {
        agent.arrive = false;
        agent.queued = true;
        agent.items.push(before);
    });
    passes(context, 2);
    assert_eq!(times_handed(context, SEAM, brief), 1, "handed once");
    (project, brief)
}

/// The window's screen from now on: its harness compacting its conversation.
fn compacting(context: &Context) {
    context.host.set_snapshot(json!({
        "tail": [COMPACTING, "└ Making room to continue.", "› Ask the agent to do anything"],
        "outputQuietMs": 5000,
    }));
}

/// The window's screen from now on: its prompt, nothing at work.
fn at_its_prompt(context: &Context) {
    context.host.set_snapshot(json!({
        "tail": ["› Ask the agent to do anything"],
        "outputQuietMs": 5000,
    }));
}

/// The compaction is over, and the window's record shows what it was handed for
/// `message`.
fn shows(context: &Context, message: i64) {
    let text = handed(context, SEAM)
        .into_iter()
        .rfind(|text| text.contains(&marker_of(message)))
        .expect("what it was handed");
    let item = context.adapter.item(Role::User, &text);
    context.adapter.with("zeus", |agent| agent.items.push(item));
    at_its_prompt(context);
}

#[test]
fn a_first_message_waits_for_a_window_compacting_past_its_launch_s_time_and_arrives_once_it_is_done(
) {
    let context = delivered();
    let (project, brief) = handed_its_brief(&context);
    // Its harness compacts the conversation before it takes the brief: a turn at work by
    // its record, the compaction on its screen.
    context.adapter.busy("zeus");
    compacting(&context);
    let killed = context.host.killed().len();
    for _ in 0..3 {
        context.advance(LAUNCH_MS + 1_000);
        passes(&context, 2);
    }
    assert_eq!(context.message(brief).state, "delivering", "waited for");
    assert_eq!(context.task(project, 1).task.state, "queued");
    assert_eq!(context.host.killed().len(), killed, "its window was kept");
    assert_eq!(times_handed(&context, SEAM, brief), 1, "never handed again");

    shows(&context, brief);
    passes(&context, 2);
    assert_eq!(context.message(brief).state, "delivered");
    context.close();
}

#[test]
fn a_first_message_waits_for_a_compaction_its_screen_shows_though_its_record_reads_at_rest() {
    let context = delivered();
    let (project, brief) = handed_its_brief(&context);
    // A harness whose record says nothing of a compaction under way: only its screen does.
    context.adapter.with("zeus", |agent| agent.settled = true);
    compacting(&context);
    let killed = context.host.killed().len();
    for _ in 0..2 {
        context.advance(LAUNCH_MS + 1_000);
        passes(&context, 2);
    }
    assert_eq!(context.message(brief).state, "delivering", "waited for");
    assert_eq!(context.task(project, 1).task.state, "queued");
    assert_eq!(context.host.killed().len(), killed, "its window was kept");

    // Done compacting, and at rest with nothing shown: its time runs from the last look
    // that found it compacting, and then it is given up as before.
    at_its_prompt(&context);
    context.advance(LAUNCH_MS + 1_000);
    passes(&context, 2);
    assert_eq!(context.message(brief).state, "failed");
    assert_eq!(context.task(project, 1).task.state, "failed");
    context.close();
}

#[test]
fn a_compaction_that_never_ends_is_told_once_to_the_requester_and_the_human_and_its_message_waits()
{
    let context = delivered();
    let (project, brief) = handed_its_brief(&context);
    context.adapter.busy("zeus");
    compacting(&context);
    let chief = context.id(project, "chief");
    let before = (
        notes(&context, chief).len(),
        to_human(&context, project).len(),
    );
    // Twenty minutes of it, then forty.
    for _ in 0..10 {
        context.advance(LAUNCH_MS + 1_000);
        passes(&context, 2);
    }
    let told = format!(
        "@zeus's window has been compacting its conversation for 15 min or more, and m-{brief}, \
         a task from @chief on T-1, waits for it to finish: it goes in once the window takes it, \
         and nothing is pasted again meanwhile. If the compaction is stuck, look at that window."
    );
    let to_chief = notes(&context, chief);
    assert_eq!(to_chief[before.0..], [told.as_str()], "the requester, once");
    let human = to_human(&context, project);
    assert_eq!(human.len(), before.1 + 1, "the human, once: {human:?}");
    assert_eq!(human[0], told);
    for _ in 0..10 {
        context.advance(LAUNCH_MS + 1_000);
        passes(&context, 2);
    }
    assert_eq!(notes(&context, chief).len(), before.0 + 1, "told once");
    assert_eq!(to_human(&context, project).len(), before.1 + 1, "told once");
    assert_eq!(context.message(brief).state, "delivering");
    assert_eq!(times_handed(&context, SEAM, brief), 1);
    context.close();
}

#[test]
fn a_later_message_is_not_pasted_again_while_its_window_compacts_at_rest_by_its_record() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]).id;
    context.pass().unwrap();
    // The chief's window takes the paste, and its record does not show it.
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project, "zeus", "chief", "One more thing");
    context.pass().unwrap();
    assert_eq!(times_handed(&context, SEAM, note.id), 1);
    // Its record reads at rest, and its screen says it compacts: the paste is waited for.
    compacting(&context);
    for _ in 0..6 {
        context.advance(31_000);
        passes(&context, 1);
    }
    assert_eq!(
        times_handed(&context, SEAM, note.id),
        1,
        "never pasted again"
    );
    let waiting = context.message(note.id);
    assert_eq!(
        (waiting.state.as_str(), waiting.attempts),
        ("delivering", 1)
    );
    // Done compacting, at rest: its whole arrival window from then, and pasted again after.
    at_its_prompt(&context);
    passes(&context, 1);
    context.advance(29_000);
    passes(&context, 1);
    assert_eq!(times_handed(&context, SEAM, note.id), 1);
    context.advance(2_000);
    passes(&context, 1);
    assert_eq!(
        times_handed(&context, SEAM, note.id),
        2,
        "pasted again once"
    );
    context.close();
}

#[test]
fn a_window_that_never_shows_the_prompt_its_first_message_waits_for_is_given_up_in_words_that_name_it(
) {
    let context = delivered();
    // A Claude Code that asks whether to trust its folder, and whose prompt never shows: its
    // screen is neither the question nor the prompt its reader knows.
    context.adapter.asks_to_trust("/work", true);
    context.host.set_snapshot(json!({
        "tail": ["Welcome to a harness that draws its status line otherwise"],
        "outputQuietMs": 5000,
    }));
    let project = context.with_staff(&["zeus"]).id;
    let brief = context
        .give(project, "zeus", "Parser")
        .message
        .expect("its brief")
        .id;
    passes(&context, 3);
    context.advance(LAUNCH_MS + 1_000);
    passes(&context, 3);
    assert_eq!(
        times_handed(&context, SEAM, brief),
        0,
        "nothing pasted blind"
    );
    assert_eq!(context.message(brief).state, "failed");
    let because = format!(
        "the window never showed the prompt its first message waits for, \
         a screen line with \"{TRUST_PROMPT}\""
    );
    let chief = context.id(project, "chief");
    let told = notes(&context, chief);
    assert!(
        told.iter()
            .any(|note| note.starts_with(&format!("T-1 failed: {because}"))),
        "the requester hears what was waited for: {told:?}"
    );
    let human = to_human(&context, project);
    assert!(
        human
            .iter()
            .any(|note| note.contains(&because) && note.contains(&format!("m-{brief}"))),
        "and the human: {human:?}"
    );
    // The watch of its screen ends at its next look, its window gone.
    while context.timers.fire_next() {
        context.settle();
    }
    context.close();
}

#[test]
fn a_window_owed_its_first_message_that_reads_at_work_is_waited_for_past_its_launch_s_time() {
    let context = delivered();
    context.adapter.asks_to_trust("/work", true);
    context.host.set_snapshot(json!({
        "tail": ["Resuming the conversation…"],
        "outputQuietMs": 5000,
    }));
    let project = context.with_staff(&["zeus"]).id;
    let brief = context
        .give(project, "zeus", "Parser")
        .message
        .expect("its brief")
        .id;
    context.pass().unwrap();
    context.settle();
    // Opened again on its conversation, which it is at work in.
    let before = context
        .adapter
        .item(Role::Assistant, "What the conversation said before");
    context
        .adapter
        .with("zeus", |agent| agent.items.push(before));
    context.adapter.busy("zeus");
    passes(&context, 2);
    let killed = context.host.killed().len();
    context.advance(LAUNCH_MS + 1_000);
    passes(&context, 3);
    assert_eq!(context.message(brief).state, "delivering", "waited for");
    assert_eq!(context.host.killed().len(), killed, "its window was kept");
    assert_eq!(
        times_handed(&context, SEAM, brief),
        0,
        "nothing pasted blind"
    );
    // Its prompt shows: the brief goes in.
    context.adapter.with("zeus", |agent| agent.settled = true);
    context
        .host
        .set_snapshot(json!({ "tail": [TRUST_PROMPT], "outputQuietMs": 5000 }));
    // The watch of its screen looks again (its sleep, which only the test ends).
    for _ in 0..4 {
        context.timers.fire_next();
        context.settle();
        passes(&context, 1);
    }
    assert_eq!(times_handed(&context, SEAM, brief), 1);
    context.close();
}

#[test]
fn words_told_to_a_window_that_never_reach_it_come_back_to_whoever_told_them_word_for_word() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]).id;
    context.give(project, "zeus", "Parser");
    passes(&context, 2);
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    let words = "Stop: use grammar v2,\nand keep `the $(lexer)` as it is";
    let told = context.tell(project, "zeus", 1, words);
    passes(&context, 1);
    // The window stops, and none of what is pasted into it ever shows in its record.
    context.adapter.with("zeus", |agent| agent.arrive = false);
    context.adapter.answer("zeus", "Half a parser");
    passes(&context, 1);
    for _ in 0..12 {
        context.advance(31_000);
        passes(&context, 1);
    }
    assert_eq!(context.message(told.id).state, "failed");
    let chief = context.id(project, "chief");
    let back = notes(&context, chief);
    assert!(
        back.iter()
            .any(|note| note.contains(&format!("m-{}", told.id))
                && note.contains("did not reach @zeus")
                && note.contains(words)),
        "whoever told them has them back, word for word: {back:?}"
    );
    context.close();
}

/// A window opened again on its conversation that its record reads at work for good (a
/// Claude Code whose status file says busy, and whose screen draws nothing the reader
/// knows): its first message is never given up, nor pasted blind, and whoever gave it and
/// the human are told once, at 15 min, that it waits on the window's work (Calliope's P3-4
/// of T-917: main gave it up in its launch's time).
#[test]
fn a_first_message_on_a_window_at_work_for_good_is_told_of_once_and_never_given_up() {
    let context = delivered();
    let (project, brief) = handed_its_brief(&context);
    context.adapter.busy("zeus");
    at_its_prompt(&context);
    let chief = context.id(project, "chief");
    let before = (
        notes(&context, chief).len(),
        to_human(&context, project).len(),
    );
    for _ in 0..10 {
        context.advance(LAUNCH_MS + 1_000);
        passes(&context, 2);
    }
    let told = format!(
        "@zeus's window has been at work for 15 min or more without taking m-{brief}, a task \
         from @chief on T-1, its first message, which waits for it: it goes in once the window \
         takes it, and nothing is pasted again meanwhile. If the window is stuck, look at it."
    );
    assert_eq!(
        notes(&context, chief)[before.0..],
        [told.as_str()],
        "the requester, once"
    );
    let human = to_human(&context, project);
    assert_eq!(human.len(), before.1 + 1, "the human, once: {human:?}");
    assert_eq!(human[0], told);
    for _ in 0..10 {
        context.advance(LAUNCH_MS + 1_000);
        passes(&context, 2);
    }
    assert_eq!(notes(&context, chief).len(), before.0 + 1, "told once");
    assert_eq!(context.message(brief).state, "delivering", "never given up");
    assert_eq!(context.task(project, 1).task.state, "queued");
    assert_eq!(times_handed(&context, SEAM, brief), 1, "never pasted again");
    context.close();
}

/// A later message that waited 15 min on a compaction is told of once, however many
/// times it is pasted: each paste is a new hand-over of the same message (Calliope's
/// P3-7 of T-917).
#[test]
fn a_message_told_of_for_a_compaction_is_not_told_of_again_when_it_is_pasted_again() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]).id;
    context.pass().unwrap();
    let before = context
        .adapter
        .item(Role::Assistant, "What the chief said before");
    context.adapter.with("chief", |agent| {
        agent.arrive = false;
        agent.items.push(before);
    });
    let note = context.note(project, "zeus", "chief", "One more thing");
    context.pass().unwrap();
    let compacting_for = |minutes: i64| {
        context.adapter.busy("chief");
        compacting(&context);
        for _ in 0..minutes {
            context.advance(60_000);
            passes(&context, 1);
        }
    };
    compacting_for(16);
    let told = |context: &Context| {
        to_human(context, project)
            .iter()
            .filter(|said| said.contains("has been compacting"))
            .count()
    };
    assert_eq!(told(&context), 1, "told once");
    // Done, at rest with nothing shown: pasted again, and compacting again as long.
    context.adapter.with("chief", |agent| agent.settled = true);
    at_its_prompt(&context);
    passes(&context, 1);
    context.advance(31_000);
    passes(&context, 1);
    assert_eq!(context.message(note.id).attempts, 2, "pasted again");
    compacting_for(16);
    assert_eq!(told(&context), 1, "not told again for the same message");
    context.close();
}

/// A launch whose window never came to rest with its screen drawn, so it was never asked
/// whether it is ready, is given up in words that say so (Calliope's P3-7 of T-917: the
/// default of `never_ready`).
#[test]
fn a_window_that_never_comes_to_rest_for_its_first_message_is_given_up_in_words_that_say_so() {
    let context = delivered();
    let project = context.with_staff(&["zeus"]).id;
    let brief = context
        .give(project, "zeus", "Parser")
        .message
        .expect("its brief")
        .id;
    context.pass().unwrap();
    context.settle();
    // Its record holds nothing, and its window never reads at rest.
    context.adapter.busy("zeus");
    passes(&context, 2);
    context.advance(LAUNCH_MS + 1_000);
    passes(&context, 3);
    assert_eq!(
        times_handed(&context, SEAM, brief),
        0,
        "nothing pasted blind"
    );
    assert_eq!(context.message(brief).state, "failed");
    let chief = context.id(project, "chief");
    let told = notes(&context, chief);
    assert!(
        told.iter().any(|note| note.starts_with(
            "T-1 failed: the window was never ready for its first message: it never came to \
             rest with its screen drawn"
        )),
        "{told:?}"
    );
    context.close();
}

/// The words that resume a paused task and never reach its window pause it again, and
/// whoever resumed it has them back, word for word, as a tell's teller does (Calliope's
/// P3-5 of T-917).
#[test]
fn the_words_that_resume_a_task_and_never_reach_its_window_come_back_word_for_word() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]).id;
    context.give(project, "zeus", "Parser");
    passes(&context, 2);
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    passes(&context, 1);
    context.adapter.answer("zeus", "Half a parser");
    passes(&context, 2);
    context.adapter.with("zeus", |agent| agent.admit = false);
    let words = "Go on with the v2 grammar,\nand keep `the $(lexer)` as it is";
    context.resume_task(project, 1, words);
    for _ in 0..8 {
        context.advance(31_000);
        passes(&context, 1);
    }
    assert_eq!(
        context.task(project, 1).task.state,
        "paused",
        "paused again"
    );
    let chief = context.id(project, "chief");
    let back = notes(&context, chief);
    assert!(
        back.iter()
            .any(|note| note.contains("not delivered") && note.contains(words)),
        "whoever resumed it has them back, word for word: {back:?}"
    );
    context.close();
}
