//! A window that does not come up says why on its own screen (Pi without a
//! login: "No API key found for the selected model … Use /login"), and the
//! screen goes with the window. The pane host keeps it, and how the program
//! ended; the engine tells both in the failure the requester and the human
//! hear, as a short quote, and in one line of the daemon's log. Three ways a
//! launch fails are told that way: the window exits right after its start, it
//! never shows its first message, and it never says which session it opened.
//! None of these tests is held to a Node recording: Node's host said neither.

use std::rc::Rc;

use cf_engine::testing::Context;
use cf_engine::SwitchWhen;
use serde_json::{json, Value};

use crate::chiefs::{to, to_human, with_codex};
use crate::fixtures::notes;

/// What Pi's window said when it had no login to run on.
const PI: [&str; 2] = [
    "No API key found for the selected model.",
    "Use /login to log into a provider.",
];
/// Those two lines as a quote holds them.
const QUOTED: &str =
    "No API key found for the selected model. / Use /login to log into a provider.";

/// The member a task is given by name, whose window is its own.
const ZEUS: &str = "zeus";

/// A project with T-1 given to zeus, whose window is open and whose brief is
/// on its way into it.
fn launched(context: &Context) -> i64 {
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    project.id
}

/// A window whose record never shows the brief it was launched with.
fn never_shows_its_brief(context: &Context) {
    let fake = Rc::downgrade(&context.adapter);
    *context.adapter.after_prepare.borrow_mut() = Some(Rc::new(move |handle| {
        if let Some(fake) = fake.upgrade() {
            fake.with(handle, |agent| agent.items.clear());
        }
    }));
}

/// What T-1's requester and the human were told when it failed, and what the
/// daemon's log was.
struct Told {
    to_chief: Vec<String>,
    to_human: Vec<String>,
    logged: Vec<String>,
}

fn told(context: &Context, project: i64) -> Told {
    Told {
        to_chief: notes(context, context.id(project, "chief")),
        to_human: to_human(context, project),
        logged: context.log.warnings.borrow().clone(),
    }
}

