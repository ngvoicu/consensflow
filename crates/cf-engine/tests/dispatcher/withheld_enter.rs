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

/// How many deliveries went with the key that empties the window's input box first (Control-C),
/// taken or not: a paste of ours left in the box is emptied by its own paste's write, not by a key
/// the engine presses on its own.
pub(crate) fn clears(context: &Context) -> usize {
    context
        .adapter
        .clear_firsts
        .borrow()
        .iter()
        .filter(|cleared| **cleared)
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
    context.host.set_snapshot(json!({ "screenQuietMs": 5_000 }));
    // The paste is in the window's box, where its Enter was not pressed.
    context
        .host
        .shows_box(Some(cf_harness::contract::InputBox::Holds));
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

    // The host takes none of it (the key goes in the paste's own write): the words wait, and
    // nothing is pasted beside the old ones.
    let writing = Rc::new(Cell::new(false));
    let taking = Rc::clone(&writing);
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(move |taken| {
        let written = taking.get();
        Box::pin(async move {
            Ok(if written {
                taken()
            } else {
                Admission::Refused {
                    reason: "the host wrote nothing".to_owned(),
                }
            })
        })
    }));
    context.resume_task(project, 1, "Carry on again");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert!(clears(&context) >= 1, "the paste carries the key that clears the box");
    assert_ne!(
        context.message(words.id).state,
        "delivered",
        "nothing went in while the host wrote nothing"
    );

    writing.set(true);
    context
        .adapter
        .with("zeus-amber-pine", |agent| agent.arrive = true);
    let asked = clears(&context);
    context.pass().unwrap();
    assert_eq!(clears(&context), asked + 1, "carried once more, and taken");
    assert!(
        handed(&context, seam).len() >= before + 2,
        "the words were handed to the host more than once: refused, then written once"
    );
    assert_eq!(
        context.adapter.clear_firsts.borrow().last(),
        Some(&true),
        "the key that empties the box went with the paste, once"
    );
    let again = tiers.task(1).messages.last().cloned().expect("the words");
    context.pass().unwrap();
    assert_eq!(context.message(again.id).state, "delivered");
    // What the paste's key emptied is gone from the box on the screen.
    context
        .host
        .shows_box(Some(cf_harness::contract::InputBox::Empty));

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
        "screenQuietMs": 5_000,
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
        context.adapter.clear_firsts.borrow().last(),
        Some(&false),
        "a box the screen shows empty is given no key with the paste"
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
        harnesses: vec!["claude-code", "pi", "codex"],
        codex: false,
        ..Made::default()
    });
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "model": "claude-opus-5" },
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
            .receive_read(
                zeus,
                &[answer.id],
                Read::Task,
                &cf_proto::cf_word::CfWord::bare(),
            )
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
    context.host.set_snapshot(json!({ "screenQuietMs": 5_000 }));
    context
        .host
        .shows_box(Some(cf_harness::contract::InputBox::Holds));
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
    context
        .host
        .shows_box(Some(cf_harness::contract::InputBox::Empty));

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

/// Calliope's P2-1 of T-945: a window held for what only the human can do in it goes
/// through the one clock of a held head message (chief-meeting's `hold`, T-839): at ten
/// minutes the human is told, and the requester of its task, in the window's own words.
/// On Muse's branch only a hold for words typed and not sent had that clock, and a
/// worker's resume could sit in the queue with its lane the only sign.
#[test]
fn a_window_held_for_the_human_is_told_at_ten_minutes_to_the_human_and_the_requester_in_its_words()
{
    use cf_harness::contract::{Held, Readiness};
    const WORDS: &str = "the box is out of view: empty it in the window";
    let context = Context::new();
    let tiers = crate::fixtures::Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.ready.replace(Some(Rc::new(|| {
        Ok::<_, String>(Readiness::Held(Held::ForHuman(WORDS.to_owned())))
    })));
    let worker = tiers.assignee(1);
    let told = context.tell(tiers.project.id, &worker, 1, "One more case.");
    context.pass().unwrap();
    context.adapter.with("zeus", |agent| agent.settled = true);
    context.pass().unwrap();
    context.pass().unwrap();
    let said = |handle: &str| -> Vec<String> {
        tiers
            .notes(handle)
            .into_iter()
            .filter(|note| note.contains("has waited") && note.contains(WORDS))
            .collect()
    };
    context.advance(9 * 60_000);
    context.pass().unwrap();
    assert_eq!(
        said("human"),
        Vec::<String>::new(),
        "not before ten minutes"
    );
    context.advance(2 * 60_000);
    context.pass().unwrap();
    let human = said("human");
    assert_eq!(human.len(), 1, "{:?}", tiers.notes("human"));
    assert!(
        human[0].starts_with(&format!("m-{},", told.id)),
        "{}",
        human[0]
    );
    let chief = said("chief");
    assert_eq!(
        chief.len(),
        1,
        "the requester of T-1: {:?}",
        tiers.notes("chief")
    );
    assert_eq!(context.message(told.id).attempts, 0, "no attempt is spent");
    context.advance(30 * 60_000);
    context.pass().unwrap();
    assert_eq!(said("human").len(), 1, "told once");
}

