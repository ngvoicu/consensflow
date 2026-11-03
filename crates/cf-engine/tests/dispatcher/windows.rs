//! A worker's window through its life: closed mid-task and resumed on its
//! conversation, paused in place, never opened, or never showing its first
//! message (`describe('the dispatcher')`).

use std::cell::RefCell;
use std::rc::Rc;

use cf_engine::testing::Context;
use serde_json::{json, Value};

use crate::fixtures::{assert_match, native_of, Tiers};
use crate::traces::held_to;

const SUITES: &[&str] = &["the dispatcher"];

/// The native session a participant's conversation runs in.
fn native(context: &Context, project: i64, handle: &str) -> String {
    let id = context.id(project, handle);
    let conversation = context.ledger.borrow().current_conversation(id);
    conversation.unwrap().unwrap().native_session.unwrap()
}

/// What the last launch prepared was for and resumed.
fn resumed(context: &Context) -> (Value, Value) {
    let launch = context.adapter.prepared().pop().unwrap();
    (
        launch["participant"]["handle"].clone(),
        launch["resume"].clone(),
    )
}

#[test]
fn pauses_the_task_and_tells_the_requester_when_the_worker_window_closes_mid_task_and_resumes_it_on_its_conversation(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let session = native(&context, project.id, "zeus");
    context.exit("zeus");
    let task = context.task(project.id, 1);
    assert_eq!(
        (task.task.state.as_str(), task.task.assignee.as_deref()),
        ("paused", Some("zeus"))
    );
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_eq!(note.recipient, "chief");
    assert_match(
        &note.body,
        r#"^T-1 is paused: @zeus's window closed\. Resume it with: cf task resume T-1 "…""#,
    );
    context.pass().unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "paused",
        "nothing happens on its own"
    );
    context.resume_task(project.id, 1, "Carry on");
    context.pass().unwrap();
    assert_eq!(resumed(&context), (json!("zeus"), json!(session)));
    let launch = context.adapter.prepared().pop().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"T-1 · task from @chief\]\nResumed: Carry on$",
    );
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    held_to(
        context.close(),
        SUITES,
        "pauses the task and tells the requester when the worker window closes mid-task, and resumes it on its conversation",
    );
}

/// A worker's window that ends by itself in the middle of its task (its harness
/// crashed, or exited) is paused with how it ended and what its screen showed last,
/// as far as the host said: the field report of 2026-10-15 (B2, a Devin worker on
/// Windows) had only "window closed", and nothing to tell its cause by.
#[test]
fn a_worker_window_that_ends_mid_task_says_how_it_ended_and_what_it_showed_in_the_pause_note() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.host.exits_showing(
        Some(0xC000_0005),
        None,
        Some(&[
            "Reading src/parser.rs",
            "thread 'main' panicked at src/tui.rs:88",
        ]),
    );
    context.exit("zeus");

    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "paused");
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_eq!(
        note.body,
        "T-1 is paused: @zeus's window closed (exit code 0xC0000005); its screen ended with: \
         \"Reading src/parser.rs / thread 'main' panicked at src/tui.rs:88\". \
         Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
    );
}

/// Kluris T-153 (T-823): a Claude Code worker's window closed with exit code 143 after the
/// project's own tests killed its `cargo test`, and the pause note said only "exit code 143".
/// No stop and no close of ConsensFlow's was under way, so the note says the signal and that it
/// did not come from ConsensFlow.
#[cfg(unix)]
#[test]
fn a_worker_window_that_exited_with_a_signals_status_says_so_and_that_consensflow_sent_it_no_kill()
{
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .host
        .exits_showing(Some(143), None, Some(&["Running the workspace tests"]));
    context.exit("zeus");

    let task = context.task(project.id, 1);
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_eq!(
        note.body,
        "T-1 is paused: @zeus's window closed (exit code 143, consistent with SIGTERM; \
         ConsensFlow had sent it no kill); \
         its screen ended with: \"Running the workspace tests\". \
         Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
    );
}

