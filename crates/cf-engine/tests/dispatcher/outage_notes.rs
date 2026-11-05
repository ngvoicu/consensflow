//! **One note for one cause, and no note of a failure that is no longer true** (the field
//! board of 2026-11-03, T-1196): the Mac's DNS dropped for a moment, six working tasks failed
//! on the harness's "API Error: Can't reach the API server — check your internet or DNS
//! (ENOTFOUND)" over 25 minutes (the longest wait between two of them 9 minutes 47 seconds),
//! and the chief, given its notes one at a time, took 13 of them: a "failed" for each task,
//! a "waits for" for each waiter, a "window closed" for each window, most of them after it
//! had reopened the six.
//!
//! The rule: the windows of one outage give up each at its own pace, and the requester, who
//! has not been given the first note yet, is told of all of them in one: which tasks failed
//! (with the words of each), that the same error from several windows looks like an outage of
//! the network or the API, what waits for them, what their windows' closes ended, and one line
//! that reopens them all. A failure that comes half an hour after the last, one with other
//! words, one the requester was given the note of already, and a single failure are notes
//! of their own, as they always were.
//!
//! And a note not yet given is not given when it is no longer true: a task reopened, called
//! off or accepted leaves the note that names it, which is written again for the tasks that
//! still failed, or withdrawn.
//!
//! The chief's window is not up in these cases, so what it is told stays queued, as it does
//! while its turn goes on.

use cf_engine::testing::Context;
use cf_ledger::{NewTask, SAME_ERROR_WITHIN_MS};
use serde_json::json;

use crate::fixtures::Tiers;

/// The harness's words of the field board's outage.
const ENOTFOUND: &str =
    "API Error: Can't reach the API server — check your internet or DNS (ENOTFOUND)";
/// What another outage says.
const CAPACITY: &str = "Selected model is at capacity. Please try a different model.";
/// What the host lists when it closes a window.
const LOCK: &str = "python3 lock.py record --run 7";

const MINUTE: i64 = 60_000;

/// Six standard workers (the tiered staff has a light one of its own, hera).
const SIX: [&str; 6] = ["zeus", "diana", "apollo", "athena", "ares", "hermes"];

/// T-1 to T-6, each at work in a window of its own.
fn six_at_work(context: &Context) -> Tiers<'_> {
    let tiers = Tiers::new(context, &SIX);
    for _ in SIX {
        tiers.open();
    }
    context.pass().unwrap();
    context.pass().unwrap();
    for number in 1..=6 {
        assert_eq!(tiers.task(number).task.state, "working", "T-{number}");
    }
    busy_chief(context);
    tiers
}

/// The chief is at work in its turn, as it is while it reads one note: what it is told waits,
/// queued, for its rest.
fn busy_chief(context: &Context) {
    context.adapter.with("chief", |agent| agent.at_work = true);
}

/// T-`number`'s window ends its turn with `words` as its harness's failure, and the
/// engine looks.
fn fails(context: &Context, tiers: &Tiers<'_>, number: i64, words: &str) {
    let window = tiers.assignee(number);
    context.adapter.with(&window, |agent| {
        agent.failed = true;
        agent.failure = Some(words.to_owned());
    });
    context.adapter.answer(&window, "Working on it");
    context.pass().unwrap();
    assert_eq!(tiers.task(number).task.state, "failed", "T-{number}");
}

/// T-`number` (the chief's), waiting for the tasks `needs`.
fn waits_for(tiers: &Tiers<'_>, needs: &[u64]) -> i64 {
    tiers
        .context
        .create_task(
            tiers.project.id,
            NewTask {
                from: "chief".to_owned(),
                pool: Some("worker".to_owned()),
                tier: Some("standard".to_owned()),
                body: "Tests of the parser".to_owned(),
                needs: needs.to_vec(),
                ..NewTask::default()
            },
        )
        .task
        .number
}

