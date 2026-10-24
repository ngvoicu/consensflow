//! A window that says it never will be ready for a paste (`Readiness::Unfit`: a
//! Devin left in its normal permission mode, field report B1 of 2026-10-15) is a
//! launch that did not come up, said on the board: its window closes, a chief's
//! without its project and to be tried again, the human told once and what waits
//! for the chief kept; a member's exit settled at its close, before what it was to be
//! given: the close pauses its task, and the message fails, whoever is told why, but
//! for words told to the window about its task, which are kept for the resume, the
//! chief's note of the pause and the human's saying why and which words wait. The
//! member's case holds the host's exit, as the bridge host answers a kill before the
//! exit comes (the merge's review, T-16 F1: the test host's exit, raised inside its
//! kill, made an order the product never takes). Never a window left to wait. Not
//! held to a Node recording: Node had no such answer.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::testing::{Context, Gate};
use cf_harness::contract::{Held, Readiness};
use cf_ledger::NewNote;
use serde_json::json;

use crate::fixtures::{closed, Tiers};

/// What the window says of itself, as the Devin adapter says it.
const WHY: &str = "its Devin window stayed in the normal permission mode, which /bypass did not change within 30 s (every tool call of its would wait for a permission nobody gives)";

/// Every window of the case says it is unfit for work whenever it is asked.
fn unfit(context: &Context) {
    *context.adapter.ready.borrow_mut() =
        Some(std::rc::Rc::new(|| Ok(Readiness::Unfit(WHY.to_owned()))));
}

#[test]
fn a_chief_whose_window_is_unfit_for_work_closes_without_its_project_and_the_human_is_told_once() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    unfit(&context);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let chief = context.host.last("chief").unwrap().pane;
    context.adapter.answer("zeus", "Parser done.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    // The result was to go into the chief's window, which said it never will take it.
    assert!(closed(&context, &chief), "the chief's window is closed");
    let told = tiers.notes("human");
    let could_not = told
        .iter()
        .filter(|body| body.starts_with("The chief could not start: "))
        .collect::<Vec<_>>();
    assert_eq!(could_not.len(), 1, "{told:?}");
    assert_eq!(
        *could_not[0],
        format!("The chief could not start: {WHY}. What comes for the chief waits for it, and ConsensFlow tries again; you may also switch the chief.")
    );
    // The result waits for the chief's next window, and the project stays open.
    let result = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .find(|message| message.kind == "result")
        .unwrap();
    assert_eq!(result.state, "queued");
    assert_eq!(context.project(tiers.project.id).state, "open");
}

/// A case whose windows never draw their screens (the host says they have printed
/// nothing) and whose chief's record shows nothing, with words for the chief: what
/// `ready` was asked, and its pane. Its windows are asked before they draw where
/// `while_drawing` says so, and say they are unfit at the third ask.
fn never_drawn(while_drawing: bool) -> (Context, Rc<Cell<u32>>) {
    let context = Context::new();
    context.host.set_snapshot(json!({ "outputQuietMs": null }));
    context.adapter.ready_while_drawing.set(while_drawing);
    let asked = Rc::new(Cell::new(0));
    let counted = Rc::clone(&asked);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        counted.set(counted.get() + 1);
        Ok(if counted.get() < 3 {
            Readiness::Held(Held::Because("switching".to_owned()))
        } else {
            Readiness::Unfit(WHY.to_owned())
        })
    }));
    (context, asked)
}

#[test]
fn a_window_that_never_draws_and_says_it_never_will_be_fit_is_given_up_where_its_harness_bounds_its_readiness(
) {
    // Asked only once it had drawn, a Devin window left in its normal mode that never drew
    // was never asked, its bound never ran, and it held what came for it for ever (Astra on
    // the B1 fix). Its harness bounds its readiness whether or not it draws: it is asked
    // as it draws, nothing goes in, and it is given up when it says it never will be fit.
    let (context, asked) = never_drawn(true);
    let tiers = Tiers::new(&context, &["zeus"]);
    context.adapter.with("chief", |agent| agent.settled = true);
    let chief = context.host.last("chief").unwrap().pane;
    let words = context.note(tiers.project.id, "human", "chief", "Any news?");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(asked.get(), 2, "asked at each look, as it draws");
    assert_eq!(
        context.message(words.id).state,
        "queued",
        "and given nothing"
    );
    assert!(!closed(&context, &chief));
    context.pass().unwrap();
    assert!(closed(&context, &chief), "unfit: given up");
    assert_eq!(told_could_not_start(&tiers), 1);
    assert_eq!(
        context.message(words.id).state,
        "queued",
        "it waits for the chief"
    );

    // A harness that does not bound it so is not asked before its window draws.
    let (context, asked) = never_drawn(false);
    let tiers = Tiers::new(&context, &["zeus"]);
    context.adapter.with("chief", |agent| agent.settled = true);
    let chief = context.host.last("chief").unwrap().pane;
    context.note(tiers.project.id, "human", "chief", "Any news?");
    for _ in 0..5 {
        context.pass().unwrap();
    }
    assert_eq!(asked.get(), 0);
    assert!(!closed(&context, &chief));
}