/// The same window's note said "its last screen lines named a secret, so they are left out"
/// for one line, and the chief never learned the cause (T-823, kluris T-153): it says what the
/// window was running, the last tool call its record shows, and one line of the screen that
/// names a secret takes only itself and its neighbours out, not the screen.
#[test]
fn a_worker_window_that_closed_says_what_it_was_running_and_leaves_out_only_the_lines_that_name_a_secret(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.with("zeus", |agent| {
        agent.last_call = Some(cf_harness::records::LastCall::shell(
            "Bash",
            "cargo fmt --check && cargo test --workspace -- --nocapture",
        ));
    });
    context.host.exits_showing(
        Some(3),
        None,
        Some(&[
            "Running the workspace tests",
            "test result: FAILED",
            "export TEST_TOKEN=abc",
            "cargo test exited",
            "bye",
        ]),
    );
    context.exit("zeus");

    let task = context.task(project.id, 1);
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_eq!(
        note.body,
        "T-1 is paused: @zeus's window closed (exit code 3); \
         its last tool call was: \"cargo fmt --check && cargo test --workspace -- --nocapture\"; \
         its screen ended with: \"Running the workspace tests / bye\" \
         (3 lines that named or lay beside a secret were left out). \
         Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
    );
}

/// The review of `a26e0848e` put a password in a real ledger note three ways, all on rows the
/// per-row rule quoted: a label with its value below, a YAML block scalar with a modifier, and
/// a value hard-wrapped over rows. The note the ledger holds says none of it, and says the
/// screen was left out; and the same for what the last tool call carries (`curl -u`), and for a
/// base64 value that the masker once let through because it had no digit.
#[test]
fn a_pause_note_the_ledger_holds_says_none_of_a_password_the_screen_or_the_last_call_carried() {
    let withheld =
        "T-1 is paused: @zeus's window closed (exit code 1); its last screen lines named a \
         secret, so they are left out. \
         Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation.";
    for (screen, value) in [
        (
            vec!["Password:", "Enter the value below", "red green blue"],
            "red green blue",
        ),
        (
            vec!["PASSWORD: >-", "  correct horse", "  battery staple"],
            "battery staple",
        ),
        (
            vec!["password: correct horse", "battery staple", "alpha bravo"],
            "battery staple",
        ),
    ] {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        context.give(project.id, "zeus", "Parser");
        context.pass().unwrap();
        context.pass().unwrap();
        context.host.exits_showing(Some(1), None, Some(&screen));
        context.exit("zeus");
        let task = context.task(project.id, 1);
        let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
        assert_eq!(note.body, withheld, "{screen:?}");
        assert!(!note.body.contains(value), "{screen:?}");
    }

    // A command that carries a credential is the tool's name alone; a base64 value with no
    // digit on a row that names nothing is masked.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.with("zeus", |agent| {
        agent.last_call = Some(cf_harness::records::LastCall::shell(
            "Bash",
            "curl -u alice:swordfish https://example.test/",
        ));
    });
    context.host.exits_showing(
        Some(1),
        None,
        Some(&[
            "running",
            "value YWJjYWJjYWJjYWJjYWJjYWJjYWJjYWJjYWJjYWJjYWJjYWJj end",
        ]),
    );
    context.exit("zeus");
    let task = context.task(project.id, 1);
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_eq!(
        note.body,
        "T-1 is paused: @zeus's window closed (exit code 1); its last tool call was Bash; \
         what it was given may name a secret, so it is left out; \
         its screen ended with: \"running / value [masked] end\" \
         (1 key- or token-like string masked). \
         Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
    );
    assert!(!note.body.contains("swordfish") && !note.body.contains("alice"));
    assert!(!note.body.contains("YWJjYWJj"));
}