/// What the chief has still to be given, oldest first, as `(id, body)`.
fn queued(tiers: &Tiers<'_>) -> Vec<(i64, String)> {
    let mut found: Vec<(i64, String)> = tiers
        .context
        .inbox(tiers.id("chief"))
        .into_iter()
        .filter(|message| message.kind == "note" && message.state == "queued")
        .map(|message| (message.id, message.body))
        .collect();
    found.reverse();
    found
}

/// The queued notes that are about failures.
fn queued_failures(tiers: &Tiers<'_>) -> Vec<(i64, String)> {
    queued(tiers)
        .into_iter()
        .filter(|(_, body)| body.contains("failed") || body.contains("waits for"))
        .collect()
}

/// Why the chief's note `id` was withdrawn: its `reason`.
fn reason_of(tiers: &Tiers<'_>, id: i64) -> Option<String> {
    tiers.context.message(id).reason
}

#[test]
fn six_windows_that_fail_on_the_same_error_over_25_minutes_are_told_in_one_note() {
    let context = Context::new();
    let tiers = six_at_work(&context);
    // The field's own spread: 25 minutes in all, the longest wait 9 minutes.
    let gaps = [0, 3, 3, 8, 3, 9];
    for (number, gap) in (1..=6).zip(gaps) {
        context.advance(gap * MINUTE);
        fails(&context, &tiers, number, ENOTFOUND);
    }

    let told = queued_failures(&tiers);
    assert_eq!(told.len(), 1, "one note, not six: {told:?}");
    let (id, body) = &told[0];
    assert!(
        body.starts_with("T-1, T-2, T-3, T-4, T-5 and T-6 failed within 26 minutes of each other with the same error, which looks like an outage of the network or the API, not their work:\n"),
        "{body}"
    );
    for number in 1..=6 {
        let window = tiers.assignee(number);
        assert!(
            body.contains(&format!(
                "\nT-{number}: @{window}'s harness reported a failure; it said: \"{ENOTFOUND}\""
            )),
            "T-{number} is told with its words: {body}"
        );
    }
    assert!(
        body.ends_with(
            "\nReopen them all with: cf task reopen T-1 \"…\"; cf task reopen T-2 \"…\"; cf task reopen T-3 \"…\"; cf task reopen T-4 \"…\"; cf task reopen T-5 \"…\"; cf task reopen T-6 \"…\""
        ),
        "{body}"
    );
    // The five notes before it were taken in, not delivered: each says what replaced it.
    let all: Vec<_> = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .filter(|message| message.kind == "note" && message.state == "cancelled")
        .collect();
    assert_eq!(all.len(), 5, "{all:?}");
    for message in all {
        assert!(
            message
                .reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("superseded by m-")),
            "{message:?}"
        );
    }
    assert!(*id > 0);
}

#[test]
fn what_waits_for_those_tasks_is_a_list_in_the_note_and_no_note_of_its_own() {
    let context = Context::new();
    let tiers = six_at_work(&context);
    let seventh = waits_for(&tiers, &[1]);
    let eighth = waits_for(&tiers, &[1, 4]);
    for number in 1..=6 {
        context.advance(2 * MINUTE);
        fails(&context, &tiers, number, ENOTFOUND);
    }

    let told = queued_failures(&tiers);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].1.contains(&format!(
            "\nWaiting for them: T-{seventh} and T-{eighth} wait for T-1; T-{eighth} waits for T-4. Cancel what waits, or reopen what it waits for.\n"
        )),
        "{}",
        told[0].1
    );
}

#[test]
fn the_closes_of_those_windows_are_a_line_each_of_the_note_and_no_note_of_their_own() {
    let context = Context::new();
    let tiers = six_at_work(&context);
    // The host ends what the window ran as it closes it.
    context.host.exits_leaving(&[LOCK]);
    for number in 1..=6 {
        context.advance(2 * MINUTE);
        fails(&context, &tiers, number, ENOTFOUND);
        // The close comes after the failure, a pass later.
        context.pass().unwrap();
    }

    let told = queued_failures(&tiers);
    assert_eq!(
        told.len(),
        1,
        "one note, no close notes beside it: {told:?}"
    );
    for number in 1..=6 {
        assert!(
            told[0].1.contains(&format!(
                "\nT-{number}'s window closed with background work running, which its close ended: {LOCK}"
            )),
            "T-{number}: {}",
            told[0].1
        );
    }
    assert!(
        told[0].1.ends_with("; cf task reopen T-6 \"…\""),
        "the line that reopens them is the last: {}",
        told[0].1
    );
}