/// Calliope's P3-1 of T-945: the words a window was held in for the human are its head
/// message's. Once that message leaves the head of the queue (its task cancelled), the
/// lane says them no more, though nothing else is pasted to ask the window again.
#[test]
fn a_window_held_for_the_human_whose_message_leaves_the_head_says_its_words_no_more() {
    use cf_harness::contract::{Held, Readiness};
    const WORDS: &str = "the box is out of view: empty it in the window";
    let context = Context::new();
    let tiers = crate::fixtures::Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.ready.replace(Some(Rc::new(|| {
        Ok::<_, String>(Readiness::Held(Held::ForHuman(WORDS.to_owned())))
    })));
    let project = tiers.project.id;
    let worker = tiers.assignee(1);
    let asked = context.ask(project, &worker, "chief", 1, "Which format?");
    context.pass().unwrap();
    context.pass().unwrap();
    let chief = tiers.id("chief");
    assert_eq!(
        context.dispatcher.held_back(chief).map(|held| held.message),
        Some(asked.id)
    );
    let activity = context.dispatcher.activity(chief);
    assert_eq!(
        (activity.state.as_str(), activity.reason.as_deref()),
        ("waiting", Some(WORDS))
    );
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "chief")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(asked.id).state, "cancelled");
    assert_eq!(context.dispatcher.held_back(chief), None);
    let activity = context.dispatcher.activity(chief);
    assert_ne!(activity.reason.as_deref(), Some(WORDS), "{activity:?}");
    assert_ne!(activity.state.as_str(), "waiting", "{activity:?}");
}

/// A paste whose Enter was withheld sits in zeus's box, and T-1 is resumed with new words: the
/// next delivery empties the box first.
fn a_box_to_clear(context: &Context) -> (i64, i64) {
    let tiers = at_work(context);
    let project = tiers.project.id;
    context.pause_task(project, 1);
    context.pass().unwrap();
    context.adapter.with("zeus-amber-pine", |agent| {
        agent.settled = true;
        agent.arrive = false;
    });
    note(context, project, "zeus-amber-pine", "Use tabs");
    context.resume_task(project, 1, "Carry on");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    enter_again_with(context, "p1-zeus-amber-pine", || {
        context.pause_task(project, 1);
    });
    context.resume_task(project, 1, "Carry on again");
    let words = tiers.task(1).messages.last().cloned().expect("the words");
    (project, words.id)
}

const QUIET: &str = r#"{ "screenQuietMs": 5000, "outputRead": 5, "outputDrawn": 5 }"#;
const BEHIND: &str =
    r#"{ "screenQuietMs": 5000, "outputQuietMs": 0, "outputRead": 5, "outputDrawn": 4 }"#;

/// What the host says of a window whose input box holds the paste whose Enter was withheld.
fn snapshot_of(text: &str) -> Value {
    let mut snapshot: Value = serde_json::from_str(text).unwrap();
    let rule = "─".repeat(40);
    snapshot["bottom"] = json!([rule, "❯ a message the human typed", rule]);
    snapshot["cursor"] = json!({ "line": 1, "col": 2, "shown": true, "plainAfter": 26 });
    snapshot
}

/// N17 (T-2150 note 2, `Dispatcher::may_press`): the key that clears a box goes into a window
/// whose screen is current and still: with output read and not yet drawn, nothing is pressed.
#[test]
fn the_key_that_clears_a_box_waits_while_the_screen_lacks_bytes_the_window_printed() {
    let context = Context::new();
    let (_, words) = a_box_to_clear(&context);
    context.host.set_snapshot(snapshot_of(BEHIND));
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(clears(&context), 0, "no key while the screen is behind");
    let begun = |context: &Context| {
        context
            .recorder
            .events()
            .into_iter()
            .filter(|event| {
                event["seam"] == "event"
                    && event["event"]["kind"] == "delivery.begun"
                    && event["event"]["data"]["message"] == json!(words)
            })
            .count()
    };
    assert_eq!(begun(&context), 0, "not begun: the look before it held it");
    context.host.set_snapshot(snapshot_of(QUIET));
    context.pass().unwrap();
    assert!(
        clears(&context) >= 1,
        "pressed once the screen shows it all"
    );
    assert!(context.message(words).attempts <= 1);
}

/// N17 (T-2160 finding 2): a host that gave no look of the window presses nothing: no key that
/// clears the box, no paste, no attempt spent. The words wait for a look, which reads the box.
#[test]
fn a_look_the_host_did_not_give_clears_no_box_and_pastes_nothing() {
    let context = Context::new();
    let (_, words) = a_box_to_clear(&context);
    context.host.set_snapshot(snapshot_of(QUIET));
    context.host.looks_fail.set(true);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(clears(&context), 0, "no key into a box nobody looked at");
    assert_eq!(context.message(words).attempts, 0, "no attempt is spent");
    context.host.looks_fail.set(false);
    context.pass().unwrap();
    assert!(clears(&context) >= 1, "the key goes once there is a look");
}