/// A window's exit reads its conversation's record with no window open, to name the last tool
/// call. A participant whose window was asked for meanwhile, up, on its way or given up, is no
/// business of that exit: it settles nothing more (the guard of `in_flight`'s `launches`),
/// and no pause note says a window closed under a task that another window now has (the
/// review of `a26e0848e`, P3: the fake answered at once, and the guard had no case).
#[test]
fn a_window_that_exits_while_its_record_is_read_and_is_replaced_before_the_read_ends_pauses_nothing(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let session = tiers.assignee(1);
    let first = context.host.last(&session).unwrap().pane;
    let native = native_of(&context, tiers.id(&session)).expect("its conversation");
    context.adapter.with("zeus", |agent| {
        agent.last_call = Some(cf_harness::records::LastCall::shell("Bash", "cargo test"));
    });
    let reading = context
        .adapter
        .last_call_holds
        .hold(move |asked| asked[0] == native.as_str());
    context
        .host
        .exits_showing(Some(1), None, Some(&["Running the workspace tests"]));
    // The window exits; its note waits for the record, which is held.
    let exit = context.begin_exit(&session);
    // The human opens the session's window again meanwhile: a replacement comes up.
    context.open_window(tiers.project.id, &session).unwrap();
    context.pass().unwrap();
    let second = context.host.last(&session).unwrap().pane;
    assert_ne!(second, first, "a replacement window came up");
    reading.open();
    drop(exit);
    context.pass().unwrap();

    let task = tiers.task(1);
    assert_ne!(
        task.task.state, "paused",
        "the replaced window's exit paused nothing"
    );
    assert!(
        task.messages
            .iter()
            .all(|m| !m.body.contains("window closed")),
        "{:?}",
        task.messages.iter().map(|m| &m.body).collect::<Vec<_>>()
    );
}

/// A screen that names a secret puts none of its lines in the pause note, which goes to
/// the board, the chief's window and the log: a passphrase of words is no token, and
/// both reviews of 2026-10-16 found the masker let one syntax after another through.
/// How the window ended is said all the same.
#[test]
fn a_screen_that_names_a_secret_puts_none_of_its_lines_in_the_pause_note() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.host.exits_showing(
        Some(1),
        None,
        Some(&[
            "password: correct horse battery staple",
            "export API_KEY='red green blue'",
            "panicked at src/main.rs:4",
        ]),
    );
    context.exit("zeus");

    let task = context.task(project.id, 1);
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_eq!(
        note.body,
        "T-1 is paused: @zeus's window closed (exit code 1); its last screen lines named a \
         secret, so they are left out. \
         Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
    );
}

/// A window whose program the host could not end in its bound: the pause note says so in
/// place of how it ended, and that it may still run: a resume opens its window beside it,
/// which nothing refuses.
#[test]
fn a_worker_window_whose_program_could_not_be_ended_says_so_in_its_pause_note() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .host
        .exits_showing(None, None, Some(&["cleaning up"]));
    context.host.exits_with_a_survivor(4242);
    context.exit("zeus");

    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "paused");
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_eq!(
        note.body,
        "T-1 is paused: @zeus's window closed, but its program (process 4242) could not be \
         ended and may still run; its screen ended with: \"cleaning up\". Resume it with: cf \
         task resume T-1 \"…\"; its window comes back on its own conversation."
    );
}

/// Whether each launch of `handle` was told the program of its window before was
/// proven gone, in the order prepared.
fn told_gone(context: &Context, handle: &str) -> Vec<bool> {
    context
        .adapter
        .prepared()
        .iter()
        .zip(context.adapter.previous_gone())
        .filter(|(launch, _)| launch["participant"]["handle"] == handle)
        .map(|(_, gone)| gone)
        .collect()
}

