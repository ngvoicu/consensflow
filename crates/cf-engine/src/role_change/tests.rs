use std::sync::Arc;

use cf_base::env::Env;
use cf_catalog::WorkTier;
use cf_proto::specs::Variant;
use serde_json::Value;

use super::*;
use crate::delivery_text::{delivery_text, goes_whole};
use crate::roles::{role_instructions, ProjectSpecs, StaffRow};
use crate::testing::Context;

/// A note to the chief as the ledger would show it, saying `body`.
fn note(sender: Option<&str>, kind: &str, body: String) -> MessageView {
    MessageView {
        id: 4242,
        project_id: 1,
        recipient: "chief".to_owned(),
        recipient_id: 2,
        recipient_role: "chief".to_owned(),
        sender: sender.map(str::to_owned),
        kind: kind.to_owned(),
        task_number: None,
        reply_to: None,
        body,
        state: "queued".to_owned(),
        attempts: 0,
        reason: None,
        receipt: Value::Null,
        questions: Value::Null,
        choices: Value::Null,
        urgent: false,
        created_at: "2026-10-13T12:00:00.000Z".to_owned(),
        delivered_at: None,
    }
}

#[test]
fn a_replacement_says_plainly_that_it_replaces_the_instructions_the_conversation_began_with_and_holds_the_text_whole_after_it(
) {
    let body = body_of("the role\nin full");
    assert!(body.starts_with(OPENING));
    assert!(
        OPENING.contains("replace the ones you were given then"),
        "{OPENING}"
    );
    assert!(body.ends_with("\n\nthe role\nin full"), "{body}");
}

#[test]
fn the_role_a_replacement_says_is_what_follows_its_opening_and_no_other_note_says_one() {
    for role in [
        "a role",
        "the role\nin full\n",
        "",
        "quotes \"and\" `ticks` \\ ✓",
    ] {
        let replacement = note(None, "note", body_of(role));
        assert_eq!(instructions_of(&replacement), Some(role), "{role:?}");
    }
    let lookalike = format!("{OPENING} but no blank line and no role");
    assert_eq!(instructions_of(&note(None, "note", lookalike)), None);
    // Not from ConsensFlow, or not a note, or not opening as one does.
    for other in [
        note(Some("chief"), "note", body_of("a role")),
        note(None, "question", body_of("a role")),
        note(None, "note", format!("Quoted: {}", body_of("a role"))),
        note(
            None,
            "note",
            "ConsensFlow stopped in the middle of your turn".to_owned(),
        ),
    ] {
        assert_eq!(instructions_of(&other), None, "{}", other.body);
    }
}

#[test]
fn a_replacement_is_a_note_from_consensflow_that_opens_as_one_does_and_no_other_is() {
    let replacement = body_of("a role");
    assert!(is_replacement(&note(None, "note", replacement.clone())));
    assert!(!is_replacement(&note(
        Some("chief"),
        "note",
        replacement.clone()
    )));
    assert!(!is_replacement(&note(None, "question", replacement)));
    assert!(!is_replacement(&note(
        None,
        "note",
        "ConsensFlow stopped in the middle of your turn, which was cut.".to_owned()
    )));
    assert!(!is_replacement(&note(
        None,
        "note",
        format!("Quoted: {OPENING}")
    )));
}

/// The real text of a chief with `rows` staff rows who each hold two roles, with
/// specs on or off: the paths of a Mac, as long as the folder its skill's copy is
/// kept in makes its path.
fn a_chiefs_role(rows: usize, specs_on: bool) -> String {
    let staff: Vec<StaffRow> = (0..rows)
        .map(|at| StaffRow {
            name: format!("member-{at}"),
            roles: vec!["worker".to_owned(), "reviewer".to_owned()],
            work_tier: Some(WorkTier::Standard),
        })
        .collect();
    let specs = ProjectSpecs {
        variant: Variant::TddHtml,
        folder: "docs/specs",
        skill: "/Users/someone/.consensflow/integrations/specmint/codex/tdd-html-0123456789abcdef0123456789abcdef01234567/.claude/skills/specmint-tdd-html/SKILL.md",
    };
    role_instructions(
        &Env::default(),
        "chief",
        &staff,
        Some("/Applications/ConsensFlow.app/Contents/Resources/bin/cf"),
        specs_on.then_some(&specs),
    )
    .unwrap()
}