#[test]
fn a_single_failure_is_told_as_it_always_was_and_its_close_in_a_note_of_its_own() {
    let context = Context::new();
    let tiers = six_at_work(&context);
    context.host.exits_leaving(&[LOCK]);
    fails(&context, &tiers, 1, ENOTFOUND);
    context.pass().unwrap();

    let window = tiers.assignee(1);
    let told = queued(&tiers);
    assert_eq!(
        told.iter().map(|(_, body)| body.as_str()).collect::<Vec<_>>(),
        [
            format!("T-1 failed: @{window}'s harness reported a failure; it said: \"{ENOTFOUND}\". Reopen it with: cf task reopen T-1 \"…\", or accept it with what you checked: cf task accept T-1 \"…\"").as_str(),
            format!("T-1's window closed with background work running, which its close ended: {LOCK}").as_str(),
        ],
        "the failure and the close, two notes"
    );
}

#[test]
fn failures_with_different_errors_are_two_notes() {
    let context = Context::new();
    let tiers = six_at_work(&context);
    fails(&context, &tiers, 1, ENOTFOUND);
    context.advance(MINUTE);
    fails(&context, &tiers, 2, CAPACITY);

    let told = queued_failures(&tiers);
    assert_eq!(told.len(), 2, "{told:?}");
    assert!(told[0].1.starts_with("T-1 failed: "), "{}", told[0].1);
    assert!(told[1].1.starts_with("T-2 failed: "), "{}", told[1].1);
}

#[test]
fn a_failure_with_the_same_error_after_the_span_is_a_note_of_its_own() {
    let context = Context::new();
    let tiers = six_at_work(&context);
    fails(&context, &tiers, 1, ENOTFOUND);
    context.advance(SAME_ERROR_WITHIN_MS + MINUTE);
    fails(&context, &tiers, 2, ENOTFOUND);

    let told = queued_failures(&tiers);
    assert_eq!(told.len(), 2, "another outage, another note: {told:?}");
}

#[test]
fn a_failure_with_the_same_error_just_within_the_span_is_told_in_the_note_before_it() {
    let context = Context::new();
    let tiers = six_at_work(&context);
    fails(&context, &tiers, 1, ENOTFOUND);
    context.advance(SAME_ERROR_WITHIN_MS - MINUTE);
    fails(&context, &tiers, 2, ENOTFOUND);
    context.advance(SAME_ERROR_WITHIN_MS - MINUTE);
    fails(&context, &tiers, 3, ENOTFOUND);

    let told = queued_failures(&tiers);
    assert_eq!(told.len(), 1, "the span runs from the newest: {told:?}");
    assert!(
        told[0]
            .1
            .starts_with("T-1, T-2 and T-3 failed within 58 minutes of each other"),
        "{}",
        told[0].1
    );
}

#[test]
fn a_failure_after_the_chief_was_given_the_note_is_a_note_of_its_own() {
    let context = Context::new();
    let tiers = six_at_work(&context);
    context.adapter.with("chief", |agent| agent.at_work = false);
    fails(&context, &tiers, 1, ENOTFOUND);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let given = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .find(|message| message.body.starts_with("T-1 failed: "))
        .expect("the chief's note of T-1");
    assert_ne!(given.state, "queued", "the chief has it: {given:?}");
    busy_chief(&context);
    context.advance(MINUTE);
    fails(&context, &tiers, 2, ENOTFOUND);

    let told = queued_failures(&tiers);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].1.starts_with("T-2 failed: "),
        "the first is the chief's already, and stays so: {}",
        told[0].1
    );
    assert_eq!(context.message(given.id).state, given.state);
}

