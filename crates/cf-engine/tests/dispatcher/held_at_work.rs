//! A chief's window that reads at work holds its queue, and from ten minutes on the
//! human is told so, once a message, and its lane says it (T-958). On Gabriel's other
//! Mac (2026-10-27, 3.0.6-alpha.2) the chief's lane read "working" for 54 hours while
//! its record showed it answering the human turn after turn: seven results waited,
//! queued, and nothing said a word, as the ten-minute note was told only of a message
//! held at a window at rest (T-839, T-883).

use cf_engine::testing::Context;
use cf_harness::contract::Waiting;
use cf_harness::records::Role;

use crate::fixtures::Tiers;

/// The notes the human was told of a message that waits for the chief's window.
fn told_waiting(tiers: &Tiers) -> Vec<String> {
    tiers
        .notes("human")
        .into_iter()
        .filter(|note| note.contains("has waited") && note.contains("@chief's window"))
        .collect()
}

fn after_minutes(context: &Context, minutes: i64) {
    context.advance(minutes * 60_000);
    context.pass().unwrap();
}

/// The chief's window reads at work for good after the worker has its brief, and the
/// worker's result is queued for it.
fn a_result_behind_a_chief_at_work(context: &Context) -> (Tiers<'_>, i64) {
    let tiers = Tiers::new(context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.settled = false);
    context.adapter.answer("zeus", "Parser done.");
    context.pass().unwrap();
    context.pass().unwrap();
    let chief = tiers.id("chief");
    let result = context
        .inbox(chief)
        .into_iter()
        .find(|message| message.kind == "result")
        .unwrap();
    assert_eq!(result.state, "queued");
    (tiers, result.id)
}

/// The field's shape: the record answers the human again and again, its turns never
/// read over. The human hears at ten minutes that the result waits, why, and that its
/// record answered meanwhile; the lane says it; the result goes in once the window rests.
#[test]
fn a_chief_read_at_work_while_its_record_answers_tells_the_human_what_waits_for_it() {
    let context = Context::new();
    let (tiers, result) = a_result_behind_a_chief_at_work(&context);
    let chief = tiers.id("chief");
    assert_eq!(
        context.dispatcher.held_back(chief).map(|held| held.message),
        Some(result),
        "the head is held from the first look"
    );
    after_minutes(&context, 4);
    let asked = context
        .adapter
        .item(Role::User, "how do I set the username?");
    context
        .adapter
        .with("chief", |agent| agent.items.push(asked));
    context
        .adapter
        .answer_never_over("chief", "In the admin console.");
    after_minutes(&context, 5);
    assert_eq!(told_waiting(&tiers), Vec::<String>::new(), "not yet");
    assert_eq!(
        context.dispatcher.activity(chief).reason,
        None,
        "nor its lane"
    );
    after_minutes(&context, 2);
    let notes = told_waiting(&tiers);
    assert_eq!(notes.len(), 1, "{:?}", tiers.notes("human"));
    assert!(
        notes[0].starts_with(&format!("m-{result}, a result from @")),
        "{}",
        notes[0]
    );
    assert!(notes[0].contains("reads at work"), "{}", notes[0]);
    assert!(notes[0].contains("has answered since"), "{}", notes[0]);
    let lane = context
        .dispatcher
        .activity(chief)
        .reason
        .unwrap_or_default();
    assert!(lane.contains(&format!("m-{result}")), "{lane}");
    after_minutes(&context, 60);
    assert_eq!(told_waiting(&tiers).len(), 1, "told once");
    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_ne!(
        context.message(result).state,
        "queued",
        "it goes in at the rest"
    );
    let lane = context
        .dispatcher
        .activity(chief)
        .reason
        .unwrap_or_default();
    assert!(!lane.contains(&format!("m-{result}")), "{lane}");
}

/// A chief at work on a long turn of its own, writing as it goes, with nothing of the
/// human's since: the human is told the result waits, in words that say its window is at
/// work and nothing of a misread.
#[test]
fn a_chief_at_work_on_a_long_turn_tells_the_human_once_that_a_result_waits() {
    let context = Context::new();
    let (tiers, result) = a_result_behind_a_chief_at_work(&context);
    after_minutes(&context, 3);
    for said in ["Reading the logs.", "Running the suite."] {
        let item = context.adapter.item(Role::Assistant, said);
        context
            .adapter
            .with("chief", |agent| agent.items.push(item));
        after_minutes(&context, 3);
    }
    after_minutes(&context, 2);
    let notes = told_waiting(&tiers);
    assert_eq!(notes.len(), 1, "{:?}", tiers.notes("human"));
    assert!(
        notes[0].starts_with(&format!("m-{result},")),
        "{}",
        notes[0]
    );
    assert!(notes[0].contains("reads at work"), "{}", notes[0]);
    assert!(!notes[0].contains("answered since"), "{}", notes[0]);
}

/// A chief at work with nothing queued for it is held for nothing, and says nothing.
#[test]
fn a_chief_at_work_with_nothing_queued_says_nothing() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.settled = false);
    after_minutes(&context, 30);
    let chief = tiers.id("chief");
    assert_eq!(context.dispatcher.held_back(chief), None);
    assert_eq!(told_waiting(&tiers), Vec::<String>::new());
    assert_eq!(context.dispatcher.activity(chief).reason, None);
}

/// A chief read at work while its harness says it waits at its prompt, as Claude's status
/// does of a window whose record never reads its turn over (T-526): the human hears that
/// ConsensFlow may be misreading it.
#[test]
fn a_chief_read_at_work_at_its_prompt_tells_the_human_it_may_be_misread() {
    let context = Context::new();
    let (tiers, result) = a_result_behind_a_chief_at_work(&context);
    context
        .adapter
        .with("chief", |agent| agent.at_prompt = true);
    after_minutes(&context, 11);
    let notes = told_waiting(&tiers);
    assert_eq!(notes.len(), 1, "{:?}", tiers.notes("human"));
    assert!(
        notes[0].starts_with(&format!("m-{result},")),
        "{}",
        notes[0]
    );
    assert!(notes[0].contains("waits at its prompt"), "{}", notes[0]);
}

/// A chief whose window shows a dialog of its harness's own: the human hears so, and the
/// dialog's words.
#[test]
fn a_chief_held_by_a_dialog_of_its_own_tells_the_human_what_it_shows() {
    let context = Context::new();
    let (tiers, result) = a_result_behind_a_chief_at_work(&context);
    context.adapter.with("chief", |agent| {
        agent.settled = true;
        agent.waiting = Some(Waiting {
            reason: Some("Allow this command?".to_owned()),
        });
    });
    after_minutes(&context, 11);
    let notes = told_waiting(&tiers);
    assert_eq!(notes.len(), 1, "{:?}", tiers.notes("human"));
    assert!(
        notes[0].starts_with(&format!("m-{result},")),
        "{}",
        notes[0]
    );
    assert!(
        notes[0].contains("shows a dialog of its own: Allow this command?"),
        "{}",
        notes[0]
    );
}
