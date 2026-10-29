//! The Enter pressed once more for a paste its window did not send may withhold
//! its key, and never makes words go twice (both reviews of 2026-10-20, round
//! 10). A key withheld leaves the paste in the window's input box, which is
//! cleared before anything else is pasted there; a window that is sent its words
//! over a channel of its harness's own has no input box to press for, so it gets
//! no Enter and its uncertain hand-over is reconciled by its record alone; and a
//! paste the window has (a row it carried read since) is no paste withdrawn.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::{Context, Made};
use cf_harness::contract::Admission;
use cf_harness::records::Role;
use cf_ledger::{MessageView, NewNote, Read};
use serde_json::{json, Value};

use crate::deliveries::enters;
use crate::resume::at_work;

/// Control-C, the key that clears Claude's input box.
pub(crate) const CLEAR: u8 = 3;

/// How many times Control-C was asked of the host, taken or not.
pub(crate) fn clears(context: &Context) -> usize {
    context
        .host
        .inputs()
        .iter()
        .filter(|body| body["bytes"] == json!([CLEAR]))
        .count()
}

/// The texts `seam`'s windows were handed, in order.
pub(crate) fn handed(context: &Context, seam: &str) -> Vec<String> {
    handed_where(context, seam, |_| true)
}

/// The texts handed to the window of `pane` of `seam`, in order.
pub(crate) fn handed_to(context: &Context, seam: &str, pane: &str) -> Vec<String> {
    handed_where(context, seam, |args| args[0]["pane"]["id"] == pane)
}

fn handed_where(context: &Context, seam: &str, to: impl Fn(&Value) -> bool) -> Vec<String> {
    context
        .recorder
        .calls(seam, &["deliver"])
        .into_iter()
        .filter(|(_, args)| to(args))
        .map(|(_, args)| args[0]["text"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// How many times `message` was handed to a window of `seam`.
pub(crate) fn times_handed(context: &Context, seam: &str, message: i64) -> usize {
    handed(context, seam)
        .iter()
        .filter(|text| text.contains(&marker_of(message)))
        .count()
}

/// A note to `to` on T-1, from `from` (ConsensFlow's own for none).
pub(crate) fn note_from(
    context: &Context,
    project: i64,
    from: Option<&str>,
    to: &str,
    body: &str,
) -> MessageView {
    context
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: from.map(str::to_owned),
                to: to.to_owned(),
                task: Some(1),
                body: body.to_owned(),
            },
        )
        .unwrap()
}

/// The chief's note to `to` on T-1.
pub(crate) fn note(context: &Context, project: i64, to: &str, body: &str) -> MessageView {
    note_from(context, project, Some("chief"), to, body)
}

/// The Enter pressed once more for the paste on its way into the window of
/// `pane`, ten seconds on: `during` runs while the window's screen is looked at.
pub(crate) fn enter_again_with(context: &Context, pane: &str, during: impl FnOnce()) {
    context.advance(11_000);
    context.host.set_snapshot(json!({ "outputQuietMs": 5_000 }));
    let pane: Value = json!(pane);
    let looking = context
        .host
        .request_holds
        .hold(move |args| args[0] == "pane.snapshot" && args[1]["id"] == pane);
    let passing = context.begin_pass();
    context.settle();
    during();
    looking.open();
    context.settle();
    passing.take().unwrap().unwrap();
}

/// Both reviews of 2026-10-20, round 10 (P1): T-1's words to go on, carrying the
/// chief's note, sat unsent in zeus's input box, and the chief paused T-1 again
/// while the Enter for them looked at the screen, taking the note back. No key
/// went, the words went back to their queue, and the paste stayed in the box: the
/// next paste went in beside it, and its Enter sent the withdrawn note and the
/// words twice. The box is cleared first, with Control-C: nothing is pasted into
/// the window until the host takes the key.
#[test]
fn a_paste_whose_enter_was_withheld_is_cleared_from_the_box_before_anything_is_pasted_there() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.pause_task(project, 1);
    context.pass().unwrap();
    context.adapter.with("zeus-amber-pine", |agent| {
        agent.settled = true;
        agent.arrive = false;
    });
    let note = note(&context, project, "zeus-amber-pine", "Use tabs");
    context.resume_task(project, 1, "Carry on");
    let words = tiers.task(1).messages.last().cloned().expect("the words");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(words.id).state, "delivering");
    let seam = "adapter:claude-code";
    let before = handed(&context, seam).len();
    enter_again_with(&context, "p1-zeus-amber-pine", || {
        context.pause_task(project, 1);
    });
    assert_eq!(enters(&context), 0, "no key that sends");
    assert_eq!(context.message(note.id).state, "cancelled");
    assert_eq!(context.message(words.id).state, "queued");

    // The host takes no key: the words wait, and nothing is pasted beside the old ones.
    context.host.refuse_keys.set(true);
    context.resume_task(project, 1, "Carry on again");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert!(clears(&context) >= 1, "the box is cleared first");
    assert_eq!(
        handed(&context, seam).len(),
        before,
        "nothing is pasted while the box may hold the paste"
    );

    context.host.refuse_keys.set(false);
    context
        .adapter
        .with("zeus-amber-pine", |agent| agent.arrive = true);
    let asked = clears(&context);
    context.pass().unwrap();
    assert_eq!(clears(&context), asked + 1, "pressed once more, and taken");
    assert_eq!(
        handed(&context, seam).len(),
        before + 1,
        "then the words go, once"
    );
    assert_eq!(
        context.adapter.cleared.borrow().last(),
        Some(&true),
        "told the box was just emptied, so Claude presses no Control-C of its own"
    );
    let again = tiers.task(1).messages.last().cloned().expect("the words");
    context.pass().unwrap();
    assert_eq!(context.message(again.id).state, "delivered");

    // Emptied once, the box is not emptied again, though the record never shows
    // the words left there (they rode in the new ones): zeus asks, the chief
    // answers, and the answer goes in with nothing pressed before it.
    let question = context.ask(project, "zeus-amber-pine", "chief", 1, "Which format?");
    let answer = context.answer(project, question.id, "JSON");
    context
        .adapter
        .with("zeus-amber-pine", |agent| agent.settled = true);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        times_handed(&context, seam, answer.id),
        1,
        "the answer goes"
    );
    assert_eq!(
        clears(&context),
        asked + 1,
        "with nothing pressed before it"
    );
}