/// A window opened again on its conversation is told whether the program of the
/// window before it is gone (`Launch::previous_gone`), which a Claude window
/// takes the record's length for: only where the host said how it ended and named
/// no program it could not end. T-442's reviews: one that may still run would
/// write past the length taken, as the new process's.
#[test]
fn a_window_opened_again_is_told_the_program_before_it_is_gone_only_where_the_host_proved_it() {
    let reopened = |says: &dyn Fn(&Context)| {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        context.give(project.id, "zeus", "Parser");
        context.pass().unwrap();
        context.pass().unwrap();
        says(&context);
        context.exit("zeus");
        context.resume_task(project.id, 1, "Carry on");
        context.pass().unwrap();
        told_gone(&context, "zeus")
    };
    let ended = |context: &Context| context.host.exits_showing(Some(0), None, None);
    assert_eq!(reopened(&ended), [false, true], "ended, and gone");
    let outlived = |context: &Context| {
        ended(context);
        context.host.exits_with_a_survivor(4242);
    };
    assert_eq!(reopened(&outlived), [false, false], "may still run");
    assert_eq!(
        reopened(&|_: &Context| {}),
        [false, false],
        "a host that said nothing of the end"
    );
}

/// The host's proof outlives the daemon that heard it (T-526, 2026-10-25): a worker's
/// window closed with its program proven gone, an update restarted the daemon, and the
/// chief's reopen opened the conversation again under the new daemon, which never saw the
/// close. Told nothing, Claude Code's reader kept the notification the closed process had
/// queued and taken, and read the reopen's answered turn as still under way for four
/// hours. The new daemon is told what the old one proved, and nothing it did not: a
/// program the host could not end, or a host that said nothing, proves nothing after a
/// restart either.
#[test]
fn a_window_opened_again_after_a_restart_is_told_the_program_before_it_is_gone_where_the_daemon_before_proved_it(
) {
    let reopened = |says: &dyn Fn(&Context)| {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        context.give(project.id, "zeus", "Parser");
        context.pass().unwrap();
        context.pass().unwrap();
        says(&context);
        context.exit("zeus");
        // The app is updated: its daemon stops with no window open, and a new one starts.
        context.ledger.borrow_mut().suspend_for_restart().unwrap();
        let after = context.make();
        after.settle_after_restart().unwrap();
        after.resume_project(project.id).unwrap();
        context.resume_task(project.id, 1, "Carry on");
        after.pass().unwrap();
        after.pass().unwrap();
        told_gone(&context, "zeus")
    };
    let ended = |context: &Context| context.host.exits_showing(Some(0), None, None);
    assert_eq!(reopened(&ended), [false, true], "ended, and gone");
    let outlived = |context: &Context| {
        ended(context);
        context.host.exits_with_a_survivor(4242);
    };
    assert_eq!(reopened(&outlived), [false, false], "may still run");
    assert_eq!(
        reopened(&|_: &Context| {}),
        [false, false],
        "a host that said nothing of the end"
    );
}

/// The proof the ledger keeps is spent as the next window opens: a daemon started while
/// that window's program may still run (its end never heard) is told nothing of the
/// program before it.
#[test]
fn the_proof_a_restart_finds_is_spent_by_the_window_that_opened_after_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.host.exits_showing(Some(0), None, None);
    context.exit("zeus");
    context.resume_task(project.id, 1, "Carry on");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(told_gone(&context, "zeus"), [false, true]);

    // The daemon stops with that window open, and no word of its end ever comes.
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(project.id).unwrap();
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(told_gone(&context, "zeus"), [false, true, false]);
}

