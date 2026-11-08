//! A chief whose window does not come up (`describe('a chief whose window
//! does not come up')`): its first message waits for the next window, the
//! project it leads stays as it is, and it is tried again, ever more slowly.

use std::cell::Cell;
use std::rc::Rc;

use cf_base::startup::{StartFailure, StartKind, StartRefusal};
use cf_engine::testing::Context;
use cf_engine::SwitchWhen;

use crate::chiefs::{handoffs_of, to, to_human, with_codex};
use crate::fixtures::assert_match;
use crate::traces::held_to;

const SUITES: &[&str] = &["a chief whose window does not come up"];

/// A project whose Claude chief has said something, so a switch has a history to hand over.
fn spoken(context: &Context) -> cf_ledger::ProjectView {
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    project
}

#[test]
fn gives_the_new_chief_its_handoff_again_when_its_first_window_closes_before_showing_it() {
    let context = with_codex();
    let project = spoken(&context);
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    context.codex.with("chief", |agent| agent.items.clear());
    let handoff = handoffs_of(&context, project.id).remove(0);
    // The window crashes, or the human closes the project, before the handoff shows.
    context.exit("chief");
    assert_eq!(context.project(project.id).state, "suspended");
    let waiting = context.message(handoff.id);
    assert_eq!((waiting.state.as_str(), waiting.attempts), ("queued", 0));

    context.resume_project(project.id).unwrap();
    let launch = context.codex.prepared().last().cloned().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"^\[ConsensFlow m-\d+ · note from ConsensFlow\]\nYou are the chief now\.",
    );
    let handoffs: Vec<(i64, String)> = handoffs_of(&context, project.id)
        .into_iter()
        .map(|message| (message.id, message.state))
        .collect();
    assert_eq!(
        handoffs,
        [(handoff.id, "delivering".to_owned())],
        "the same handoff, and no other"
    );
    held_to(
        context.close(),
        SUITES,
        "gives the new chief its handoff again when its first window closes before showing it",
    );
}

#[test]
fn keeps_the_project_open_when_the_new_chief_cannot_take_its_handoff_and_opens_it_again_with_it() {
    let context = with_codex();
    let project = spoken(&context);
    let chief = context.id(project.id, "chief");
    let zeus = context.id(project.id, "zeus");
    let slow = Rc::new(Cell::new(true));
    *context.codex.started.borrow_mut() = Some(Rc::new(move || {
        if !slow.replace(false) {
            return Ok(());
        }
        Err("the Codex broker never named the thread it opened".to_owned())
    }));
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    let killed = context.host.killed().len();
    // A window that could not start never took its handoff: its record holds none of it.
    context.codex.forgets_first_messages.set(true);
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    context.codex.forgets_first_messages.set(false);
    assert_eq!(
        context.project(project.id).state,
        "open",
        "the staff keeps working"
    );
    assert_eq!(
        context.host.killed().len(),
        killed + 2,
        "the old chief, then the new window"
    );
    assert_eq!(context.dispatcher.pane(chief), None);
    assert_ne!(context.dispatcher.pane(zeus), None);
    let handoff = handoffs_of(&context, project.id).remove(0);
    assert_eq!((handoff.state.as_str(), handoff.attempts), ("queued", 0));
    assert_eq!(
        to_human(&context, project.id),
        ["The chief could not start: the window could not take its first message (its cause is in ConsensFlow's log). What comes for the chief waits for it, and ConsensFlow tries again; you may also switch the chief."]
    );

    context.pass().unwrap();
    assert_eq!(context.codex.prepared().len(), 1, "not again at once");
    context.advance(5_000);
    context.pass().unwrap();
    assert_eq!(context.codex.prepared().len(), 2);
    let launch = context.codex.prepared().last().cloned().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"You are the chief now\.",
    );
    assert_eq!(handoffs_of(&context, project.id).len(), 1);
    held_to(
        context.close(),
        SUITES,
        "keeps the project open when the new chief cannot take its handoff, and opens it again with it",
    );
}

#[test]
fn a_chief_whose_launch_keeps_failing_its_handoff_waits_no_other_is_written_it_is_tried_ever_more_slowly_and_the_human_hears_once(
) {
    let context = with_codex();
    let project = spoken(&context);
    let tries = Rc::new(Cell::new(0));
    let counted = Rc::clone(&tries);
    *context.codex.prepare.borrow_mut() = Some(Rc::new(move |_| {
        counted.set(counted.get() + 1);
        Err("codex is broken".to_owned())
    }));
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    let ready = context.note(project.id, "zeus", "chief", "Ready");
    for _ in 0..10 {
        context.pass().unwrap();
    }
    assert_eq!(tries.get(), 1, "no pass tries again at once");
    for wait in [5_000, 10_000, 20_000] {
        context.advance(wait - 1);
        context.pass().unwrap();
        context.advance(1);
        context.pass().unwrap();
    }
    assert_eq!(tries.get(), 4, "after 5, 10 and 20 seconds");
    let handoffs: Vec<(String, i64)> = handoffs_of(&context, project.id)
        .into_iter()
        .map(|message| (message.state, message.attempts))
        .collect();
    assert_eq!(
        handoffs,
        [("queued".to_owned(), 0)],
        "one handoff, never spent"
    );
    let waiting = context.message(ready.id);
    assert_eq!((waiting.state.as_str(), waiting.attempts), ("queued", 0));
    assert_eq!(to_human(&context, project.id).len(), 1, "told once");
    assert_match(
        &to_human(&context, project.id)[0],
        "could not start: the launch failed \\(its cause is in ConsensFlow's log\\)\\. What comes",
    );

    *context.codex.prepare.borrow_mut() = None;
    context.advance(40_000);
    context.pass().unwrap();
    let launch = context.codex.prepared().last().cloned().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"You are the chief now\.",
    );
    assert_eq!(to_human(&context, project.id).len(), 1);
    held_to(
        context.close(),
        SUITES,
        "a chief whose launch keeps failing: its handoff waits, no other is written, it is tried ever more slowly, and the human hears once",
    );
}