/// T-829 (Calliope's P2-1 of T-824): a box the human emptied since its paste's Enter
/// was withheld is pressed no key. Control-C into an empty box at rest arms the quit of
/// Claude Code and Muse Code, and a paste between does not disarm Muse's (T-829's probe,
/// Muse 1.4.14): the box its harness reads empty counts as emptied, and the words go.
#[test]
fn a_box_its_harness_reads_empty_is_pressed_no_key_and_the_words_go_into_it() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.pause_task(project, 1);
    context.pass().unwrap();
    context.adapter.with("zeus-amber-pine", |agent| {
        agent.settled = true;
        agent.arrive = false;
    });
    note(&context, project, "zeus-amber-pine", "Use tabs");
    context.resume_task(project, 1, "Carry on");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    let seam = "adapter:claude-code";
    let before = handed(&context, seam).len();
    enter_again_with(&context, "p1-zeus-amber-pine", || {
        context.pause_task(project, 1);
    });
    assert_eq!(enters(&context), 0, "no key that sends");
    // The human emptied the box meanwhile: its harness reads it so.
    context.host.set_snapshot(json!({
        "outputQuietMs": 5_000,
        "bottom": ["────────────────────", "❯", "────────────────────"],
    }));
    context
        .adapter
        .with("zeus-amber-pine", |agent| agent.arrive = true);
    context.resume_task(project, 1, "Carry on again");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(clears(&context), 0, "no Control-C into an empty box");
    assert_eq!(
        handed(&context, seam).len(),
        before + 1,
        "the words go, once"
    );
    assert_eq!(
        context.adapter.cleared.borrow().last(),
        Some(&true),
        "told the box is empty, so the window presses nothing of its own"
    );
}

/// The same of a continuation withdrawn while the screen was looked at: its task
/// paused, or cancelled. What goes into the window next goes into an empty box.
#[test]
fn a_continuation_withdrawn_while_the_screen_was_looked_at_is_cleared_before_the_next_paste() {
    for cancel in [false, true] {
        let context = Context::new();
        let tiers = at_work(&context);
        let project = tiers.project.id;
        context.adapter.with("zeus-amber-pine", |agent| {
            agent.settled = true;
            agent.arrive = false;
        });
        let words = note_from(
            &context,
            project,
            None,
            "zeus-amber-pine",
            &cf_ledger::continuation(1),
        );
        context.pass().unwrap();
        assert_eq!(context.message(words.id).state, "delivering");
        enter_again_with(&context, "p1-zeus-amber-pine", || {
            if cancel {
                context
                    .ledger
                    .borrow_mut()
                    .cancel_task(project, 1, "human")
                    .unwrap();
            } else {
                context.human_pause(project, 1);
            }
        });
        assert_eq!(enters(&context), 0, "cancel {cancel}: no key that sends");
        assert_eq!(context.message(words.id).state, "cancelled");
        assert_eq!(
            clears(&context),
            0,
            "nothing is pressed until a paste is to go in"
        );

        let seam = "adapter:claude-code";
        let zeus = "p1-zeus-amber-pine";
        let before = handed_to(&context, seam, zeus).len();
        // What goes in next: the words that resume T-1, or a note once it is gone.
        let next = if cancel {
            context.note(project, "chief", "zeus-amber-pine", "Next")
        } else {
            context.resume_task(project, 1, "Carry on");
            tiers.task(1).messages.last().cloned().expect("the words")
        };
        for _ in 0..2 {
            context.pass().unwrap();
        }
        let closed = context.host.killed().iter().any(|killed| killed.id == zeus);
        if closed {
            // The window went with its task: what it held went with it.
            assert_eq!(
                handed_to(&context, seam, zeus).len(),
                before,
                "cancel {cancel}"
            );
            continue;
        }
        assert_eq!(
            (clears(&context), times_handed(&context, seam, next.id)),
            (1, 1),
            "cancel {cancel}: the box emptied, then the note pasted"
        );
    }
}