/// The real text of a chief with specs on and a staff of twenty-five who each hold
/// two roles is still under what a message goes in whole at: it is the role itself
/// the replacement says, not its opening and a command to read the rest of it.
#[test]
fn the_real_text_of_a_chief_with_specs_and_a_large_staff_goes_in_whole_as_a_replacement() {
    let text = a_chiefs_role(25, true);
    let replacement = note(None, "note", body_of(&text));
    assert!(goes_whole(&replacement, &[]));
    let said = delivery_text(&replacement, &[]);
    assert!(said.contains(&text), "the role is cut");
    assert!(
        !said.contains("read all of it with"),
        "a command stands in for its end"
    );
}

/// The staff a role holds whole in a replacement, which the docs of this module
/// say: more rows are a role told in part.
#[test]
fn the_staff_a_replacement_holds_whole_is_as_large_as_the_docs_say() {
    let rows_whole = |specs_on| {
        (1..)
            .take_while(|&rows| {
                goes_whole(
                    &note(None, "note", body_of(&a_chiefs_role(rows, specs_on))),
                    &[],
                )
            })
            .last()
            .unwrap_or(0)
    };
    assert_eq!(rows_whole(true), 43, "docs: whole to 43 rows with specs on");
    assert_eq!(rows_whole(false), 72, "docs: whole to 72 rows without");
}

/// How many units of a Windows command line a word takes, as the pane host
/// (portable-pty) writes it: a word with a blank or a quote in it is wrapped in
/// quotes, and each `"` in it is backslashed, with the backslashes before it
/// doubled.
fn on_the_line(word: &str) -> usize {
    let units: Vec<u16> = word.encode_utf16().collect();
    let needs_quotes = units.is_empty()
        || units
            .iter()
            .any(|unit| [0x20, 0x09, 0x0a, 0x0b, 0x22].contains(unit));
    if !needs_quotes {
        return units.len();
    }
    let mut taken = 2;
    let mut at = 0;
    while at < units.len() {
        let slashes = units[at..].iter().take_while(|unit| **unit == 0x5c).count();
        at += slashes;
        match units.get(at) {
            None => taken += 2 * slashes,
            Some(0x22) => taken += 2 * slashes + 2,
            Some(_) => taken += slashes + 1,
        }
        at += 1;
    }
    taken
}

/// What the chief's role comes to on a Windows command line with `rows` staff
/// rows (two roles each), as the developer instructions of its window: the real
/// text with specs on, or off, the path of a skill's copy, `cf`'s path, and
/// `framing` units before it, as the harness and Codex's own instructions add.
fn on_a_windows_line(rows: usize, specs_on: bool, framing: usize) -> usize {
    let staff: Vec<StaffRow> = (0..rows)
        .map(|at| StaffRow {
            name: format!("member-{at}"),
            roles: vec!["worker".to_owned(), "reviewer".to_owned()],
            work_tier: Some(WorkTier::Standard),
        })
        .collect();
    let specs = ProjectSpecs {
        variant: Variant::TddHtml,
        folder: "docs/specs",
        skill: "C:/Users/someone/AppData/Roaming/consensflow/integrations/specmint/codex/tdd-html-0123456789abcdef0123456789abcdef01234567/.claude/skills/specmint-tdd-html/SKILL.md",
    };
    let text = role_instructions(
        &Env::default(),
        "chief",
        &staff,
        Some("C:\\Program Files\\ConsensFlow\\cf.exe"),
        specs_on.then_some(&specs),
    )
    .unwrap();
    let developer = format!(
        "developer_instructions={}",
        serde_json::to_string(&format!("{}{text}", "x".repeat(framing))).unwrap()
    );
    on_the_line(&developer)
}

/// What the docs of this module and of `codex/role.rs` say of the command line of
/// a Windows launch, held to the real text. The role is on it once, as the
/// developer instructions, a launch that says a changed role included: the note
/// that says it goes in by paste once the window is up. The arithmetic is theirs;
/// a role text that grows or shrinks by much moves these sizes, and the docs with
/// them: it is to be measured again.
#[test]
fn the_command_line_of_a_windows_launch_is_the_size_the_docs_say() {
    /// What the rest of the line takes: the two programs' paths, the flags, the
    /// thread's id and the model.
    const REST: usize = 400;
    /// What the harness adds before the role in the developer instructions.
    const FRAMING: usize = 190;
    /// What Codex's own instructions are, the human's own included, at most.
    const CODEX_OWN: usize = 2_500;
    /// The most a Windows command line takes, its end excluded.
    const LIMIT: usize = 32_766;

    let word = |rows| on_a_windows_line(rows, true, FRAMING);
    assert_eq!(word(1), 14_122, "docs: 14,122 units at 1 row");
    assert_eq!(word(25), 15_313, "docs: 15,313 units at 25 rows");
    assert_eq!(word(45), 16_313, "docs: 16,313 units at 45 rows");
    assert_eq!(
        on_a_windows_line(25, false, FRAMING),
        13_891,
        "docs: 1,422 fewer without specs"
    );

    // The most rows a line holds with the rest of it, and then with Codex's own
    // instructions (and the 200 units or so that quoting them takes), and without
    // specs.
    let rows_held = |specs_on, framing| {
        (1..)
            .take_while(|&rows| on_a_windows_line(rows, specs_on, framing) + REST <= LIMIT)
            .last()
            .unwrap_or(0)
    };
    assert_eq!(rows_held(true, FRAMING), 360, "docs: 360 rows");
    assert_eq!(
        rows_held(true, FRAMING + CODEX_OWN + 200),
        307,
        "docs: 307 rows with Codex's own instructions"
    );
    assert_eq!(
        rows_held(false, FRAMING),
        388,
        "docs: 388 rows without specs"
    );
}

