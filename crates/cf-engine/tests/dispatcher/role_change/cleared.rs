//! T-829 (a P3 of Calliope's T-824): a window its human switched to a session no hook
//! of its harness greeted (Muse Code's `/clear`, which runs no SessionStart hook) began
//! that conversation with no role text. On a harness that takes its role at every
//! launch, the engine writes the role for it, and it goes in first; once delivered, the
//! conversation is recorded as told it. A session a hook greeted, and one on a harness
//! that keeps its first instructions, are given nothing.

use cf_engine::testing::Context;
use cf_ledger::MessageView;

use super::recorded;
use crate::chiefs::to_human;
use crate::resume::at_work;

/// The words that tell the note that gives a cleared session its role.
const BEGAN_WITHOUT: &str = "began without your ConsensFlow role instructions";

/// The notes ConsensFlow wrote to `participant` that give a session its role, oldest first.
fn greetings(context: &Context, participant: i64) -> Vec<MessageView> {
    let mut notes: Vec<MessageView> = context
        .inbox(participant)
        .into_iter()
        .filter(|message| {
            message.sender.is_none()
                && message.kind == "note"
                && message.body.contains(BEGAN_WITHOUT)
        })
        .collect();
    notes.reverse();
    notes
}

#[test]
fn a_session_no_hook_greeted_is_given_the_role_first_and_one_a_hook_greeted_nothing() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    // A /resume of an older one: its hook greeted it.
    context.adapter.switch_to("chief", "native-older");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(greetings(&context, chief).is_empty(), "a greeted session");
    // A /clear that no hook greeted: the role goes in first, and is recorded told.
    context.adapter.with("chief", |agent| {
        agent.ungreeted.push("native-cleared".into())
    });
    context.adapter.switch_to("chief", "native-cleared");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let notes = greetings(&context, chief);
    assert_eq!(notes.len(), 1, "{notes:?}");
    let note = context.message(notes[0].id);
    assert_eq!(note.state, "delivered");
    let role = note
        .body
        .split_once("\n\n")
        .expect("the role after the opening")
        .1;
    assert_eq!(recorded(&context, chief).as_deref(), Some(role));
    // Looked at again, the session is told no more.
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(greetings(&context, chief).len(), 1);
    // A second /clear in the same window is given the role too.
    context.adapter.with("chief", |agent| {
        agent.ungreeted.push("native-cleared-2".into())
    });
    context.adapter.switch_to("chief", "native-cleared-2");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(greetings(&context, chief).len(), 2);
    // Back on the first, which was told the role as it is: nothing more.
    context.adapter.switch_to("chief", "native-cleared");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(greetings(&context, chief).len(), 2);
}

#[test]
fn a_harness_that_keeps_its_first_instructions_is_given_nothing_at_a_switch() {
    let context = Context::new();
    context.adapter.keeps_first_instructions.set(true);
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| {
        agent.ungreeted.push("native-cleared".into())
    });
    context.adapter.switch_to("chief", "native-cleared");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(greetings(&context, chief).is_empty());
}

/// The panes the host was asked to kill, by their ids.
fn killed(context: &Context) -> Vec<String> {
    context
        .host
        .killed()
        .into_iter()
        .map(|pane| pane.id)
        .collect()
}

/// T-931 P1-1: the greeting is no launch. A member that switched to a session no hook
/// greeted and is at work there past the time a launch has (180 s) keeps its window, and
/// the human is told nothing; the role goes in once the window rests.
#[test]
fn a_member_at_work_long_after_a_switch_to_a_session_no_hook_greeted_keeps_its_window() {
    let context = Context::new();
    let tiers = at_work(&context);
    let zeus = context.id(tiers.project.id, "zeus-amber-pine");
    context.adapter.with("zeus-amber-pine", |agent| {
        agent.ungreeted.push("native-cleared".into())
    });
    context
        .adapter
        .switch_to("zeus-amber-pine", "native-cleared");
    context
        .adapter
        .with("zeus-amber-pine", |agent| agent.settled = false);
    context.pass().unwrap();
    assert_eq!(greetings(&context, zeus).len(), 1, "the role is written");
    for _ in 0..4 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    assert!(killed(&context).is_empty(), "{:?}", killed(&context));
    let told = to_human(&context, tiers.project.id);
    assert!(
        !told.iter().any(|body| body.contains("could not start")),
        "{told:?}"
    );
    // At rest, it is given the role.
    context
        .adapter
        .with("zeus-amber-pine", |agent| agent.settled = true);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let note = context.message(greetings(&context, zeus)[0].id);
    assert_eq!(note.state, "delivered");
}