/// Astra's review of 2026-10-20, round 10 (P1): Codex took T-1's words to go on,
/// carrying the chief's note, and its acknowledgement was lost, so the hand-over
/// was uncertain and the words were waited for. The Enter pressed once more found
/// the note withdrawn by a pause while it looked at the screen, and sent the
/// words back to their queue; Codex's record showed them later, which no one
/// waited for any more, and the Resume sent them again. A window sent its words
/// over its harness's own channel has no input box an Enter sends: it gets none,
/// and its record alone says what arrived.
#[test]
fn an_uncertain_hand_over_to_codex_is_never_given_back_by_an_enter_and_arrives_once() {
    let context = Context::made(Made {
        harnesses: vec!["claude-code", "opencode", "codex"],
        codex: false,
        ..Made::default()
    });
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "agent": "apollo" },
            "staff": [{ "agent": "zeus", "harness": "codex", "role": "worker", "tier": "standard" }],
        }))
        .unwrap()
        .expect("the project")
        .id;
    context.give(project, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    context.pause_task(project, 1);
    context.pass().unwrap();
    context.adapter.with("zeus", |agent| {
        agent.settled = true;
        agent.arrive = false;
    });
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(|_| {
        Box::pin(async {
            Ok(Admission::Uncertain {
                reason: "the broker's answer was lost".to_owned(),
            })
        })
    }));
    let note = note(&context, project, "zeus", "Use tabs");
    context.resume_task(project, 1, "Carry on");
    let words = context
        .task(project, 1)
        .messages
        .last()
        .cloned()
        .expect("the words");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(words.id).state, "delivering");
    let seam = "adapter:codex";
    let pane = context.host.last("zeus").expect("its window").pane.id;
    let looked = Rc::new(Cell::new(0));
    let (counting, at) = (Rc::clone(&looked), pane.clone());
    *context.host.on_request.borrow_mut() = Some(Rc::new(move |op: &str, body: &Value| {
        if op == "pane.snapshot" && body["id"] == at.as_str() {
            counting.set(counting.get() + 1);
        }
    }));
    enter_again_with(&context, &pane, || context.pause_task(project, 1));
    assert_eq!(
        looked.get(),
        0,
        "no look at a screen whose box holds nothing of it"
    );
    assert_eq!(
        enters(&context),
        0,
        "no Enter into a window sent its words over a channel"
    );
    assert_eq!(
        context.message(words.id).state,
        "delivering",
        "the words are waited for still"
    );

    // Codex's record shows them now, the note with them.
    let given = handed(&context, seam)
        .into_iter()
        .find(|text| text.contains(&marker_of(words.id)))
        .expect("the words were handed over");
    let item = context.adapter.item(Role::User, &given);
    context.adapter.with("zeus", |agent| agent.items.push(item));
    context.pass().unwrap();
    assert_eq!(context.message(words.id).state, "delivered");

    context.resume_task(project, 1, "Go on");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        times_handed(&context, seam, words.id),
        1,
        "the words went once"
    );
    let noted = handed(&context, seam)
        .iter()
        .filter(|text| text.contains(&note.body))
        .count();
    assert_eq!(noted, 1, "and the note that rode in them");
}