/// How many windows the chief has been launched in.
fn chief_launches(context: &Context) -> usize {
    context
        .adapter
        .prepared()
        .iter()
        .filter(|launch| launch["role"] == "chief")
        .count()
}

/// How often the human was told the chief could not start.
fn told_could_not_start(tiers: &Tiers) -> usize {
    tiers
        .notes("human")
        .iter()
        .filter(|body| body.starts_with("The chief could not start: "))
        .count()
}

/// The chief's window opened again: it takes its first message and answers it, and the
/// next of what waits for it is to go in.
fn through_its_first_message(context: &Context) {
    context.pass().unwrap();
    context.adapter.answer("chief", "Noted.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
}

#[test]
fn a_chief_whose_windows_stay_unfit_is_tried_again_ever_more_slowly_and_the_human_is_told_once_until_one_is_ready(
) {
    // Each window it opened again cleared its count of failures as it started: every
    // failure was the first, tried again after 5 s and told the human again (Sol on the
    // B1 fix). Its count goes until a window is ready for work.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(chief_launches(&context), 1);
    unfit(&context);
    // Each window opened again takes one as its first message, and is to be given the
    // next when it is asked whether it is ready.
    for words in ["One?", "Two?", "Three?", "Four?", "Five?"] {
        context.note(project, "human", "chief", words);
    }
    context.pass().unwrap();
    assert!(closed(&context, &context.host.last("chief").unwrap().pane));
    assert_eq!(told_could_not_start(&tiers), 1);

    // Tried again after 5 s, then 10, then 20: each window says the same, and the human
    // is not told again.
    for wait in [5_000, 10_000, 20_000] {
        let launches = chief_launches(&context);
        context.advance(wait - 1);
        context.pass().unwrap();
        assert_eq!(chief_launches(&context), launches, "not before {wait} ms");
        context.advance(1);
        context.pass().unwrap();
        assert_eq!(
            chief_launches(&context),
            launches + 1,
            "tried again at {wait} ms"
        );
        through_its_first_message(&context);
        assert!(
            closed(&context, &context.host.last("chief").unwrap().pane),
            "the window after {wait} ms was unfit too"
        );
        assert_eq!(told_could_not_start(&tiers), 1, "after {wait} ms");
    }

    // A window that is ready for work came up: its count is over, and a window unfit
    // after it is a failure the human hears of again.
    *context.adapter.ready.borrow_mut() = None;
    context.advance(40_000);
    context.pass().unwrap();
    through_its_first_message(&context);
    let ready = context.host.last("chief").unwrap().pane;
    assert!(!closed(&context, &ready), "this one is ready, and stays");
    let five = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .find(|message| message.body == "Five?")
        .unwrap();
    assert_ne!(five.state, "queued", "given to it as it was ready");
    context.adapter.answer("chief", "Noted.");
    context.pass().unwrap();
    context.pass().unwrap();
    unfit(&context);
    context.note(project, "human", "chief", "Seven?");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(closed(&context, &ready));
    assert_eq!(told_could_not_start(&tiers), 2);
    let launches = chief_launches(&context);
    context.advance(4_999);
    context.pass().unwrap();
    assert_eq!(
        chief_launches(&context),
        launches,
        "a first failure again: 5 s"
    );
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(chief_launches(&context), launches + 1);
}

#[test]
fn a_members_window_unfit_for_work_closes_and_the_words_told_to_it_are_kept_for_its_resume() {
    let context = Context::new();
    // The exit comes after the kill is answered, as the bridge host sends it.
    context.host.hold_exits.set(true);
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let session = tiers.assignee(1);
    let window = context.host.last(&session).unwrap().pane;
    // At rest with its task in hand, and a note on its task to be pasted to it.
    context.adapter.with(&session, |agent| agent.settled = true);
    unfit(&context);
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: Some("chief".to_owned()),
                to: session.clone(),
                task: Some(1),
                body: "Use the grammar in the docs".to_owned(),
            },
        )
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(closed(&context, &window), "the member's window is closed");
    // The close paused its task, and the note, words told to its window, waits for
    // the resume with why its paste failed; no failure is told.
    let kept = context.ledger.borrow().message(note.id).unwrap().unwrap();
    assert_eq!(
        (kept.state.as_str(), kept.reason.as_deref()),
        (
            "queued",
            Some(format!("kept for T-1's resume: {WHY}").as_str())
        )
    );
    assert_eq!(tiers.task(1).task.state, "paused");
    // Whoever is told hears why, once each: the chief in the note that says T-1 is
    // paused, which names the words kept for the resume; the human as main told it,
    // but with no "did not reach" for words the window has at the resume.
    let kept_words = format!("m-{}", note.id);
    let chief: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|said| said.contains(&kept_words) || said.contains(WHY))
        .collect();
    assert_eq!(
        chief,
        [format!(
            "T-1 is paused: @{session}'s window closed: {WHY}; the words told its window wait for the resume: {kept_words}. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
        )]
    );
    let human: Vec<String> = tiers
        .notes("human")
        .into_iter()
        .filter(|said| said.contains(&kept_words))
        .collect();
    assert_eq!(
        human,
        [format!(
            "{kept_words}, a note from @chief on T-1, waits for T-1's resume to reach @{session}: {WHY}."
        )]
    );
    assert!(
        !tiers
            .notes("human")
            .iter()
            .any(|note| note.contains("did not reach")),
        "{:?}",
        tiers.notes("human")
    );
    // The host's exit, when it comes, settles nothing more.
    let host = Rc::clone(&context.host);
    let gone = window.clone();
    context.run(async move { host.exited(gone).await });
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(
        tiers
            .notes("chief")
            .iter()
            .filter(|said| said.starts_with("T-1 is paused: "))
            .count(),
        1
    );
    assert_eq!(
        context
            .ledger
            .borrow()
            .message(note.id)
            .unwrap()
            .unwrap()
            .state,
        "queued"
    );
    // The resume's window, fit for work, has the note once, before the resume.
    *context.adapter.ready.borrow_mut() = None;
    context.resume_task(project, 1, "Carry on");
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(
        context
            .ledger
            .borrow()
            .message(note.id)
            .unwrap()
            .unwrap()
            .state,
        "delivered"
    );
    assert_eq!(tiers.task(1).task.state, "working");
    let given = crate::fixtures::given(&context, &session);
    assert_eq!(
        given
            .iter()
            .filter(|words| words.contains("Use the grammar in the docs"))
            .count(),
        1,
        "{given:?}"
    );
    crate::fixtures::assert_match(
        given.last().unwrap(),
        &format!(
            r"^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\n\(kept for you: note m-{} from @chief\)\nUse the grammar in the docs\n\nResumed: Carry on$",
            note.id
        ),
    );
}