#[test]
fn a_task_reopened_before_its_note_was_given_leaves_the_note_and_the_notes_of_what_waits() {
    let context = Context::new();
    let tiers = six_at_work(&context);
    // T-7 is the chief's own; T-8 is the human's own and has no part in the chief's note.
    let seventh = waits_for(&tiers, &[1]);
    let eighth = tiers
        .context
        .create_task(
            tiers.project.id,
            NewTask {
                from: "human".to_owned(),
                pool: Some("worker".to_owned()),
                tier: Some("standard".to_owned()),
                body: "Tests of the lexer".to_owned(),
                needs: vec![1u64],
                ..NewTask::default()
            },
        )
        .task
        .number;
    fails(&context, &tiers, 1, ENOTFOUND);
    let told = queued_failures(&tiers);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].1.contains(&format!("T-{seventh} waits for it")),
        "{}",
        told[0].1
    );
    let human = tiers.notes("human");
    let waits: Vec<_> = human
        .iter()
        .filter(|note| note.starts_with(&format!("T-{eighth} waits for T-1, which failed")))
        .collect();
    assert_eq!(waits.len(), 1, "{human:?}");

    context
        .ledger
        .borrow_mut()
        .reopen_task(tiers.project.id, 1, "chief", "Carry on: a network blip")
        .unwrap();

    assert!(
        queued_failures(&tiers).is_empty(),
        "nothing of T-1 is told as failed any more: {:?}",
        queued_failures(&tiers)
    );
    assert_eq!(
        reason_of(&tiers, told[0].0).as_deref(),
        Some("T-1 is queued now")
    );
    let human_queued: Vec<_> = context
        .inbox(tiers.id("human"))
        .into_iter()
        .filter(|message| message.kind == "note" && message.state == "queued")
        .collect();
    assert!(
        human_queued
            .iter()
            .all(|message| !message.body.contains("which failed")),
        "the human's note that T-{eighth} waits for a failed task is withdrawn too: {human_queued:?}"
    );
}

#[test]
fn a_task_accepted_leaves_its_note_as_a_reopened_one_does() {
    let context = Context::new();
    let tiers = six_at_work(&context);
    fails(&context, &tiers, 1, ENOTFOUND);
    context.advance(31 * MINUTE);
    fails(&context, &tiers, 2, CAPACITY);
    assert_eq!(queued_failures(&tiers).len(), 2);

    context
        .ledger
        .borrow_mut()
        .accept_failed_task(tiers.project.id, 2, "chief", "Nothing of it was needed")
        .unwrap();

    let told = queued_failures(&tiers);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].1.starts_with("T-1 failed: "), "{}", told[0].1);
}

#[test]
fn a_waiter_called_off_leaves_the_notes_that_it_waits() {
    let context = Context::new();
    let tiers = six_at_work(&context);
    let by_the_chief = waits_for(&tiers, &[1]);
    let by_the_human = tiers
        .context
        .create_task(
            tiers.project.id,
            NewTask {
                from: "human".to_owned(),
                pool: Some("worker".to_owned()),
                tier: Some("standard".to_owned()),
                body: "Tests of the lexer".to_owned(),
                needs: vec![1u64],
                ..NewTask::default()
            },
        )
        .task
        .number;
    fails(&context, &tiers, 1, ENOTFOUND);
    let waits_for_it = format!(
        " T-{by_the_chief} waits for it: cancel T-{by_the_chief}, or reopen T-1 and let it finish."
    );
    let told = queued_failures(&tiers);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].1.ends_with(&waits_for_it), "{}", told[0].1);
    assert_eq!(
        tiers
            .notes("human")
            .iter()
            .filter(|note| note.contains("which failed"))
            .count(),
        1
    );

    context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, by_the_human, "chief")
        .unwrap();
    context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, by_the_chief, "chief")
        .unwrap();

    let after = queued_failures(&tiers);
    assert_eq!(after.len(), 1, "{after:?}");
    assert!(
        after[0].1.starts_with("T-1 failed: ") && !after[0].1.contains("waits for it"),
        "nothing waits for T-1 now: {}",
        after[0].1
    );
    let human: Vec<_> = context
        .inbox(tiers.id("human"))
        .into_iter()
        .filter(|message| message.state == "queued" && message.body.contains("which failed"))
        .collect();
    assert!(human.is_empty(), "{human:?}");
}