/// Sol's review of 2026-10-20, round 10 (P2): T-1's words to go on carried the
/// chief's answer to zeus's question. Claude took the paste while its record lagged,
/// and ran `cf task get`, which marked the answer read. The Enter pressed once
/// more asked the ledger again, which counted the answer as withdrawn, and the
/// words went back to their queue and were pasted again. A row read since is a
/// sign the window has the paste: no key goes, nothing is cleared, and the words
/// are taken for arrived.
#[test]
fn a_paste_a_row_of_which_was_read_while_the_screen_was_looked_at_is_taken_for_arrived() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    let question = context.ask(project, "zeus-amber-pine", "chief", 1, "Which format?");
    context.pause_task(project, 1);
    context.pass().unwrap();
    let answer = context.answer(project, question.id, "JSON");
    context.adapter.with("zeus-amber-pine", |agent| {
        agent.settled = true;
        agent.arrive = false;
    });
    context.resume_task(project, 1, "Carry on");
    let words = tiers.task(1).messages.last().cloned().expect("the words");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(words.id).state, "delivering");
    let seam = "adapter:claude-code";
    let pasted = handed(&context, seam).pop().expect("the paste");
    assert!(
        pasted.contains(&marker_of(words.id)) && pasted.contains("JSON"),
        "the answer rides with the words: {pasted}"
    );
    assert_eq!(context.message(answer.id).state, "queued");
    enter_again_with(&context, "p1-zeus-amber-pine", || {
        context
            .ledger
            .borrow_mut()
            .receive_read(zeus, &[answer.id], Read::Task)
            .unwrap();
    });
    assert_eq!(enters(&context), 0, "the window has the paste: no key");
    assert_eq!(clears(&context), 0, "and nothing to clear");
    assert_eq!(context.message(answer.id).state, "read");
    let words = context.message(words.id);
    assert_eq!(
        words.state, "delivered",
        "the words arrived with the answer"
    );

    context
        .adapter
        .with("zeus-amber-pine", |agent| agent.arrive = true);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        times_handed(&context, seam, words.id),
        1,
        "the words went once"
    );
}

/// Astra's review of 2026-10-16, round 4, gave a window the Close could not close
/// no key while closed, and the paste waits in its box for the Resume. A Resume
/// after the paste's whole arrival window: the record never showed it, so it went
/// again, pasted beside the old one, and its Enter sent both. A key withheld for a
/// closed project clears the box too, and the paste goes again once, after it.
#[test]
fn a_paste_whose_enter_waited_for_the_resume_past_its_arrival_window_goes_once() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project.id, "zeus", "chief", "hello");
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    context.host.refuse_kills.set(true);
    assert!(
        context.close_project(project.id).is_err(),
        "its windows stay"
    );
    context.host.refuse_kills.set(false);
    context.advance(11_000);
    context.host.set_snapshot(json!({ "outputQuietMs": 5_000 }));
    context.pass().unwrap();
    assert_eq!(enters(&context), 0, "no key that sends, while closed");
    for _ in 0..4 {
        context.advance(20_000);
        context.pass().unwrap();
    }
    context.resume_project(project.id).unwrap();
    context.adapter.with("chief", |agent| agent.arrive = true);
    for _ in 0..4 {
        context.pass().unwrap();
    }
    let seam = "adapter:claude-code";
    assert_eq!(
        enters(&context),
        0,
        "no Enter for a paste emptied from the box"
    );
    assert_eq!(
        (times_handed(&context, seam, note.id), clears(&context)),
        (2, 1),
        "pasted again once, into a box emptied first"
    );
    assert_eq!(context.message(note.id).state, "delivered");

    // Emptied once, the box is not emptied again: the next paste goes into it as
    // it is (a second Control-C in a row ends Claude Code).
    context.adapter.answer("chief", "Noted");
    let next = context.note(project.id, "zeus", "chief", "again");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        times_handed(&context, seam, next.id),
        1,
        "the next note goes"
    );
    assert_eq!(clears(&context), 1, "with nothing pressed before it");
}

/// T-931 P2-1: a window held for what only the human can do in it (Muse's taken-back brief
/// in a box whose first line is out of view) says so on the board, in the window's words,
/// and is asked again at every look: once the human has done it, the words go in.
#[test]
fn a_window_held_for_the_human_waits_in_its_words_on_the_board_and_goes_on_once_it_lifts() {
    use cf_harness::contract::{Held, Readiness};
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    const WORDS: &str = "the box is out of view: empty it in the window";
    let lifted = Rc::new(Cell::new(false));
    let seen = Rc::clone(&lifted);
    context.adapter.ready.replace(Some(Rc::new(move || {
        Ok::<_, String>(if seen.get() {
            Readiness::Ready
        } else {
            Readiness::Held(Held::ForHuman(WORDS.to_owned()))
        })
    })));
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project.id,
            &NewNote {
                from: None,
                to: "chief".to_owned(),
                task: None,
                body: "Use tabs".to_owned(),
            },
        )
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let activity = context.dispatcher.activity(chief);
    assert_eq!(
        (activity.state.as_str(), activity.reason.as_deref()),
        ("waiting", Some(WORDS))
    );
    assert_eq!(context.message(note.id).state, "queued");
    assert_eq!(context.message(note.id).attempts, 0, "no attempt is spent");
    lifted.set(true);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(note.id).state, "delivered");
    assert_ne!(context.dispatcher.activity(chief).state.as_str(), "waiting");
}
