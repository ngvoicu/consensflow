use cf_base::env::Env;
use cf_catalog::WorkTier;
use cf_proto::specs::Variant;
use serde_json::Value;

use super::*;
use crate::delivery_text::delivery_text;
use crate::roles::{role_instructions, ProjectSpecs, StaffRow};

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

/// The real text of a chief with specs on, as long as the folder its skill's copy
/// is kept in makes its path and with a staff of twenty-five who each hold two
/// roles, is still under what a message goes in whole at: it is the role itself the
/// replacement says, not its opening and a command to read the rest of it.
#[test]
fn the_real_text_of_a_chief_with_specs_and_a_large_staff_goes_in_whole_as_a_replacement() {
    let staff: Vec<StaffRow> = (0..25)
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
    let text = role_instructions(
        &Env::default(),
        "chief",
        &staff,
        Some("/Applications/ConsensFlow.app/Contents/Resources/bin/cf"),
        Some(&specs),
    )
    .unwrap();
    let said = delivery_text(&note(None, "note", body_of(&text)), &[]);
    assert!(said.contains(&text), "the role is cut");
    assert!(
        !said.contains("read all of it with"),
        "a command stands in for its end"
    );
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

/// What the docs of this module say of the command line of a Windows launch that
/// says a changed role (the role twice, as developer instructions and as the first
/// message), held to the real text: with a staff of twenty-five rows, two roles
/// each, it is some 30,800 units of the 32,767 Windows takes. A role text that
/// grows or shrinks by much moves these sizes, and the docs' arithmetic with them:
/// it is to be measured again.
#[test]
fn the_command_line_of_a_windows_launch_that_says_a_changed_role_is_the_size_the_docs_say() {
    /// What the rest of the line takes: the two programs' paths, the flags, the
    /// thread's id and the model.
    const REST: usize = 400;
    /// What the harness adds before the role in the developer instructions.
    const FRAMING: usize = 190;
    let staff: Vec<StaffRow> = (0..25)
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
        Some(&specs),
    )
    .unwrap();
    let developer = format!(
        "developer_instructions={}",
        serde_json::to_string(&format!("{}{text}", "x".repeat(FRAMING))).unwrap()
    );
    let said = delivery_text(&note(None, "note", body_of(&text)), &[]);
    let line = on_the_line(&developer) + on_the_line(&said) + REST;
    assert!(
        (30_000..=31_700).contains(&line),
        "{line} units: the docs say 30,848 at 25 rows, of the 32,766 Windows takes"
    );
}