/// The engine's own close settles a window before the host says how it ended: the
/// host's word, when it comes, is what the next launch is told.
#[test]
fn the_hosts_word_on_a_window_the_engine_closed_itself_is_what_its_next_launch_is_told() {
    let closed_then = |word: bool| {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        context.give(project.id, "zeus", "Parser");
        context.pass().unwrap();
        context.pass().unwrap();
        context.host.exits_showing(Some(0), None, None);
        context.host.hold_exits.set(true);
        context.close_project(project.id).unwrap();
        context.host.hold_exits.set(false);
        if word {
            context.exit("zeus");
        }
        // The Resume brings back T-1, which the Close paused.
        context.resume_project(project.id).unwrap();
        context.pass().unwrap();
        context.pass().unwrap();
        context.pass().unwrap();
        told_gone(&context, "zeus")
    };
    assert_eq!(closed_then(true), [false, true]);
    assert_eq!(closed_then(false), [false, false]);
}

/// The same, with a restart between the host's word and the next launch: the word the
/// host gave after the engine's own close is kept for the daemon after it too.
#[test]
fn the_hosts_word_on_a_window_the_engine_closed_itself_is_kept_for_the_daemon_after_a_restart() {
    let closed_then = |word: bool| {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        context.give(project.id, "zeus", "Parser");
        context.pass().unwrap();
        context.pass().unwrap();
        context.host.exits_showing(Some(0), None, None);
        context.host.hold_exits.set(true);
        context.close_project(project.id).unwrap();
        context.host.hold_exits.set(false);
        if word {
            context.exit("zeus");
        }
        context.ledger.borrow_mut().suspend_for_restart().unwrap();
        let after = context.make();
        after.settle_after_restart().unwrap();
        // The Resume brings back T-1, which the Close paused.
        after.resume_project(project.id).unwrap();
        after.pass().unwrap();
        after.pass().unwrap();
        after.pass().unwrap();
        told_gone(&context, "zeus")
    };
    assert_eq!(closed_then(true), [false, true]);
    assert_eq!(closed_then(false), [false, false]);
}

/// What the close ended goes between how the window ended and its screen.
#[test]
fn a_worker_window_that_ends_mid_task_with_work_running_says_both() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.host.exits_showing(Some(1), None, Some(&["Killed"]));
    context.host.exits_leaving(&["npm test -- --watch"]);
    context.exit("zeus");

    let task = context.task(project.id, 1);
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_match(
        &note.body,
        r#"^T-1 is paused: @zeus's window closed \(exit code 1\) with background work running, which its close ended: npm test -- --watch; its screen ended with: "Killed"\. Resume it with"#,
    );
}

#[test]
fn brings_a_tell_to_a_task_whose_window_is_gone_on_that_windows_own_conversation_still_paused() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let session = native(&context, project.id, "zeus");
    context.exit("zeus");
    assert_eq!(context.task(project.id, 1).task.state, "paused");
    let told = context.tell(project.id, "zeus", 1, "Which grammar did you start from?");
    context.pass().unwrap();
    assert_eq!(
        resumed(&context),
        (json!("zeus"), json!(session)),
        "the same conversation"
    );
    let launch = context.adapter.prepared().pop().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"^\[ConsensFlow m-\d+ · T-1 · question from @chief\]\nWhich grammar did you start from\?\n\nT-1 is paused for this\.",
    );
    context.pass().unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "paused",
        "the task waits for the chief"
    );
    assert_ne!(context.message(told.id).state, "queued");
    held_to(
        context.close(),
        SUITES,
        "brings a tell to a task whose window is gone on that window's own conversation, still paused",
    );
}