/// The failure note of T-1 the chief is sent, for a failure saying `because`.
fn failure_of_t1(because: &str) -> String {
    format!(r#"T-1 failed: {because}. Reopen it with: cf task reopen T-1 "…""#)
}

#[test]
fn a_window_that_exits_right_after_its_start_fails_its_task_with_its_code_and_what_it_showed() {
    let context = Context::new();
    let project = launched(&context);
    context.host.exits_showing(Some(3), None, Some(&PI));
    context.exit("zeus");

    let task = context.task(project, 1);
    assert_eq!(task.task.state, "failed");
    let because =
        format!(r#"@{ZEUS}'s window closed (exit code 3); its screen ended with: "{QUOTED}""#);
    let told = told(&context, project);
    assert_eq!(told.to_chief, [failure_of_t1(&because)]);
    assert_eq!(
        told.to_human,
        [format!(
            "m-{}, a task from @chief on T-1, did not reach @{ZEUS}: {because}.",
            task.messages[0].id
        )]
    );
    assert_eq!(
        told.logged,
        [format!("the launch of p1-{ZEUS} failed: {because}")]
    );
}

#[test]
fn a_window_that_exits_before_its_open_is_answered_says_the_same() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.host.exits_showing(Some(3), None, Some(&PI));
    // The host sends the exit first, in the same read as its answer to the open.
    *context.host.exit_after_open.borrow_mut() = Some("zeus".to_owned());
    context.pass().unwrap();

    assert_eq!(context.task(project.id, 1).task.state, "failed");
    let because =
        format!(r#"@{ZEUS}'s window closed (exit code 3); its screen ended with: "{QUOTED}""#);
    assert_eq!(
        told(&context, project.id).to_chief,
        [failure_of_t1(&because)]
    );
}

#[test]
fn a_program_a_signal_ended_is_told_by_the_signal() {
    let context = Context::new();
    let project = launched(&context);
    context
        .host
        .exits_showing(Some(1), Some("Killed: 9"), Some(&["Out of memory"]));
    context.exit("zeus");

    assert_eq!(
        told(&context, project).to_chief,
        [failure_of_t1(&format!(
            r#"@{ZEUS}'s window closed (ended by signal Killed: 9); its screen ended with: "Out of memory""#
        ))]
    );
}

#[test]
fn a_window_that_never_shows_its_first_message_fails_with_what_its_screen_was_showing() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    never_shows_its_brief(&context);
    context.host.set_snapshot(json!({ "tail": PI }));
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.advance(121_000);
    context.pass().unwrap();

    assert_eq!(context.task(project.id, 1).task.state, "failed");
    let because =
        format!(r#"the window never showed its first message; its screen ended with: "{QUOTED}""#);
    let told = told(&context, project.id);
    assert_eq!(told.to_chief, [failure_of_t1(&because)]);
    assert_eq!(
        told.logged,
        [format!("the launch of p1-{ZEUS} failed: {because}")]
    );

    // The screen goes with the window, so it was asked for before the window was killed.
    let events = context.recorder.events();
    let position = |wanted: &dyn Fn(&Value) -> bool| events.iter().position(wanted);
    let asked = position(&|event| {
        event["seam"] == "host"
            && event["method"] == "request"
            && event["args"][0] == "pane.snapshot"
            && event["args"][1]["tail"].is_u64()
    });
    let killed = position(&|event| event["seam"] == "host" && event["method"] == "kill");
    assert!(
        asked.is_some() && asked < killed,
        "asked at {asked:?}, killed at {killed:?}"
    );
}

#[test]
fn a_window_that_never_says_which_session_it_opened_fails_with_what_its_screen_was_showing() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    *context.adapter.started.borrow_mut() = Some(Rc::new(|| {
        Err("Devin never said which session it opened (its wire log stayed empty)".to_owned())
    }));
    context.host.set_snapshot(json!({ "tail": PI }));
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();

    assert_eq!(context.task(project.id, 1).task.state, "failed");
    let because = format!(
        "the window could not take its first message: Devin never said which session it opened \
         (its wire log stayed empty); its screen ended with: \"{QUOTED}\""
    );
    let told = told(&context, project.id);
    assert_eq!(told.to_chief, [failure_of_t1(&because)]);
    assert_eq!(
        told.logged,
        [format!("the launch of p1-{ZEUS} failed: {because}")]
    );
}

#[test]
fn an_exit_that_comes_while_the_host_says_what_the_window_shows_settles_the_launch_once() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    never_shows_its_brief(&context);
    // What the host answers, late, is what the screen showed before the exit.
    context.host.set_snapshot(json!({ "tail": PI }));
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.advance(121_000);
    // The pass gives the window up, and asks the host what it shows: the answer is late.
    let answer = context
        .host
        .request_holds
        .hold(|args| args[0] == "pane.snapshot" && args[1].get("tail").is_some());
    let passing = context.begin_pass();
    context.settle();
    // The window exits meanwhile, and says how.
    context.host.exits_showing(Some(3), None, Some(&PI));
    context.exit("zeus");
    answer.open();
    context.settle();
    passing.take().unwrap().unwrap();

    assert_eq!(context.task(project.id, 1).task.state, "failed");
    let because =
        format!(r#"@{ZEUS}'s window closed (exit code 3); its screen ended with: "{QUOTED}""#);
    let told = told(&context, project.id);
    assert_eq!(
        told.to_chief,
        [failure_of_t1(&because)],
        "told once, with what the exit said"
    );
    assert_eq!(
        told.logged,
        [format!("the launch of p1-{ZEUS} failed: {because}")],
        "and logged once"
    );
    assert!(
        context.host.killed().is_empty(),
        "the window was gone: nothing was left to close"
    );
}

#[test]
fn an_exit_that_comes_while_the_host_says_what_a_window_that_took_no_message_shows_settles_it_once()
{
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    *context.adapter.started.borrow_mut() =
        Some(Rc::new(|| Err("the server never answered".to_owned())));
    context.host.set_snapshot(json!({ "tail": PI }));
    context.give(project.id, "zeus", "Parser");
    let answer = context
        .host
        .request_holds
        .hold(|args| args[0] == "pane.snapshot" && args[1].get("tail").is_some());
    let passing = context.begin_pass();
    context.settle();
    context.host.exits_showing(Some(3), None, Some(&PI));
    context.exit("zeus");
    answer.open();
    context.settle();
    passing.take().unwrap().unwrap();

    assert_eq!(context.task(project.id, 1).task.state, "failed");
    let because =
        format!(r#"@{ZEUS}'s window closed (exit code 3); its screen ended with: "{QUOTED}""#);
    let told = told(&context, project.id);
    assert_eq!(told.to_chief, [failure_of_t1(&because)], "told once");
    assert_eq!(
        told.logged,
        [format!("the launch of p1-{ZEUS} failed: {because}")],
        "and logged once"
    );
}

#[test]
fn an_empty_screen_says_so_plainly() {
    let context = Context::new();
    let project = launched(&context);
    context.host.exits_showing(Some(0), None, Some(&[]));
    context.exit("zeus");

    assert_eq!(
        told(&context, project).to_chief,
        [failure_of_t1(&format!(
            "@{ZEUS}'s window closed (exit code 0); its screen was empty"
        ))]
    );
}

#[test]
fn a_screen_with_a_great_deal_on_it_is_cut_to_its_last_lines_and_a_few_hundred_characters() {
    let context = Context::new();
    let project = launched(&context);
    let lines: Vec<String> = (1..=30)
        .map(|number| format!("output line number {number} of the window"))
        .collect();
    let shown: Vec<&str> = lines.iter().map(String::as_str).collect();
    context.host.exits_showing(Some(1), None, Some(&shown));
    context.exit("zeus");

    let note = told(&context, project).to_chief.remove(0);
    assert!(
        note.contains("output line number 30 of the window"),
        "{note}"
    );
    assert!(
        note.contains("output line number 25 of the window"),
        "{note}"
    );
    assert!(
        !note.contains("output line number 24 of the window"),
        "six lines at most: {note}"
    );

    // One line of thousands of characters is cut too, and says it was.
    let context = Context::new();
    let project = launched(&context);
    let long = "x".repeat(3_000);
    context
        .host
        .exits_showing(Some(1), None, Some(&["start", &long]));
    context.exit("zeus");
    let note = told(&context, project).to_chief.remove(0);
    assert!(
        note.chars().count() < 600,
        "{} characters",
        note.chars().count()
    );
    assert!(note.contains("…\""), "{note}");
}

#[test]
fn a_key_looking_string_on_the_screen_is_masked_in_the_note_and_in_the_log() {
    let context = Context::new();
    let project = launched(&context);
    let key = "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789";
    let screen = format!("Invalid API key: {key}");
    context
        .host
        .exits_showing(Some(1), None, Some(&[screen.as_str()]));
    context.exit("zeus");

    let told = told(&context, project);
    let because = format!(
        "@{ZEUS}'s window closed (exit code 1); its screen ended with: \"Invalid API key: [masked]\" \
         (1 key- or token-like string masked)"
    );
    assert_eq!(told.to_chief, [failure_of_t1(&because)]);
    assert_eq!(
        told.logged,
        [format!("the launch of p1-{ZEUS} failed: {because}")]
    );
    for said in told
        .to_chief
        .iter()
        .chain(&told.to_human)
        .chain(&told.logged)
    {
        assert!(!said.contains("AbCdEf"), "{said}");
    }
}

#[test]
fn a_host_that_says_nothing_leaves_the_failure_as_it_was_and_the_log_empty() {
    let context = Context::new();
    let project = launched(&context);
    context.exit("zeus");

    let told = told(&context, project);
    assert_eq!(
        told.to_chief,
        [failure_of_t1(&format!("@{ZEUS}'s window closed"))]
    );
    assert!(told.logged.is_empty(), "{:?}", told.logged);
}

#[test]
fn a_chief_whose_window_closes_before_its_first_message_showed_tells_the_human_why() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    // The new chief's window never shows the handoff, and exits.
    context.codex.with("chief", |agent| agent.items.clear());
    context
        .host
        .exits_showing(Some(1), None, Some(&["codex: command not found"]));
    context.exit("chief");

    assert_eq!(context.project(project.id).state, "suspended");
    assert_eq!(
        to_human(&context, project.id),
        [
            "@chief's window closed (exit code 1); its screen ended with: \"codex: command not found\". \
             The project is closed: resume it to try again; what comes for the chief waits for it."
        ]
    );
    assert_eq!(
        context.log.warnings.borrow().clone(),
        [
            "the launch of p1-chief failed: @chief's window closed (exit code 1); \
             its screen ended with: \"codex: command not found\""
        ]
    );
}

#[test]
fn a_window_that_closes_under_a_paste_is_no_failed_launch_and_its_screen_is_not_quoted() {
    let context = Context::new();
    let project = context.with_staff(&[ZEUS]);
    context.pass().unwrap();
    // A message goes into the chief's window, which never shows it, and closes.
    context.adapter.with("chief", |agent| agent.arrive = false);
    let ready = context.note(project.id, ZEUS, "chief", "Ready");
    context.pass().unwrap();
    assert_eq!(context.message(ready.id).state, "delivering");
    context.host.exits_showing(Some(1), None, Some(&PI));
    context.exit("chief");

    let waiting = context.message(ready.id);
    assert_eq!(waiting.state, "queued", "it goes again, to the next window");
    assert_eq!(waiting.reason.as_deref(), Some("@chief's window closed"));
    assert!(context.log.warnings.borrow().is_empty());
    assert!(to_human(&context, project.id).is_empty());
}