#[test]
fn notices_a_chief_window_that_exits_before_its_open_is_answered() {
    let context = with_codex();
    let project = spoken(&context);
    let chief = context.id(project.id, "chief");
    // The host sends the exit first, in the same read as its answer to the open: the
    // window went before Codex took its handoff, which its record holds nothing of.
    *context.host.exit_after_open.borrow_mut() = Some("chief".to_owned());
    context.codex.forgets_first_messages.set(true);
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    context.codex.forgets_first_messages.set(false);
    assert_eq!(context.dispatcher.pane(chief), None);
    assert_eq!(
        context.project(project.id).state,
        "suspended",
        "the chief went, as it would have later"
    );
    assert_eq!(
        handoffs_of(&context, project.id)[0].state,
        "queued",
        "its handoff waits"
    );

    context.resume_project(project.id).unwrap();
    assert_ne!(context.dispatcher.pane(chief), None);
    let launch = context.codex.prepared().last().cloned().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"You are the chief now\.",
    );
    held_to(
        context.close(),
        SUITES,
        "notices a chief window that exits before its open is answered",
    );
}

#[test]
fn keeps_what_was_queued_for_a_first_chief_whose_window_does_not_open() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.close_project(project.id).unwrap();
    context.host.refuse.set(true);
    context.resume_project(project.id).unwrap();
    assert_match(
        &to_human(&context, project.id)[0],
        r"^The chief could not start: the window did not open: the host would not open it\.",
    );
    let result = context.note(project.id, "zeus", "chief", "A result");
    // Tried again with the result as its first message, and refused again.
    context.advance(5_000);
    context.host.refuse.set(true);
    context.pass().unwrap();
    let waiting = context.message(result.id);
    assert_eq!(
        (waiting.state.as_str(), waiting.attempts),
        ("queued", 0),
        "not spent on a window that did not open"
    );
    assert_eq!(to_human(&context, project.id).len(), 1);
    context.advance(10_000);
    context.pass().unwrap();
    let launch = context.adapter.prepared().last().cloned().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"note from @zeus\]\nA result$",
    );
    held_to(
        context.close(),
        SUITES,
        "keeps what was queued for a first chief whose window does not open",
    );
}

/// How a launch that failed to start is told, whatever the failure said (T-1328): the stage, and
/// where a producer typed the failure, its kind, its code and its advice. The pause note of a
/// chief and of a member from an adapter that could not prepare the window, a host that refused
/// to open it, and a host that refused it as a program that cannot be run: what each says.
struct Startup {
    chief_prepared: String,
    chief_refused: String,
    member_prepared: String,
    member_refused: String,
}

/// The notes a start that failed with `cause` leaves: the adapter's error, the host's refusal in
/// its own words, or (`unrunnable`) the host's refusal of a program that cannot be run as it is.
fn startup_notes(cause: &str, unrunnable: Option<StartRefusal>) -> Startup {
    // The adapter cannot prepare the chief's window.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.close_project(project.id).unwrap();
    let said = cause.to_owned();
    *context.adapter.prepare.borrow_mut() = Some(Rc::new(move |_| Err(said.clone())));
    context.resume_project(project.id).unwrap();
    let chief_prepared = to_human(&context, project.id).join(" | ");
    context.close();

    // The host refuses to open it.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.close_project(project.id).unwrap();
    *context.host.refused_with.borrow_mut() = Some(cause.to_owned());
    *context.host.unrunnable_with.borrow_mut() = unrunnable.clone();
    context.host.refuse.set(true);
    context.resume_project(project.id).unwrap();
    let chief_refused = to_human(&context, project.id).join(" | ");
    context.close();

    // A member's task fails with the adapter's error.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let said = cause.to_owned();
    *context.adapter.prepare.borrow_mut() = Some(Rc::new(move |_| Err(said.clone())));
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    let member_prepared = failure_note(&context, project.id);
    context.close();

    // A member's task fails with the host's refusal.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    *context.host.refused_with.borrow_mut() = Some(cause.to_owned());
    *context.host.unrunnable_with.borrow_mut() = unrunnable;
    context.host.refuse.set(true);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    let member_refused = failure_note(&context, project.id);
    context.close();
    Startup {
        chief_prepared,
        chief_refused,
        member_prepared,
        member_refused,
    }
}

