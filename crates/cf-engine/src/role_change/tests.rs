use cf_proto::cf_word::{CfWord, Shell};
use std::sync::Arc;

use cf_base::env::Env;
use cf_base::role_read::{printed_part, role_read_whole};
use cf_ledger::{STAFF_RULES_MAX_BYTES, STAFF_RULES_MAX_LINES};
use cf_proto::agents::WorkTier;
use cf_proto::specs::Variant;
use serde_json::Value;

use super::*;
use crate::deliveries::Delivering;
use crate::delivery_text::{delivery_text, goes_whole};
use crate::roles::{
    role_instructions, role_instructions_for, with_staff_rules, ProjectSpecs, StaffRow,
};
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
    let body = body_of("the role\nin full", &CfWord::bare());
    assert!(body.starts_with(OPENING));
    assert!(
        OPENING.contains("replace the ones you were given then"),
        "{OPENING}"
    );
    assert!(body.ends_with("\n\nthe role\nin full"), "{body}");
}

#[test]
fn the_ledger_knows_a_replacement_by_the_role_it_was_written_to_say_and_no_note_by_its_opening() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    let said = context
        .ledger
        .borrow_mut()
        .note_role(
            project.id,
            "chief",
            &body_of("a role", &CfWord::bare()),
            "a role",
        )
        .unwrap();
    // The same words, from ConsensFlow itself and from a participant, and the
    // opening quoted: none of them says a role.
    let alike =
        context.note_from_consensflow(project.id, "chief", &body_of("a role", &CfWord::bare()));
    let by_zeus = context.note(
        project.id,
        "zeus",
        "chief",
        &body_of("a role", &CfWord::bare()),
    );
    let quoting = context.note(project.id, "zeus", "chief", &format!("Quoted: {OPENING}"));
    let hand_off = context.note_from_consensflow(
        project.id,
        "chief",
        "ConsensFlow stopped in the middle of your turn, which was cut.",
    );
    assert_eq!(
        context.ledger.borrow().role_notes_to(chief).unwrap(),
        [said.id],
        "{:?}",
        [alike.id, by_zeus.id, quoting.id, hand_off.id]
    );
    // What it is, the replacement is for good, whatever became of it.
    context
        .ledger
        .borrow_mut()
        .cancel_message(said.id, "withdrawn")
        .unwrap();
    assert_eq!(
        context.ledger.borrow().role_notes_to(chief).unwrap(),
        [said.id]
    );
    context.close();
}

#[test]
fn a_launch_writes_the_replacement_as_the_note_the_ledger_knows_and_whose_words_it_wrote() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    let participant = context
        .project(project.id)
        .participants
        .into_iter()
        .find(|participant| participant.role == "chief")
        .expect("a chief");
    let role = RoleText {
        specs: None,
        instructions: "the role\nin full".to_owned(),
    };
    let written = context
        .dispatcher
        .replace_role(&context.project(project.id), &participant, &role)
        .unwrap();
    assert_eq!(written.body, body_of("the role\nin full", &CfWord::bare()));
    assert_eq!(
        context.ledger.borrow().role_notes_to(chief).unwrap(),
        [written.id]
    );
    context.close();
}

/// The most rules for the staff the ledger takes, in what costs a text that says
/// them the most: every byte the ledger allows and every line, the lines in
/// characters of three bytes (a letter of a script outside the Latin ones) and
/// the rest in one-byte ones, so that the bytes and lines of a note are as many
/// as any rules can make them.
fn the_most_rules() -> String {
    let lines = STAFF_RULES_MAX_LINES;
    let each = (STAFF_RULES_MAX_BYTES - (lines - 1)) / lines;
    let line = format!("{}{}", "日".repeat(each / 3), "a".repeat(each % 3));
    let mut rules = vec![line; lines].join("\n");
    rules += &"a".repeat(STAFF_RULES_MAX_BYTES - rules.len());
    assert_eq!(rules.len(), STAFF_RULES_MAX_BYTES);
    assert_eq!(rules.split('\n').count(), STAFF_RULES_MAX_LINES);
    assert_eq!(
        cf_ledger::parse_staff_rules(Some(&Value::from(rules.clone()))).ok(),
        Some(Some(rules.clone())),
        "the ledger takes them"
    );
    rules
}

/// The real text of a chief with `rows` staff rows who each hold two roles, with
/// specs on or off and the project's rules for the staff, if it has some: the paths
/// of a Mac, as long as the folder its skill's copy is kept in makes its path.
fn a_chiefs_role_with(rows: usize, specs_on: bool, rules: Option<&str>) -> String {
    a_chiefs_role_for(&a_mac(), rows, specs_on, rules)
}

/// A window's `cf` on a Mac, by the path of the app's.
fn a_mac() -> CfWord {
    CfWord::new(
        "/Applications/ConsensFlow.app/Contents/Resources/bin/cf",
        Shell::Posix,
    )
}

/// A window's `cf` on Windows, as PowerShell, which Muse's and Codex's shell there is, runs it.
fn a_windows() -> CfWord {
    CfWord::new(
        r"C:\Users\someone\AppData\Local\ConsensFlow\cli\bin\cf.exe",
        Shell::PowerShell,
    )
}