#[test]
fn pauses_a_working_task_in_its_open_window() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let launches = context.adapter.prepared().len();
    context.pause_task(project.id, 1);
    context.pass().unwrap();
    context.pass().unwrap();
    let pane = context.host.last("zeus").unwrap().pane;
    assert_eq!(
        context.host.inputs(),
        [json!({ "id": pane.id, "generation": pane.generation, "bytes": [27] })],
        "Escape, once"
    );
    assert!(context.host.killed().is_empty(), "the window stays");
    // Still working three seconds later (a harness that ignored the key while it thought): again, up to three times.
    context.adapter.busy("zeus");
    context.advance(3_100);
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs().len(),
        2,
        "pressed again while the window still works"
    );
    context.advance(3_100);
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 3, "and then no more");
    // Its turn ends after the third round, before the look that would close it:
    // paid at rest, and the window stays.
    context.advance(2_000);
    context.adapter.answer("zeus", "Parser done");
    context.advance(1_100);
    context.pass().unwrap();
    context.advance(3_100);
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 3);
    assert!(
        context.host.killed().is_empty(),
        "the window stopped: it stays"
    );
    let zeus = context.id(project.id, "zeus");
    assert_eq!(context.dispatcher.unstopped(zeus), None);
    assert_eq!(
        context.task(project.id, 1).task.state,
        "paused",
        "said after the pause: not a result"
    );
    context.resume_task(project.id, 1, "Add the tests too");
    context.pass().unwrap();
    assert_eq!(context.adapter.prepared().len(), launches, "no new window");
    let last = context.adapter.agent("zeus").items.pop().unwrap();
    assert_match(
        &last.text,
        r"T-1 · task from @chief\]\nResumed: Add the tests too$",
    );
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser and tests done");
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "done");
    // Not held to Node's recording: its stop is pressed in rounds (`stops.rs`).
}

#[test]
fn fails_the_task_when_the_worker_window_cannot_open() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.host.refuse.set(true);
    context.pass().unwrap();
    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "failed");
    assert_match(
        task.messages[0].reason.as_deref().unwrap(),
        "refused by the test",
    );
    held_to(
        context.close(),
        SUITES,
        "fails the task when the worker window cannot open",
    );
}

#[test]
fn forgets_the_files_a_launch_wrote_when_its_window_never_opens() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.host.refuse.set(true);
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "failed");
    let launched = context.adapter.prepared().pop().unwrap();
    let forgotten = || context.launch_files.forgotten.borrow().clone();
    assert!(
        forgotten().contains(&launched["launchId"].as_str().unwrap().to_owned()),
        "the host refused the window"
    );

    let written = Rc::new(RefCell::new(String::new()));
    let seen = Rc::clone(&written);
    *context.adapter.prepare.borrow_mut() = Some(Rc::new(move |asked| {
        *seen.borrow_mut() = asked["launchId"].as_str().unwrap().to_owned();
        Err("the settings could not be written".to_owned())
    }));
    context.give(project.id, "zeus", "Lexer");
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 2).task.state, "failed");
    assert!(
        forgotten().contains(&written.take()),
        "the adapter failed after writing them"
    );
    held_to(
        context.close(),
        SUITES,
        "forgets the files a launch wrote when its window never opens: refused by the host, or its adapter failed",
    );
}

#[test]
fn fails_a_launch_whose_first_message_never_arrives() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    // The window opens, but its record never shows the brief.
    let fake = Rc::downgrade(&context.adapter);
    *context.adapter.after_prepare.borrow_mut() = Some(Rc::new(move |handle| {
        if let Some(fake) = fake.upgrade() {
            fake.with(handle, |agent| agent.items.clear());
        }
    }));
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.advance(121_000);
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "failed");
    assert_eq!(
        context.host.killed().last().unwrap().id,
        context.host.last("zeus").unwrap().pane.id
    );
    held_to(
        context.close(),
        SUITES,
        "fails a launch whose first message never arrives",
    );
}

#[test]
fn fails_the_first_message_at_once_when_the_harness_cannot_take_it_after_the_window_opens() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    *context.adapter.started.borrow_mut() =
        Some(Rc::new(|| Err("the server never answered".to_owned())));
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "failed");
    assert_match(
        task.messages[0].reason.as_deref().unwrap(),
        "the server never answered",
    );
    assert_eq!(
        context.host.killed().last().unwrap().id,
        context.host.last("zeus").unwrap().pane.id
    );
    held_to(
        context.close(),
        SUITES,
        "fails the first message at once when the harness cannot take it after the window opens",
    );
}