/// A member's unfit window whose kill the host refuses: its exit never comes, so its
/// task is never paused, and the message it was to be given goes all the same, as
/// before the exit was settled first (Calliope on 876286f4, T-47 P3-1: it was left
/// working, its window asked and killed again at every pass, nobody told). Told words
/// fail their task, and the chief and the human hear why, once each.
#[test]
fn a_members_unfit_window_whose_kill_is_refused_goes_with_its_message_and_whoever_is_told_hears_why(
) {
    let context = Context::new();
    context.host.refuse_kills.set(true);
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let session = tiers.assignee(1);
    context.adapter.with(&session, |agent| agent.settled = true);
    unfit(&context);
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: Some("chief".to_owned()),
                to: session.clone(),
                task: Some(1),
                body: "Use the grammar in the docs".to_owned(),
            },
        )
        .unwrap();
    for _ in 0..4 {
        context.pass().unwrap();
    }
    let failed = context.ledger.borrow().message(note.id).unwrap().unwrap();
    assert_eq!(failed.state, "failed");
    assert_eq!(tiers.task(1).task.state, "failed", "not left at work");
    let named = |handle: &str| -> Vec<String> {
        tiers
            .notes(handle)
            .into_iter()
            .filter(|said| said.contains(&format!("m-{},", note.id)))
            .collect()
    };
    assert_eq!(
        named("chief"),
        [format!(
            "T-1 failed: what @chief told its window, m-{}, did not reach it ({WHY}). Reopen it with: cf task reopen T-1 \"…\"",
            note.id
        )]
    );
    assert_eq!(
        named("human"),
        [format!(
            "m-{}, a note from @chief on T-1, did not reach @{session}: {WHY}.",
            note.id
        )]
    );
}