#[test]
fn a_note_of_several_is_written_again_for_those_that_failed_still() {
    let context = Context::new();
    let tiers = six_at_work(&context);
    for number in 1..=3 {
        context.advance(MINUTE);
        fails(&context, &tiers, number, ENOTFOUND);
    }
    let before = queued_failures(&tiers);
    assert_eq!(before.len(), 1);

    context
        .ledger
        .borrow_mut()
        .reopen_task(tiers.project.id, 2, "chief", "Carry on")
        .unwrap();

    let after = queued_failures(&tiers);
    assert_eq!(after.len(), 1, "{after:?}");
    assert_ne!(after[0].0, before[0].0, "written again");
    assert_eq!(
        reason_of(&tiers, before[0].0),
        Some(format!("superseded by m-{}", after[0].0))
    );
    assert!(
        after[0]
            .1
            .starts_with("T-1 and T-3 failed within 2 minutes of each other"),
        "{}",
        after[0].1
    );
    assert!(!after[0].1.contains("T-2"), "{}", after[0].1);
    assert!(
        after[0].1.ends_with(
            "\nReopen them all with: cf task reopen T-1 \"…\"; cf task reopen T-3 \"…\""
        ),
        "{}",
        after[0].1
    );

    // The last of them reopened: one failure, in the words of one.
    context
        .ledger
        .borrow_mut()
        .reopen_task(tiers.project.id, 3, "chief", "Carry on")
        .unwrap();
    let last = queued_failures(&tiers);
    assert_eq!(last.len(), 1, "{last:?}");
    assert!(last[0].1.starts_with("T-1 failed: "), "{}", last[0].1);
    context
        .ledger
        .borrow_mut()
        .reopen_task(tiers.project.id, 1, "chief", "Carry on")
        .unwrap();
    assert!(queued_failures(&tiers).is_empty());
}

#[test]
fn a_note_already_given_is_left_when_its_task_is_reopened() {
    let context = Context::new();
    let tiers = six_at_work(&context);
    context.adapter.with("chief", |agent| agent.at_work = false);
    fails(&context, &tiers, 1, ENOTFOUND);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let given = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .find(|message| message.body.starts_with("T-1 failed: "))
        .expect("the chief's note of T-1");
    assert_ne!(given.state, "queued", "{given:?}");

    context
        .ledger
        .borrow_mut()
        .reopen_task(tiers.project.id, 1, "chief", "Carry on")
        .unwrap();

    assert_eq!(
        context.message(given.id).state,
        given.state,
        "the chief has it"
    );
}

#[test]
fn a_failure_a_delivery_gave_up_is_withdrawn_with_its_task_as_well() {
    // The engine's own note of a launch that did not come up is no note of failures, and is
    // as untrue once its task is accepted.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    busy_chief(&context);
    context.host.exits_showing(Some(3), None, None);
    context.exit("zeus");
    assert_eq!(tiers.task(1).task.state, "failed");
    let told = queued_failures(&tiers);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].1.starts_with("T-1 failed: "), "{}", told[0].1);

    context
        .ledger
        .borrow_mut()
        .accept_failed_task(tiers.project.id, 1, "chief", "Nothing of it was needed")
        .unwrap();

    assert!(
        queued_failures(&tiers).is_empty(),
        "{:?}",
        queued_failures(&tiers)
    );
    assert_eq!(
        reason_of(&tiers, told[0].0).as_deref(),
        Some("T-1 is accepted now")
    );
}

/// A key a harness or a shell could show: made up, as long as a real one.
const SECRET: &str = "sk-proj-Zq81XmN4vT7bKd29LpWs3YcRu6EhGf0A";