/// T-931 P1-1: a chief whose window exits with the greeting still to go closes its
/// project, as any chief's exit does; no launch is tried again for it.
#[test]
fn a_chief_that_exits_with_its_greeting_queued_suspends_its_project() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| {
        agent.ungreeted.push("native-cleared".into())
    });
    context.adapter.switch_to("chief", "native-cleared");
    context.adapter.with("chief", |agent| agent.settled = false);
    context.pass().unwrap();
    assert_eq!(greetings(&context, chief).len(), 1);
    context.exit("chief");
    context.pass().unwrap();
    assert_eq!(context.project(project.id).state, "suspended");
    let told = to_human(&context, project.id);
    assert!(
        !told.iter().any(|body| body.contains("could not start")),
        "{told:?}"
    );
}

/// T-931 P1-1: a greeting that never gets in is given up as a message is, and the
/// chief's window, which is up, stays open: no launch failed.
#[test]
fn a_greeting_the_chief_s_window_refuses_fails_and_the_window_stays_open() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| {
        agent.ungreeted.push("native-cleared".into())
    });
    context.adapter.switch_to("chief", "native-cleared");
    context.adapter.with("chief", |agent| agent.admit = false);
    for _ in 0..8 {
        context.advance(5_000);
        context.pass().unwrap();
    }
    let note = context.message(greetings(&context, chief)[0].id);
    assert_eq!(note.state, "failed", "{note:?}");
    assert!(killed(&context).is_empty(), "{:?}", killed(&context));
    assert_eq!(context.project(project.id).state, "open");
    let told = to_human(&context, project.id);
    assert!(
        !told.iter().any(|body| body.contains("could not start")),
        "{told:?}"
    );
}

/// T-931 P3-4: a greeting is decided at the switch, and read again before it goes in. A
/// harness whose hook greeted the session meanwhile (a later Muse that runs its
/// SessionStart hook on a `/clear`, at the first prompt) has the note withdrawn: the
/// session is not told its role twice.
#[test]
fn a_greeting_whose_session_a_hook_greeted_meanwhile_is_withdrawn_unsent() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| {
        agent.ungreeted.push("native-cleared".into())
    });
    context.adapter.switch_to("chief", "native-cleared");
    context.adapter.with("chief", |agent| agent.settled = false);
    context.pass().unwrap();
    let note = greetings(&context, chief)[0].id;
    assert_eq!(context.message(note).state, "queued");
    // The hook spoke at the session's first prompt.
    context.adapter.with("chief", |agent| {
        agent.ungreeted.clear();
        agent.settled = true;
    });
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(note).state, "cancelled");
    assert!(
        !crate::withheld_enter::handed(&context, "adapter:claude-code")
            .iter()
            .any(|text| text.contains(BEGAN_WITHOUT)),
        "never pasted"
    );
}

/// T-931: a second session no hook greeted, before the first one's greeting went in, writes
/// no second note: the one that waits goes into the session the window shows then.
#[test]
fn two_switches_before_the_greeting_goes_in_write_one_note() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| {
        agent.ungreeted.push("native-cleared".into());
        agent.ungreeted.push("native-cleared-2".into());
    });
    context.adapter.switch_to("chief", "native-cleared");
    context.adapter.with("chief", |agent| agent.settled = false);
    context.pass().unwrap();
    context.adapter.switch_to("chief", "native-cleared-2");
    context.adapter.with("chief", |agent| agent.settled = false);
    context.pass().unwrap();
    assert_eq!(greetings(&context, chief).len(), 1);
    context.adapter.with("chief", |agent| agent.settled = true);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let notes = greetings(&context, chief);
    assert_eq!(notes.len(), 1);
    assert_eq!(context.message(notes[0].id).state, "delivered");
}