/// The note of the chief to an unfit member's window, and another behind it: the first is
/// the message the window was to be given, kept as its paste fails; the second waits still,
/// and waits for the resume too. Both are named in the chief's note of the pause, the why
/// said once, and the human hears of each once (the chief's decision, T-77: the second went
/// unnamed, and the chief could think it went in).
#[test]
fn a_members_window_unfit_for_work_with_a_second_note_waiting_names_both_words_kept_for_its_resume()
{
    let context = Context::new();
    context.host.hold_exits.set(true);
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context.adapter.with(&session, |agent| agent.settled = true);
    unfit(&context);
    let tell = |body: &str| {
        context
            .ledger
            .borrow_mut()
            .note(
                project,
                &NewNote {
                    from: Some("chief".to_owned()),
                    to: session.clone(),
                    task: Some(1),
                    body: body.to_owned(),
                },
            )
            .unwrap()
            .id
    };
    let (first, second) = (tell("Use the grammar in the docs"), tell("Mind the tests"));
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "paused");
    for id in [first, second] {
        let kept = context.ledger.borrow().message(id).unwrap().unwrap();
        assert_eq!(
            (kept.state.as_str(), kept.reason.as_deref()),
            (
                "queued",
                Some(format!("kept for T-1's resume: {WHY}").as_str())
            ),
            "m-{id}"
        );
    }
    assert_eq!(
        tiers
            .notes("chief")
            .into_iter()
            .filter(|said| said.starts_with("T-1 is paused: "))
            .collect::<Vec<_>>(),
        [format!(
            "T-1 is paused: @{session}'s window closed: {WHY}; the words told its window wait for the resume: m-{first}, m-{second}. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
        )]
    );
    // Once each: the word still waiting as the window went, then the one its give-up kept.
    let mut human: Vec<String> = tiers
        .notes("human")
        .into_iter()
        .filter(|said| said.contains("waits for T-1's resume"))
        .collect();
    human.sort_by_key(|said| {
        said.split(',')
            .next()
            .and_then(|named| named.strip_prefix("m-")?.parse::<i64>().ok())
    });
    assert_eq!(
        human,
        [first, second].map(|id| format!(
            "m-{id}, a note from @chief on T-1, waits for T-1's resume to reach @{session}: {WHY}."
        ))
    );
}

/// An unfit member's window whose kill the host never answers: the pass is not held for
/// it. Past the kill's wait the message goes without the exit, failing, T-1 with it,
/// whoever is told hearing why, as for a kill refused; the window's exit, once the kill is
/// answered, settles nothing more (T-77: the give-up waited on the kill for ever).
#[test]
fn a_members_unfit_window_whose_kill_is_never_answered_has_its_message_fail_after_the_wait() {
    let context = Context::new();
    context.host.hold_exits.set(true);
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context.adapter.with(&session, |agent| agent.settled = true);
    let held = Gate::default();
    let (host, handle, gate) = (Rc::clone(&context.host), session.clone(), held.clone());
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        let window = host.last(&handle).expect("its window").pane;
        *host.hold_kill.borrow_mut() = Some((window.generation, gate.clone()));
        Ok(Readiness::Unfit(WHY.to_owned()))
    }));
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: Some("chief".to_owned()),
                to: session.clone(),
                task: Some(1),
                body: "Use the grammar in the docs".to_owned(),
            },
        )
        .unwrap();
    let passing = context.begin_pass();
    context.settle();
    assert_eq!(
        tiers.task(1).task.state,
        "working",
        "the kill is not answered"
    );

    assert!(context.timers.fire_next(), "the kill's wait ends");
    context.settle();
    passing.take().unwrap().unwrap();
    let failed = context.ledger.borrow().message(note.id).unwrap().unwrap();
    assert_eq!(
        (failed.state.as_str(), failed.reason.as_deref()),
        ("failed", Some(WHY))
    );
    assert_eq!(tiers.task(1).task.state, "failed");
    let words = format!("m-{}", note.id);
    assert_eq!(
        tiers
            .notes("chief")
            .into_iter()
            .filter(|said| said.contains(&words))
            .collect::<Vec<_>>(),
        [format!(
            "T-1 failed: what @chief told its window, {words}, did not reach it ({WHY}). Reopen it with: cf task reopen T-1 \"…\""
        )]
    );
    assert_eq!(
        tiers
            .notes("human")
            .into_iter()
            .filter(|said| said.contains(&words))
            .collect::<Vec<_>>(),
        [format!(
            "{words}, a note from @chief on T-1, did not reach @{session}: {WHY}."
        )]
    );

    // The kill is answered, and the window's exit comes: nothing more is settled.
    let window = context.host.last(&session).unwrap().pane;
    held.open();
    context.settle();
    let host = Rc::clone(&context.host);
    context.run(async move { host.exited(window).await });
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "failed");
}
