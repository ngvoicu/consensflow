//! A text typed into a window starts each command it teaches with that window's own `cf`
//! (T-1049): the shell of a window can find another `cf` first on its PATH (a Devin member's
//! login zsh, a Claude Code member's login Git Bash, 2026-10-31), so what ConsensFlow wrote of
//! a message, and the footer and the command that reads the rest it adds, say the path, and
//! what a participant wrote stays as it was. The tests of their own, held to no trace, as
//! Node's windows were all of one `cf`.

use cf_engine::testing::Context;
use cf_harness::records::Role;
use cf_proto::cf_word::{CfWord, Shell};

const CF: &str = "/Users/me/consensflow-dist/Contents/Resources/cli/bin/cf";

/// What the window of `handle` was given as the message `id`, whole.
fn given(context: &Context, handle: &str, id: i64) -> String {
    let header = format!("[ConsensFlow m-{id} ");
    context
        .adapter
        .agent(handle)
        .items
        .iter()
        .filter(|item| item.role == Role::User)
        .map(|item| item.text.to_string())
        .find(|text| text.contains(&header))
        .unwrap_or_else(|| panic!("m-{id} was not given to {handle}"))
}

#[test]
fn what_consensflow_wrote_of_a_note_reaches_a_window_with_the_commands_started_by_its_own_cf() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    *context.roles.cf.borrow_mut() = CfWord::new(CF, Shell::Posix);
    let own = context.note_from_consensflow(
        project.id,
        "chief",
        "T-1 is paused: @zeus's window is gone. Resume it with: cf task resume T-1 \"…\"",
    );
    context.pass().unwrap();
    // One message goes into a window at a time: the next once the turn it began is over.
    context.adapter.answer("chief", "Understood");
    let theirs = context.note(project.id, "zeus", "chief", "Run cf task get T-1 yourself");
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(
        given(&context, "chief", own.id)
            .ends_with(&format!("Resume it with: {CF} task resume T-1 \"…\"")),
        "ConsensFlow's own words say the cf of the window"
    );
    assert!(
        given(&context, "chief", theirs.id).ends_with("\nRun cf task get T-1 yourself"),
        "a member's words are its own"
    );
}

#[test]
fn a_window_that_is_told_no_path_is_given_the_word_cf_as_it_always_was() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let own =
        context.note_from_consensflow(project.id, "chief", "Resume it with: cf task resume T-1");
    context.pass().unwrap();
    assert!(given(&context, "chief", own.id).ends_with("\nResume it with: cf task resume T-1"));
}