/// [`a_chiefs_role_with`] for a window whose `cf` is `cf`.
fn a_chiefs_role_for(cf: &CfWord, rows: usize, specs_on: bool, rules: Option<&str>) -> String {
    let staff: Vec<StaffRow> = (0..rows)
        .map(|at| StaffRow {
            // A member is named by its model and effort (T-1263), a handle of about twenty
            // characters, and the table says the model once, in the name.
            name: format!("gpt-6-1-sol-xhigh-{at}"),
            model: None,
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
        cf,
        specs_on.then_some(&specs),
    )
    .map(|text| with_staff_rules(text, "chief", rules))
    .unwrap()
}

/// What `cf inbox read` prints of part `part` (the first is 1) of `note` in the window whose
/// `cf` is `cf`, while its paste is on its way (the longest line a role's note is printed
/// under), and how many parts it has.
fn printed_by_cf(note: &MessageView, part: usize, cf: &CfWord) -> (String, usize) {
    let line = format!("m-{} [delivering] note from ConsensFlow", note.id);
    let (text, of) =
        printed_part(&line, &cf.in_text(&note.body), part, cf.as_str(), note.id).unwrap();
    (format!("{text}\n"), of)
}

/// Whether `cf inbox read` prints `note` in the window whose `cf` is `cf` in one piece read
/// whole, as a role was before it was printed in parts.
fn read_in_one(note: &MessageView, cf: &CfWord) -> bool {
    let (printed, of) = printed_by_cf(note, 1, cf);
    of == 1 && role_read_whole(&printed)
}

#[test]
fn own_hands_role_is_printed_in_one_piece_when_a_session_resumes() {
    let staff: Vec<_> = (0..5)
        .map(|at| StaffRow {
            name: format!("gpt-6-1-sol-xhigh-{at}"),
            model: None,
            roles: vec!["worker".to_owned(), "reviewer".to_owned()],
            work_tier: Some(WorkTier::Standard),
        })
        .collect();
    let specs = ProjectSpecs {
        variant: Variant::TddHtml,
        folder: "docs/specs",
        skill: "/Users/someone/.consensflow/integrations/specmint/codex/tdd-html-0123456789abcdef0123456789abcdef01234567/.claude/skills/specmint-tdd-html/SKILL.md",
    };
    for cf in [a_mac(), a_windows()] {
        for specs_on in [false, true] {
            let role = role_instructions_for(
                &Env::default(),
                "chief",
                &staff,
                &cf,
                specs_on.then_some(&specs),
                true,
            )
            .unwrap();
            let ordinary = role_instructions(
                &Env::default(),
                "chief",
                &staff,
                &cf,
                specs_on.then_some(&specs),
            )
            .unwrap();
            assert!(role.len() < ordinary.len(), "{cf}, specs on: {specs_on}");
            assert!(role.contains("project's own folder"));
            let replacement = note(None, "note", body_of(&role, &cf));
            assert!(read_in_one(&replacement, &cf), "{cf}, specs on: {specs_on}");
        }
    }
}

/// The real text of a chief with specs on is told whatever its staff and its rules: it is
/// longer than a message goes in whole at since the chief's rules for smaller contexts
/// (T-1009), so its window is given the opening, which says to read the rest now, and the
/// command that prints it; and that command prints it in one piece read whole, or, past
/// that (T-1513, review T-1510: the most rules and a staff of 6 since the ban on a task for
/// a member by name came back, 8 before it), in parts, each read whole, which the opening
/// says how many of. A Mac's and a Windows window's (PowerShell's) both.
#[test]
fn the_real_text_of_a_chief_with_specs_any_staff_and_the_most_rules_is_told_whole_or_read_in_every_part(
) {
    let most = the_most_rules();
    for cf in [a_mac(), a_windows()] {
        for (rows, rules) in [
            (25, None),
            (300, None),
            (5, Some(most.as_str())),
            (6, Some(most.as_str())),
            (8, Some(most.as_str())),
            (12, Some(most.as_str())),
            (20, Some(most.as_str())),
            (143, Some(most.as_str())),
            (300, Some(most.as_str())),
        ] {
            told_whole_or_told_and_read(&a_chiefs_role_for(&cf, rows, true, rules), &cf);
        }
    }
}

/// A chief's `text`, said again in a replacement in the window whose `cf` is `cf`, goes in
/// whole, or in part with the command that prints the rest, which prints it in one piece or
/// in the parts its opening says, each of which its window reads whole.
fn told_whole_or_told_and_read(text: &str, cf: &CfWord) {
    let text = text.to_owned();
    let replacement = note(None, "note", body_of(&text, cf));
    let said = delivery_text(&replacement, &[], cf);
    if goes_whole(&replacement, &[], cf) {
        assert!(said.contains(&text), "the role is cut");
        return;
    }
    assert!(
        said.ends_with(&format!("read all of it with: {cf} inbox read m-4242)")),
        "the command that prints the rest: {}",
        &said[said.len() - 200..]
    );
    assert!(replacement.body.ends_with(&text));
    let (_, of) = printed_by_cf(&replacement, 1, cf);
    let opening = if of == 1 {
        OPENING_IN_PART.to_owned()
    } else {
        Telling::Replaces.in_parts(of)
    };
    assert!(
        said.starts_with(&format!(
            "[ConsensFlow m-4242 · note from ConsensFlow]\n{opening}\n\n"
        )),
        "it says to read the rest now, and in how many parts: {}",
        &said[..900]
    );
    let mut words = String::new();
    for part in 1..=of {
        let (printed, _) = printed_by_cf(&replacement, part, cf);
        assert!(
            role_read_whole(&printed),
            "part {part} of {of} is read whole: {} bytes, {} lines",
            printed.len(),
            printed.matches('\n').count()
        );
        words += cf_base::role_read::role_parts(&cf.in_text(&replacement.body))[part - 1].words;
    }
    assert_eq!(
        words,
        cf.in_text(&replacement.body),
        "the parts are the note"
    );
}

/// The staff a role holds whole in a replacement, the staff `cf` prints it for in one piece
/// read whole, which the docs of this module say, and the parts it is printed in past that.
#[test]
fn the_staff_a_replacement_holds_whole_and_read_in_one_piece_is_as_large_as_the_docs_say() {
    let cf = a_mac();
    let rows_while = |specs_on, rules: Option<&str>, holds: &dyn Fn(&MessageView) -> bool| {
        (1..)
            .take_while(|&rows| {
                holds(&note(
                    None,
                    "note",
                    body_of(&a_chiefs_role_with(rows, specs_on, rules), &cf),
                ))
            })
            .last()
            .unwrap_or(0)
    };
    let whole = |note: &MessageView| goes_whole(note, &[], &cf);
    let one = |note: &MessageView| read_in_one(note, &cf);
    let most = the_most_rules();
    assert_eq!(
        rows_while(true, None, &whole),
        0,
        "docs: never whole with specs on"
    );
    assert_eq!(
        rows_while(false, None, &whole),
        0,
        "docs: never whole without specs either"
    );
    assert_eq!(
        rows_while(true, None, &one),
        141,
        "docs: one piece to 141 rows with specs on"
    );
    assert_eq!(
        rows_while(false, None, &one),
        166,
        "docs: one piece to 166 rows without"
    );
    assert_eq!(
        rows_while(true, Some(&most), &one),
        5,
        "docs: one piece to 5 rows with specs on and the most rules"
    );
    assert_eq!(
        rows_while(false, Some(&most), &one),
        30,
        "docs: one piece to 30 rows without specs and the most rules"
    );
    let parts = |rows| {
        printed_by_cf(
            &note(
                None,
                "note",
                body_of(&a_chiefs_role_with(rows, true, Some(&most)), &cf),
            ),
            1,
            &cf,
        )
        .1
    };
    assert_eq!(
        [parts(6), parts(143), parts(300)],
        [2, 2, 3],
        "docs: past one piece, 2 parts to 143 rows, 3 at 300"
    );
}

/// The real text of every member with the most rules for the staff the ledger
/// takes, said again in a replacement or in the note that greets a session that
/// began without it (a Muse `/clear`), goes into its window whole, in one message:
/// a member's role is never told in part for its rules.
#[test]
fn a_members_real_text_with_the_most_rules_for_the_staff_goes_in_whole() {
    // As many units as bytes: the most units any rules can have.
    let most = "a".repeat(STAFF_RULES_MAX_BYTES);
    for role in ["worker", "advisor", "reviewer", "tester", "designer"] {
        let text = role_instructions(
            &Env::default(),
            role,
            &[],
            &CfWord::new(
                "/Applications/ConsensFlow.app/Contents/Resources/bin/cf",
                Shell::Posix,
            ),
            None,
        )
        .map(|text| with_staff_rules(text, role, Some(&most)))
        .unwrap();
        for (whole, telling) in [
            (OPENING, Telling::Replaces),
            (BEGAN_WITHOUT, Telling::BeganWithout),
        ] {
            let said = note_of(telling, &text, &CfWord::bare());
            assert!(said.starts_with(whole), "{role}: told whole");
            assert!(
                goes_whole(&note(None, "note", said.clone()), &[], &CfWord::bare()),
                "{role}: {} units",
                cf_base::text::utf16_len(&said)
            );
        }
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
        synthetic: false,
        compaction: false,
        detail: None,
    }
}

/// What the final answer of a record, as `answers_to_the_task` says it with
/// m-4242 the one replacement the ledger knows, says.
fn answers(items: &[Item]) -> Vec<String> {
    final_answer(items, &[4242])
}

/// The words of the final answer of `items`, `replacements` the ledger knows.
fn final_answer(items: &[Item], replacements: &[i64]) -> Vec<String> {
    answers_to_the_task(items, replacements).map_or_else(Vec::new, |answers| {
        answers
            .final_run
            .iter()
            .map(|item| item.text.to_string())
            .collect()
    })
}

#[test]
fn the_summary_of_a_compaction_after_the_answer_leaves_the_answer_whole() {
    // Astra's review of round 1: a report, then the summary Claude Code writes as it
    // compacts the conversation at rest, then the look that takes the result.
    let user = |text: &str| said_by(Role::User, text, true);
    let assistant = |text: &str| said_by(Role::Assistant, text, true);
    let summary = Item {
        compaction: true,
        ..user("This session is being continued from a previous conversation.")
    };
    let items = [
        user("Review the parser"),
        assistant("Reading it"),
        said_by(Role::Tool, "parser.rs", true),
        assistant("The parser is sound."),
        summary.clone(),
    ];
    let answered = answers_to_the_task(&items, &[]).unwrap();
    assert_eq!(
        answered.last.map(|last| &*last.text),
        Some("The parser is sound.")
    );
    assert_eq!(answers(&items), ["The parser is sound."]);
    // Nor does one between a replacement of the role and its word end the role's turn.
    let replacement = delivery_text(
        &note(None, "note", body_of("a role", &CfWord::bare())),
        &[],
        &CfWord::bare(),
    );
    let mut items = vec![assistant("Parser done"), user(&replacement), summary];
    items.push(assistant("Understood."));
    assert_eq!(answers(&items), ["Parser done"]);
}

#[test]
fn a_turn_of_a_record_answers_a_replacement_by_the_id_in_its_header_and_no_words_it_quotes() {
    let user = |text: &str| said_by(Role::User, text, true);
    let assistant = |text: &str| said_by(Role::Assistant, text, true);
    let replacement = delivery_text(
        &note(None, "note", body_of("a role", &CfWord::bare())),
        &[],
        &CfWord::bare(),
    );
    // The opening, and the whole text of the replacement, quoted in a note about
    // the task (m-7), from someone: the worker's word answers the task.
    let mut quoting = note(Some("chief"), "note", format!("Review this: {replacement}"));
    quoting.id = 7;
    quoting.task_number = Some(1);
    let quoted = delivery_text(&quoting, &[], &CfWord::bare());
    assert!(quoted.contains(OPENING), "{quoted}");
    assert_eq!(
        answers(&[user(&quoted), assistant("Parser done")]),
        ["Parser done"]
    );
    // The same id, as the ledger says no replacement it knows: no turn of the role.
    assert_eq!(
        final_answer(&[user(&replacement), assistant("Understood.")], &[]),
        ["Understood."]
    );
}

#[test]
fn the_words_a_window_gives_a_replacement_of_its_role_are_no_answer_to_its_task() {
    let replacement = delivery_text(
        &note(None, "note", body_of("a role", &CfWord::bare())),
        &[],
        &CfWord::bare(),
    );
    let user = |text: &str| said_by(Role::User, text, true);
    let assistant = |text: &str| said_by(Role::Assistant, text, true);

    assert_eq!(answers(&[]), Vec::<String>::new());
    assert_eq!(answers(&[assistant("Parser done")]), ["Parser done"]);
    // The word that follows the role is the window's word of its role, and none of the task's.
    assert_eq!(
        answers(&[user(&replacement), assistant("Understood.")]),
        Vec::<String>::new()
    );
    // What it said of the task before the role came is the task's, and is kept, whole: two
    // items, since a final run with no words would take back the last of them (T-262).
    assert_eq!(
        answers(&[
            assistant("Parser done"),
            assistant("All tests pass"),
            user(&replacement),
            assistant("Understood.")
        ]),
        ["Parser done", "All tests pass"]
    );
    // And what it says to the next message is the task's again, whoever the message is from.
    assert_eq!(
        answers(&[
            user(&replacement),
            assistant("Understood."),
            user("ConsensFlow stopped while you were working on T-1."),
            assistant("Parser done"),
        ]),
        ["Parser done"]
    );
    // The final answer is what comes after the tool's output: what was said before it
    // is the turn's progress.
    assert_eq!(
        answers(&[
            user(&replacement),
            assistant("Understood."),
            user("Go on"),
            assistant("First"),
            said_by(Role::Tool, "output", true),
            assistant("Second"),
        ]),
        ["Second"]
    );
}

#[test]
fn the_final_answer_is_what_follows_the_last_tool_call_or_message_and_what_is_not_the_roles() {
    let user = |text: &str| said_by(Role::User, text, true);
    let assistant = |text: &str| said_by(Role::Assistant, text, true);
    let tool = |text: &str| said_by(Role::Tool, text, true);
    let replacement = delivery_text(
        &note(None, "note", body_of("a role", &CfWord::bare())),
        &[],
        &CfWord::bare(),
    );

    // Several words after the last tool call are all of it, in order.
    assert_eq!(
        answers(&[
            user("Write the report"),
            assistant("Still reviewing"),
            tool("diff"),
            assistant("Now the tests"),
            tool("tests"),
            assistant("The report, first"),
            assistant("and last"),
        ]),
        ["The report, first", "and last"]
    );
    // A turn that ended with a word of progress and went on when its agent was woken (a
    // message of the harness's own begins the turn): the first turn's last word is no part
    // of the answer.
    assert_eq!(
        answers(&[
            user("Run the mutation tests"),
            assistant("Running them"),
            tool("Command running in background with ID: b1"),
            assistant("Still going; I'll write it up when it ends"),
            user("<task-notification>b1 completed</task-notification>"),
            assistant("Reading the output"),
            tool("33 killed"),
            assistant("All 33 mutants are killed."),
        ]),
        ["All 33 mutants are killed."]
    );
    // What the harness put in the conversation itself is neither: it cuts nothing and
    // is no word of the member's.
    assert_eq!(
        answers(&[
            user("Go"),
            assistant("Looking"),
            tool("out"),
            assistant("Done"),
            said_by(Role::Custom, "a hook's context", true),
        ]),
        ["Done"]
    );
    // A tool's output inside the turn of a replacement of the role cuts nothing of the
    // task's answer, and its words are not the answer.
    assert_eq!(
        answers(&[
            user("Go"),
            assistant("Parser done"),
            user(&replacement),
            assistant("Understood."),
            tool("out"),
            assistant("Understood, again"),
        ]),
        ["Parser done"]
    );
    // No word, or a last word that is none: nothing answers.
    assert!(answers_to_the_task(&[user("Go"), tool("out")], &[]).is_none());
    let silent = [user("Go"), assistant("Looking"), tool("out"), assistant("")];
    let answered = answers_to_the_task(&silent, &[]).expect("a last word");
    assert_eq!(answered.last.map(|last| last.text.as_ref()), Some(""));
    assert_eq!(answers(&silent), ["Looking"], "never empty (T-262)");
}

/// The texts of `items`.
fn texts(items: &[&Item]) -> Vec<String> {
    items.iter().map(|item| item.text.to_string()).collect()
}

#[test]
fn a_turn_with_no_words_after_its_last_tool_call_answers_with_the_last_words_it_wrote_before_it() {
    // The chief's decision on T-262: a result is never empty. What follows the last tool call
    // (`after_the_last_tool`) is kept apart, for what decides on it (a tell's note, T-878);
    // the answer a result and Finish take is the last item with words before it.
    let user = |text: &str| said_by(Role::User, text, true);
    let assistant = |text: &str| said_by(Role::Assistant, text, true);
    let step = |text: &str| said_by(Role::Assistant, text, false);
    let tool = |text: &str| said_by(Role::Tool, text, true);
    let silent = [user("Go"), step("Looking"), tool("out"), assistant("")];
    let answered = answers_to_the_task(&silent, &[]).expect("a last word");
    assert_eq!(texts(&answered.after_the_last_tool), [""]);
    assert_eq!(texts(&answered.final_run), ["Looking"]);
    assert_eq!(
        answered.complete_run.as_deref().map(|run| texts(run)),
        Some(vec!["Looking".to_owned()]),
        "Finish takes the same"
    );
    // A step that said nothing, or only space, is no words: the last that said some is.
    let quiet_step = [
        user("Go"),
        step("Still reviewing the diff"),
        tool("a"),
        step(" \n "),
        tool("b"),
        assistant(""),
    ];
    let answered = answers_to_the_task(&quiet_step, &[]).unwrap();
    assert_eq!(texts(&answered.final_run), ["Still reviewing the diff"]);
    // The harness's own words are none of the member's: Codex's notes (`commentary`), and
    // Claude Code's close of a cut turn or Pi's step of a run that never began (`synthetic`).
    let noted = Item {
        commentary: true,
        ..step("Checking the counts now.")
    };
    let its_own = Item {
        synthetic: true,
        ..step("[Request interrupted by user]")
    };
    for harness in [noted, its_own] {
        let items = [
            user("Go"),
            step("Read the diff"),
            tool("a"),
            harness,
            tool("b"),
            assistant(""),
        ];
        let answered = answers_to_the_task(&items, &[]).unwrap();
        assert_eq!(texts(&answered.final_run), ["Read the diff"]);
    }
    // Words after the last tool call are the answer, whatever came before.
    let worded = [user("Go"), step("Looking"), tool("out"), assistant("Done")];
    let answered = answers_to_the_task(&worded, &[]).unwrap();
    assert_eq!(texts(&answered.final_run), ["Done"]);
    // Nothing from before the latest message to the member: a woken turn's own words only.
    let earlier = [
        assistant("Looking"),
        user("Go on"),
        tool("out"),
        assistant(" "),
    ];
    let answered = answers_to_the_task(&earlier, &[]).unwrap();
    assert_eq!(texts(&answered.final_run), [" "]);
    assert_eq!(
        answered.complete_run.as_deref().map(|run| texts(run)),
        Some(vec![" ".to_owned()])
    );
    // Nor from a turn on a replacement of the role, which is no answer to the task.
    let replacement = delivery_text(
        &note(None, "note", body_of("a role", &CfWord::bare())),
        &[],
        &CfWord::bare(),
    );
    let of_the_role = [
        user("Go"),
        step("Parser done"),
        tool("out"),
        user(&replacement),
        assistant("Understood."),
    ];
    let answered = answers_to_the_task(&of_the_role, &[4242]).unwrap();
    assert!(answered.after_the_last_tool.is_empty());
    assert_eq!(texts(&answered.final_run), ["Parser done"]);
    // Finish takes the answer as it stood at the last complete item: words written after it,
    // in a step still at work, are not yet its.
    let at_work = [
        user("Go"),
        step("Looking"),
        tool("out"),
        assistant(""),
        user("<task-notification>b1 completed</task-notification>"),
        step("Reading the log"),
    ];
    let answered = answers_to_the_task(&at_work, &[]).unwrap();
    assert_eq!(
        answered.complete_run.as_deref().map(|run| texts(run)),
        Some(vec!["Looking".to_owned()])
    );
}

/// The words of the reply of `items`: what the member wrote after its message and before its
/// first tool call, where a tool call followed (`Answers::reply`).
fn reply(items: &[Item], replacements: &[i64]) -> Vec<String> {
    answers_to_the_task(items, replacements).map_or_else(Vec::new, |answers| {
        answers
            .reply
            .into_iter()
            .map(|item| item.text.to_string())
            // A call that says nothing is an item of the record, with no words.
            .filter(|text| !text.trim().is_empty())
            .collect()
    })
}

#[test]
fn what_a_member_writes_before_its_first_tool_call_is_its_reply_and_what_it_writes_between_its_tools_is_not(
) {
    // Devin's turn on a tell (T-1151), and Claude Code's, Codex's and Muse's: text, a tool,
    // another call, a tool, text.
    let user = |text: &str| said_by(Role::User, text, true);
    let assistant = |text: &str, complete| said_by(Role::Assistant, text, complete);
    let items = [
        user("Go"),
        assistant("Stopped", false),
        said_by(Role::Tool, "No output produced after 10.0 seconds", true),
        assistant("Reading on", false),
        said_by(Role::Tool, "Exited with code 0", true),
        assistant("The last words", true),
    ];
    assert_eq!(reply(&items, &[]), ["Stopped"]);
    assert_eq!(answers(&items), ["The last words"]);
    // Several items before the first tool are one reply, in order.
    let items = [
        user("Go"),
        assistant("Stopped", false),
        assistant("and ready", false),
        said_by(Role::Tool, "out", true),
        assistant("The last words", true),
    ];
    assert_eq!(reply(&items, &[]), ["Stopped", "and ready"]);
}

#[test]
fn a_turn_with_no_tool_call_or_with_one_before_its_words_has_no_reply() {
    let user = |text: &str| said_by(Role::User, text, true);
    let assistant = |text: &str| said_by(Role::Assistant, text, true);
    // No tool call: the one run is the final answer, and the reply is nothing besides it.
    assert!(reply(&[user("Go"), assistant("Done")], &[]).is_empty());
    // A tool call first (an empty call, a result), then words.
    assert!(reply(
        &[
            user("Go"),
            said_by(Role::Assistant, "", false),
            said_by(Role::Tool, "out", true),
            assistant("Done"),
        ],
        &[]
    )
    .is_empty());
    // No answer at all.
    assert!(answers_to_the_task(&[user("Go"), said_by(Role::Tool, "out", true)], &[]).is_none());
}

#[test]
fn only_the_turn_after_the_latest_message_has_a_reply() {
    let user = |text: &str| said_by(Role::User, text, true);
    let assistant = |text: &str| said_by(Role::Assistant, text, true);
    let tool = |text: &str| said_by(Role::Tool, text, true);
    let items = [
        user("Go"),
        assistant("Looking at the first"),
        tool("out"),
        assistant("First done"),
        user("And now this"),
        assistant("Stopped"),
        tool("out"),
        assistant("Second done"),
    ];
    assert_eq!(reply(&items, &[]), ["Stopped"]);
    // A message to the member that is a role begins a turn of its own: its words are the role's,
    // and neither end nor begin the reply.
    let replacement = delivery_text(
        &note(None, "note", body_of("a role", &CfWord::bare())),
        &[],
        &CfWord::bare(),
    );
    let items = [
        user("Go"),
        assistant("Stopped"),
        tool("out"),
        user(&replacement),
        assistant("Understood."),
        tool("out"),
        assistant("The last words"),
    ];
    assert_eq!(reply(&items, &[4242]), ["Stopped"]);
}

#[test]
fn what_a_harness_wrote_itself_is_no_reply_but_its_progress_marks_are_kept() {
    let user = |text: &str| said_by(Role::User, text, true);
    // Claude Code's close of a turn it found cut: no model behind it.
    let closed = Item {
        synthetic: true,
        commentary: true,
        ..said_by(Role::Assistant, "No response requested.", true)
    };
    // Codex marks the text it writes beside a tool call as progress, where its phase says so.
    let progress = Item {
        commentary: true,
        ..said_by(Role::Assistant, "Stopped", false)
    };
    let items = [
        user("Go"),
        closed,
        progress,
        said_by(Role::Tool, "out", true),
        said_by(Role::Assistant, "The last words", true),
    ];
    assert_eq!(reply(&items, &[]), ["Stopped"]);
}

#[test]
fn a_message_to_the_member_after_its_answer_leaves_the_turn_unanswered_and_its_last_complete_answer_kept(
) {
    // T-442's shape: the member answered with its job running, and the job's end woke a
    // turn that has written nothing yet. That turn's answer is still to come, and the
    // answer before it is no answer to it; it is still the last complete one after the
    // task's latest message, which Finish takes.
    let user = |text: &str| said_by(Role::User, text, true);
    let items = [
        said_by(Role::Assistant, "Watching the run", true),
        user("<task-notification>the run ended</task-notification>"),
    ];
    let answered = answers_to_the_task(&items, &[]).expect("a complete answer");
    assert!(
        answered.last.is_none(),
        "nothing answers the notification yet"
    );
    assert!(answered.final_run.is_empty());
    let complete: Vec<&str> = answered
        .complete_run
        .expect("the last complete answer")
        .iter()
        .map(|item| item.text.as_ref())
        .collect();
    assert_eq!(complete, ["Watching the run"]);
    // With nothing complete before it, there is no answer at all.
    let items = [
        said_by(Role::Assistant, "Looking", false),
        user("<task-notification>the run ended</task-notification>"),
    ];
    assert!(answers_to_the_task(&items, &[]).is_none());
}

#[test]
fn a_displaced_message_that_rides_in_the_paste_of_another_is_not_begun_and_its_launch_is_not_refused(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let brief = context
        .give(project.id, "zeus", "Write the parser")
        .message
        .expect("its brief");
    // The task is paused and resumed before the brief went in: the words that
    // resume it carry the brief in their paste.
    context
        .ledger
        .borrow_mut()
        .pause_task(project.id, 1, None, Some("a test pauses it"))
        .unwrap();
    context
        .ledger
        .borrow_mut()
        .resume_task(project.id, 1, Some("chief"), "Go on")
        .unwrap();
    let displaced = Delivering {
        message: brief.id,
        since: 0,
        rested_since: None,
        launch: true,
        chief: false,
        queued: false,
        free_since: None,
        turned: false,
        entered_again: false,
        carried: Vec::new(),
        session: None,
        owed: None,
        late: false,
        work: crate::at_work::OnWork::default(),
        not_ready: None,
    };
    assert!(
        context
            .ledger
            .borrow_mut()
            .begin_delivery(brief.id)
            .is_err(),
        "the brief rides in another message's paste now"
    );
    context.dispatcher.begin_displaced(&displaced).unwrap();
    assert_eq!(
        context.message(brief.id).state,
        "queued",
        "it goes with its carrier, and there is nothing of it to fail"
    );
    context.close();
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
    let said = context
        .ledger
        .borrow_mut()
        .note_role(
            project.id,
            "chief",
            &body_of("the role it is now", &CfWord::bare()),
            "the role it is now",
        )
        .unwrap();
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

/// T-931 P1-1: the note that greets a session no hook greeted is no launch's prelude: a
/// chief whose window comes up after it was told has its count of failed starts cleared,
/// as after any launch that came up.
#[test]
fn a_chief_that_comes_up_after_its_greeting_was_told_has_its_failed_starts_cleared() {
    let context = Context::new();
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
    let record = context.dispatcher.record_of(chief);
    record.window.borrow_mut().relaunch = Some(crate::windows::Relaunch {
        failures: 1,
        at: 0,
        reopens: false,
    });
    context.dispatcher.came_up(&record);
    assert!(record.window.borrow().relaunch.is_none());
}

/// The most the staff's rules cost a hook's JSON answer, in what the ledger takes: its bytes
/// and lines at the most (8,000 in 150), every one a character a JSON string writes in two
/// bytes (a quote, a backslash, a line break), so 16,000 bytes of answer where the most
/// UTF-8 rules ([`the_most_rules`]) are 8,000 and a letter is one. T-1246's review found it:
/// the real worst case of a role's size in a hook is these, not the most bytes.
fn the_most_escaped_rules() -> String {
    let lines = STAFF_RULES_MAX_LINES;
    let each = (STAFF_RULES_MAX_BYTES - (lines - 1)) / lines;
    let line = "\"\\".repeat(each / 2) + &"\"".repeat(each % 2);
    let mut rules = vec![line; lines].join("\n");
    rules += &"\"".repeat(STAFF_RULES_MAX_BYTES - rules.len());
    assert_eq!(rules.len(), STAFF_RULES_MAX_BYTES);
    assert_eq!(rules.split('\n').count(), STAFF_RULES_MAX_LINES);
    assert_eq!(
        cf_ledger::parse_staff_rules(Some(&Value::from(rules.clone()))).ok(),
        Some(Some(rules.clone())),
        "the ledger takes them"
    );
    assert_eq!(
        cf_harness::muse::hook_output(&rules).len() - cf_harness::muse::hook_output("").len(),
        2 * STAFF_RULES_MAX_BYTES,
        "every byte two in a hook's answer"
    );
    rules
}

/// What Muse keeps of the blocks of a role that is given in more than one, put together: each
/// block with the white space at both ends cut off (Muse's cut), then the frame's two lines
/// taken off. A role of one block is that block, as Muse cuts it.
fn kept_by_muse(blocks: &[String]) -> String {
    if let [only] = blocks {
        return only.trim().to_owned();
    }
    blocks
        .iter()
        .map(|block| {
            let block = block.trim();
            let (head, rest) = block.split_once('\n').expect("a head line");
            let (inner, tail) = rest.rsplit_once('\n').expect("a tail line");
            assert!(head.starts_with(cf_harness::muse::MARK), "{head}");
            assert!(tail.starts_with("<<end of consensflow-role "), "{tail}");
            inner.to_owned()
        })
        .collect()
}

/// T-1246: Muse takes at most 16,384 bytes of a hook's output, and a Muse window has its role
/// by hooks alone, so a role is given in parts, a hook each (`cf_harness::muse`), each part's
/// hook under that limit, and what Muse keeps of the parts, put together, the role to the
/// byte. A chief's role is longer than one hook says since each command of it named the
/// window's `cf` in full (T-1049: 17,455 bytes with no staff at all), so no chief's is one
/// part; a member's is one with the most rules in the most bytes, and two with the most in
/// JSON: the rules the ledger takes can make 16,000 bytes of answer of 8,000. The most of
/// parts is the hooks Muse was measured to take at one start, and the case keeps a role at
/// half of it. These are the numbers: the spare bytes are in the message of each case that
/// fails, and a role that grows to a part more fails here, so that it is looked at.
#[test]
fn the_largest_roles_are_given_to_a_muse_window_in_the_parts_its_hooks_say_and_every_part_fits_a_hook(
) {
    use cf_harness::muse::{blocks, hook_output, parts, MOST_PARTS, OUTPUT_LIMIT};
    let (utf8, escaped) = (the_most_rules(), the_most_escaped_rules());
    let on = |name: &str, role: &str, parts_wanted: usize| {
        let cut = parts(role);
        assert_eq!(cut.concat(), role, "{name}: the parts are the role");
        let framed = blocks(role);
        let outputs: Vec<usize> = framed
            .iter()
            .map(|block| hook_output(block).len())
            .collect();
        let spare = outputs.last().map(|last| OUTPUT_LIMIT - last);
        assert_eq!(
            framed.len(),
            parts_wanted,
            "{name}: {} bytes ({} in one hook) in parts of {outputs:?} bytes of hook output; the last has {spare:?} spare",
            role.len(),
            hook_output(role).len()
        );
        assert!(
            framed.len() <= MOST_PARTS / 2,
            "{name}: past half the hooks Muse takes"
        );
        assert!(
            outputs.iter().all(|&bytes| bytes <= OUTPUT_LIMIT),
            "{name}: a part past Muse's limit: {outputs:?}"
        );
        let has = kept_by_muse(&framed);
        assert!(
            has == role || (framed.len() == 1 && has == role.trim()),
            "{name}: what Muse keeps is not the role"
        );
    };
    for (cf, shape) in [(a_mac(), "a Mac"), (a_windows(), "Windows")] {
        let chief = |rows, specs, rules: Option<&str>| a_chiefs_role_for(&cf, rows, specs, rules);
        let member = |role: &str, rules: &str| {
            role_instructions(&Env::default(), role, &[], &cf, None)
                .map(|text| with_staff_rules(text, role, Some(rules)))
                .unwrap()
        };
        on(
            &format!("a chief with no staff on {shape}"),
            &chief(0, false, None),
            2,
        );
        on(
            &format!("the largest chief in bytes (25 staff, specs, the most rules) on {shape}"),
            &chief(25, true, Some(&utf8)),
            2,
        );
        on(
            &format!(
                "the largest chief in a hook (25 staff, specs, the most escaped rules) on {shape}"
            ),
            &chief(25, true, Some(&escaped)),
            3,
        );
        for role in ["worker", "advisor", "reviewer", "tester", "designer"] {
            on(
                &format!("the {role} with the most rules on {shape}"),
                &member(role, &utf8),
                1,
            );
            on(
                &format!("the {role} with the most escaped rules on {shape}"),
                &member(role, &escaped),
                2,
            );
        }
    }
}

/// T-1435: where the parts of the largest chief's role begin in a Muse window. Muse is given a
/// role in parts, a hook each, and a part ends where a paragraph or a heading begins, not
/// between a line and the list it introduces (`cf_harness::muse::parts`). These are the first
/// lines of the parts after the first, per case: a change to the chief's text that moves them
/// fails here, so that where the role is cut is looked at. A worker's role is one part.
#[test]
fn the_parts_of_a_muse_chiefs_role_begin_at_a_paragraph_or_a_heading_and_never_in_a_list() {
    use cf_harness::muse::parts;
    let (utf8, escaped) = (the_most_rules(), the_most_escaped_rules());
    // The first lines of the first `checked` parts after the first (the rules a project gives
    // its staff are cut where the room ends, in their lines, which have no blank line).
    let begin = |role: &str, checked: usize| -> Vec<String> {
        let cut = parts(role);
        assert_eq!(cut.concat(), role);
        let mut before = String::new();
        let mut starts = Vec::new();
        for part in &cut {
            if !before.is_empty() && starts.len() < checked {
                let last = before.trim_end().lines().last().unwrap_or("");
                assert!(before.ends_with("\n\n"), "a part ends at a blank line");
                assert!(
                    !last.ends_with(':') && !last.starts_with('#'),
                    "{last:?} introduces what is cut off"
                );
                let first = part.lines().next().unwrap_or("");
                assert!(
                    !first.starts_with(|c: char| c.is_whitespace() || "-*+|>`".contains(c)),
                    "{first:?} goes on with what is before it"
                );
                starts.push(first.chars().take(60).collect());
            }
            before += part;
        }
        starts
    };
    // A case: its name, the window's `cf`, the staff's rows, specs on or off, the rules, and the
    // lines the part after the first begins with.
    type Case<'a> = (&'a str, CfWord, usize, bool, Option<&'a str>, &'a [&'a str]);
    // The image designer's paragraph in the chief's section on roles (T-1480).
    const IMAGES: &str = "An image comes from the image designer (`--design`). Give it";
    let cases: [Case; 4] = [
        ("a Mac, no staff", a_mac(), 0, false, None, &[IMAGES]),
        (
            "a Mac, the largest chief",
            a_mac(),
            25,
            true,
            Some(&utf8),
            &[IMAGES],
        ),
        (
            "Windows, the largest chief",
            a_windows(),
            25,
            true,
            Some(&utf8),
            &["Where the staff has an integrator, it lands accepted work; a"],
        ),
        (
            "a Mac, the most escaped rules",
            a_mac(),
            25,
            true,
            Some(&escaped),
            &[IMAGES],
        ),
    ];
    for (name, cf, rows, specs, rules, expected) in cases {
        let role = a_chiefs_role_for(&cf, rows, specs, rules);
        assert_eq!(begin(&role, 1), expected, "{name}");
    }
    let worker = role_instructions(&Env::default(), "worker", &[], &a_mac(), None).unwrap();
    assert_eq!(
        parts(&with_staff_rules(worker, "worker", Some(&utf8))).len(),
        1
    );
}