/// T-`number`'s window ends its turn failed with no words of the harness's own, so that what
/// the failure says is what its window shows last.
fn fails_showing_only_its_screen(context: &Context, tiers: &Tiers<'_>, number: i64) {
    let window = tiers.assignee(number);
    context.adapter.with(&window, |agent| {
        agent.failed = true;
        agent.failure = None;
    });
    context.adapter.answer(&window, "Working on it");
    context.pass().unwrap();
    assert_eq!(tiers.task(number).task.state, "failed", "T-{number}");
}

/// Everything the ledger holds in its log and in its messages, as text.
fn ledger_text(context: &Context) -> String {
    let ledger = context.ledger.borrow();
    let mut text = String::new();
    for sql in ["SELECT data FROM event", "SELECT body FROM message"] {
        let mut statement = ledger.sqlite().prepare(sql).unwrap();
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap();
        for row in rows {
            text.push_str(&row.unwrap());
            text.push('\n');
        }
    }
    text
}

/// The parts of `secret` a ledger must not hold: the whole, and every run of 8 characters
/// of it (a part of a key is as good as a key to whoever reads the log).
fn parts_of(secret: &str) -> Vec<String> {
    let chars: Vec<char> = secret.chars().collect();
    let mut parts = vec![secret.to_owned()];
    parts.extend(chars.windows(8).map(|run| run.iter().collect::<String>()));
    parts
}

#[test]
fn a_failure_whose_screen_names_a_key_leaves_no_part_of_it_in_the_ledger() {
    let context = Context::new();
    let tiers = six_at_work(&context);
    context.host.set_snapshot(json!({
        "tail": [
            "Working on the parser",
            format!("export OPENAI_API_KEY={SECRET}"),
            "Error: the request failed",
        ]
    }));
    fails_showing_only_its_screen(&context, &tiers, 1);

    let held = ledger_text(&context);
    for part in parts_of(SECRET) {
        assert!(!held.contains(&part), "the ledger holds {part:?}");
    }
    // The note says the screen named a secret and left its lines out, as it always did.
    let told = queued_failures(&tiers);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0]
            .1
            .contains("its last screen lines named a secret, so they are left out"),
        "{}",
        told[0].1
    );
}

#[test]
fn a_failure_whose_harness_quotes_a_key_leaves_no_part_of_it_in_the_ledger() {
    // A provider's error that says which key it refused ("Incorrect API key provided: sk-…").
    let context = Context::new();
    let tiers = six_at_work(&context);
    fails(
        &context,
        &tiers,
        1,
        &format!("Incorrect API key provided: {SECRET}. You can find your API key at the provider's page."),
    );

    let held = ledger_text(&context);
    for part in parts_of(SECRET) {
        assert!(!held.contains(&part), "the ledger holds {part:?}");
    }
}

#[test]
fn failures_whose_screens_say_the_same_are_still_one_note_and_other_screens_stand_apart() {
    let context = Context::new();
    let tiers = six_at_work(&context);
    let screen = |last: &str| json!({ "tail": ["Working on the parser", format!("export OPENAI_API_KEY={SECRET}"), last] });
    context
        .host
        .set_snapshot(screen("Error: the request failed"));
    fails_showing_only_its_screen(&context, &tiers, 1);
    context.advance(MINUTE);
    fails_showing_only_its_screen(&context, &tiers, 2);
    context.advance(MINUTE);
    context
        .host
        .set_snapshot(screen("Error: the model is not found"));
    fails_showing_only_its_screen(&context, &tiers, 3);

    let told = queued_failures(&tiers);
    assert_eq!(told.len(), 2, "{told:?}");
    assert!(
        told[0].1.starts_with("T-3 failed: ") || told[1].1.starts_with("T-3 failed: "),
        "{told:?}"
    );
    assert!(
        told.iter()
            .any(|(_, body)| body.starts_with("T-1 and T-2 failed within")),
        "the same screen is one note: {told:?}"
    );
}