#[test]
fn a_message_of_a_conversation_says_a_replacement_where_it_holds_the_opening_of_one() {
    let said = delivery_text(&note(None, "note", body_of("a role")), &[]);
    assert!(says_a_replacement(&said));
    for other in [
        "Write the parser",
        "ConsensFlow stopped in the middle of your turn, which was cut.",
        "Your ConsensFlow role instructions have changed",
        "",
    ] {
        assert!(!says_a_replacement(other), "{other:?}");
    }
}

/// An item of a conversation's record, from `role`, saying `text`.
fn said_by(role: Role, text: &str, complete: bool) -> Item {
    Item {
        id: Arc::from(format!("i-{}", text.len()).as_str()),
        role,
        text: Arc::from(text),
        complete,
        at: None,
        commentary: false,
    }
}

/// What the answers of a record, as `answers_to_the_task` says them, say.
fn answers(items: &[Item]) -> Vec<String> {
    answers_to_the_task(items)
        .into_iter()
        .map(|item| item.text.to_string())
        .collect()
}

#[test]
fn the_words_a_window_gives_a_replacement_of_its_role_are_no_answer_to_its_task() {
    let replacement = delivery_text(&note(None, "note", body_of("a role")), &[]);
    let user = |text: &str| said_by(Role::User, text, true);
    let assistant = |text: &str| said_by(Role::Assistant, text, true);

    assert_eq!(answers(&[]), Vec::<String>::new());
    assert_eq!(answers(&[assistant("Parser done")]), ["Parser done"]);
    // The word that follows the role is the window's word of its role, and none of the task's.
    assert_eq!(
        answers(&[user(&replacement), assistant("Understood.")]),
        Vec::<String>::new()
    );
    // What it said of the task before the role came is the task's, and is kept.
    assert_eq!(
        answers(&[
            assistant("Parser done"),
            user(&replacement),
            assistant("Understood.")
        ]),
        ["Parser done"]
    );
    // And what it says to the next message is the task's again, whoever the message is from.
    assert_eq!(
        answers(&[
            user(&replacement),
            assistant("Understood."),
            user("ConsensFlow quit while you were working on T-1."),
            assistant("Parser done"),
        ]),
        ["Parser done"]
    );
    // A turn's several answers, and the tool's output between them, all stay.
    assert_eq!(
        answers(&[
            user(&replacement),
            assistant("Understood."),
            user("Go on"),
            assistant("First"),
            said_by(Role::Tool, "output", true),
            assistant("Second"),
        ]),
        ["First", "Second"]
    );
}

#[test]
fn a_replacement_whose_recipient_has_no_conversation_any_more_is_confirmed_and_records_nothing() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let chief = context.id(project.id, "chief");
    let conversation = context
        .ledger
        .borrow()
        .current_conversation(chief)
        .unwrap()
        .expect("a conversation");
    context
        .ledger
        .borrow_mut()
        .set_conversation_role_text(conversation.id, "the role it began with")
        .unwrap();
    let said = context.note_from_consensflow(project.id, "chief", &body_of("the role it is now"));
    let begun = context.ledger.borrow_mut().begin_delivery(said.id).unwrap();
    // The conversation ended while the note was on its way.
    context
        .ledger
        .borrow_mut()
        .end_conversation(conversation.id)
        .unwrap();

    context
        .dispatcher
        .confirm_delivered(&begun.message, &serde_json::json!({ "item": "i-1" }))
        .unwrap();
    assert_eq!(context.message(said.id).state, "delivered");
    assert_eq!(
        context
            .ledger
            .borrow()
            .conversation_role_text(conversation.id)
            .unwrap()
            .as_deref(),
        Some("the role it began with"),
        "there was no conversation to have told"
    );
    context.close();
}