/// The note the ledger holds on task 1 saying it failed.
fn failure_note(context: &Context, project: i64) -> String {
    context
        .task(project, 1)
        .messages
        .iter()
        .find(|m| m.kind == "note")
        .map(|m| m.body.clone())
        .unwrap_or_default()
}

/// What a note says of a launch that failed whatever its cause said: the adapter's error and the
/// host's refusal are free text, and a path in them can name a credential (the joint review T-1322:
/// five blockers, then two left, by guessing which words were safe). So no word of them is in a
/// note, the stage is, and ConsensFlow's log has the words, masked. `Pi hunter22.cmd` in a name,
/// a Pi shim at `sk-abc defg hijk lmno\pi.cmd`, `spawn pi hunter22.cmd ENOENT` and a cause that is
/// no path at all are told alike.
#[test]
fn a_launch_that_failed_with_words_of_an_adapter_or_a_host_quotes_no_word_of_them() {
    let mut failed = Vec::new();
    for cause in [
        r"C:\tools\pi hunter22.cmd is not an npm shim, and only cmd.exe could run it in a window",
        r"C:\tools\sk-abc defg hijk lmno\pi.cmd runs on Node, and ConsensFlow finds no Node for it",
        "spawn pi hunter22.cmd ENOENT",
        "spawn /opt/bin/Claude hunter22 ENOENT",
        r"%LOCALAPPDATA%\pi-node\current holds no node",
        "could not fetch https://example.test/models.json: refused",
        "codex is broken: key: abc",
        "no conversation found with that id",
    ] {
        let notes = startup_notes(cause, None);
        for (route, note, begins) in [
            (
                "chief, prepared",
                &notes.chief_prepared,
                "The chief could not start: the launch failed (its cause is in ConsensFlow's log). What comes for the chief waits for it, and ConsensFlow tries again; you may also switch the chief.",
            ),
            (
                "chief, refused",
                &notes.chief_refused,
                "The chief could not start: the window did not open: the host would not open it. What comes for the chief waits for it, and ConsensFlow tries again; you may also switch the chief.",
            ),
            (
                "member, prepared",
                &notes.member_prepared,
                "T-1 failed: the launch failed (its cause is in ConsensFlow's log).",
            ),
            (
                "member, refused",
                &notes.member_refused,
                "T-1 failed: the window did not open: the host would not open it.",
            ),
        ] {
            if !note.starts_with(begins) {
                failed.push(format!("{route}: {note}"));
            }
            for word in ["hunter22", "sk-abc", "example.test", "key: abc", "ENOENT", "broken"] {
                if note.contains(word) {
                    failed.push(format!("{route}: `{word}` is in {note}"));
                }
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A program that cannot be run as it is (an npm shim with no Node to run on, a script only
/// cmd.exe could run, a program that was not found) is refused by the host with the kind of
/// failure typed by its producer (`cf_base::startup`): the note says what failed, the error code
/// and the advice, and no path (T-1328). The person's own words, which name the file, are not in it.
#[test]
fn a_window_refused_as_a_program_that_cannot_be_run_says_its_kind_and_advice_and_no_path() {
    let words = r"C:\tools\sk-abc defg hijk lmno\pi hunter22.cmd runs on Node";
    let mut failed = Vec::new();
    for (failure, said) in [
        (
            StartFailure::of(StartKind::NoNode),
            "Node.js was not found for Claude Code; make the harness's Node visible to ConsensFlow, or install the harness's own build instead of the npm one",
        ),
        (
            StartFailure::of(StartKind::NoNodeForPi),
            "Node.js was not found for Claude Code; install Node 22.19 or newer, or run Pi's installer again",
        ),
        (
            StartFailure::of(StartKind::NotAnNpmShim),
            "Claude Code's program is a script that is not an npm shim; install Claude Code's own build, or its npm shim",
        ),
        (
            StartFailure::of(StartKind::NotFound).with_errno("ENOENT"),
            "Claude Code's program was not found (ENOENT); check that Claude Code is installed and that ConsensFlow can find it",
        ),
    ] {
        let refusal = StartRefusal::new(failure, words.to_owned());
        let notes = startup_notes("not used", Some(refusal));
        for (route, note, begins) in [
            (
                "chief",
                &notes.chief_refused,
                format!("The chief could not start: the window did not open: {said}. What comes for the chief waits for it, and ConsensFlow tries again; you may also switch the chief."),
            ),
            (
                "member",
                &notes.member_refused,
                format!("T-1 failed: the window did not open: {said}."),
            ),
        ] {
            if !note.starts_with(&begins) {
                failed.push(format!("{route}: {note}"));
            }
            for word in ["hunter22", "sk-abc", "tools", "lmno"] {
                if note.contains(word) {
                    failed.push(format!("{route}: `{word}` is in {note}"));
                }
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}
