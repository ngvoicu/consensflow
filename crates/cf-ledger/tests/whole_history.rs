//! A participant's whole history, as the history viewer reads it: one
//! timeline of every item copied of every conversation, oldest first and the
//! current one too, a compaction's summary marked as one, read a page at a time
//! from a place that does not move, with no heading of any conversation
//! (chief-harness, CHF-11): an item says the conversation it stands in, and a
//! conversation with no item is not there. Reading it writes nothing.

#![allow(clippy::expect_used)]

#[path = "whole_history/bounds.rs"]
mod bounds;
#[path = "whole_history/ceilings.rs"]
mod ceilings;
#[path = "whole_history/deleting.rs"]
mod deleting;
#[path = "whole_history/drawn.rs"]
mod drawn;
#[path = "whole_history/healing.rs"]
mod healing;
#[path = "whole_history/load.rs"]
mod load;
#[path = "whole_history/old_logs.rs"]
mod old_logs;
#[path = "whole_history/pages.rs"]
mod pages;
#[path = "whole_history/reads.rs"]
mod reads;

use cf_ledger::{
    open_ledger, ChiefSwitch, HistoryAsk, HistoryFrom, HistoryItem, HistoryRead, Ledger,
    LedgerError, NewChief, NewMember, NewProject, Options, ProjectView, HISTORY_SCAN_BYTES,
};
use serde_json::{json, Value};

/// A conversation's id, and its items.
type Conversation = (i64, Vec<HistoryItem>);

/// Everything of `handle`'s history, the tools' outputs too, read an item at
/// a time from the most recent back (each read's room takes its first item
/// alone), and set out by the conversation each stands in, oldest first.
fn everything(
    ledger: &Ledger,
    project: i64,
    handle: &str,
) -> Result<Vec<Conversation>, LedgerError> {
    let mut ask = HistoryAsk {
        from: HistoryFrom::Newest,
        find: None,
        tools: true,
        room: 1,
        scan: HISTORY_SCAN_BYTES,
    };
    // Newest first, as the reads go.
    let mut shown: Vec<Conversation> = Vec::new();
    // A history of these tests is a few entries: a read that never ends is a fault.
    for _ in 0..100 {
        let HistoryRead {
            entries, resume, ..
        } = ledger.read_history(project, handle, &ask)?;
        for row in entries {
            let id = row.key.conversation;
            if shown
                .last()
                .is_none_or(|(conversation, _)| *conversation != id)
            {
                shown.push((id, Vec::new()));
            }
            if let Some((_, items)) = shown.last_mut() {
                items.push(row.item);
            }
        }
        let Some(resume) = resume else {
            shown.reverse();
            for (_, items) in &mut shown {
                items.reverse();
            }
            return Ok(shown);
        };
        ask.from = HistoryFrom::Before(resume);
    }
    panic!("no end after 100 reads: {} conversations", shown.len());
}

fn ledger_in(dir: &tempfile::TempDir) -> Ledger {
    open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger")
}

/// A project whose chief runs on Claude Code as `ares`.
fn project(ledger: &mut Ledger) -> ProjectView {
    ledger
        .create_project(&NewProject {
            directory: "/work/harbour".into(),
            name: "harbour".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("ares".into()),
                effort: None,
                model_name: None,
            },
            staff: Vec::new(),
            gate: false,
            wake_up: None,
            specs: None,
            staff_rules: None,
            compaction: None,
            advisor_tool: false,
        })
        .expect("a project")
}

fn id_of(project: &ProjectView, handle: &str) -> i64 {
    project
        .participants
        .iter()
        .find(|participant| participant.handle == handle)
        .expect("the participant")
        .id
}

/// An item as a window's reading gives it, complete.
fn item(id: &str, role: &str, text: &str) -> Value {
    json!({ "id": id, "role": role, "text": text, "complete": true })
}

/// What each conversation holds, in a line: its items.
fn lines(history: &[Conversation]) -> Vec<String> {
    history
        .iter()
        .map(|(_, items)| {
            let items: Vec<String> = items
                .iter()
                .map(|item| {
                    let mark = if item.compaction { " (compaction)" } else { "" };
                    format!("{}: {}{mark}", item.role, item.text)
                })
                .collect();
            format!("{items:?}")
        })
        .collect()
}

#[test]
fn reads_every_conversation_of_the_chief_across_a_clear_a_resume_a_compaction_a_switch_and_a_fresh_window(
) {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let chief = id_of(&project, "chief");
    // The chief works, and Claude Code compacts its conversation.
    let first = ledger
        .start_conversation(chief, "claude-code")
        .expect("a conversation");
    ledger.bind_conversation(first.id, "s-1").expect("bound");
    let mut summary = item("u-3", "user", "This session is being continued.");
    summary["compaction"] = true.into();
    let worked = [
        item("u-1", "user", "Ship the parser"),
        item("a-1", "assistant", "On it"),
        item("t-1", "tool", "ok 12 passed"),
        summary,
    ];
    ledger
        .copy_transcript(first.id, &worked, 0)
        .expect("copied");
    // `/clear`, then `/resume` back to the first.
    let cleared = ledger
        .follow_conversation(chief, "claude-code", "s-2")
        .expect("followed");
    ledger
        .copy_transcript(cleared.id, &[item("u-9", "user", "A fresh start")], 0)
        .expect("copied");
    ledger
        .follow_conversation(chief, "claude-code", "s-1")
        .expect("resumed");
    let mut resumed = worked.to_vec();
    resumed.push(item("u-4", "user", "Where were we?"));
    ledger
        .copy_transcript(first.id, &resumed, 0)
        .expect("copied");
    // The human switches the chief to Codex, on maia; it works; a window opens fresh.
    ledger
        .switch_chief(
            project.id,
            &ChiefSwitch {
                harness: "codex".into(),
                model: "maia".into(),
                effort: None,
                cut: false,
                model_name: None,
            },
        )
        .expect("switched");
    let third = ledger
        .start_conversation(chief, "codex")
        .expect("a conversation");
    ledger
        .copy_transcript(third.id, &[item("m-1", "assistant", "Taken over")], 0)
        .expect("copied");
    let fourth = ledger
        .start_conversation(chief, "codex")
        .expect("a conversation");

    let logged = ledger.events(project.id, 0, 500).expect("the log").len();
    let history = everything(&ledger, project.id, "chief").expect("the history");
    // One timeline across the clear, the resume, the compaction and the switch: the
    // fresh window's conversation, which holds no item yet, is not there.
    assert_eq!(
        lines(&history),
        [
            r#"["user: Ship the parser", "assistant: On it", "tool: ok 12 passed", "user: This session is being continued. (compaction)", "user: Where were we?"]"#,
            r#"["user: A fresh start"]"#,
            r#"["assistant: Taken over"]"#,
        ]
    );
    let ids: Vec<i64> = history.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, [first.id, cleared.id, third.id]);
    assert!(
        !ids.contains(&fourth.id),
        "the current one, with nothing copied"
    );
    assert_eq!(
        ledger.events(project.id, 0, 500).expect("the log").len(),
        logged,
        "reading the history writes nothing"
    );
    // `cf history` reads the ended ones, the summary marked there too.
    let earlier = ledger.chief_history(project.id).expect("the history");
    assert_eq!(earlier.len(), 3);
    assert!(earlier[0].items[3].compaction && earlier[0].items[3].role == "user");
    // Once the current one holds an item, it is read too, after the rest.
    ledger
        .copy_transcript(fourth.id, &[item("n-1", "user", "Now")], 0)
        .expect("copied");
    let now = everything(&ledger, project.id, "chief").expect("the history");
    let ids: Vec<i64> = now.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, [first.id, cleared.id, third.id, fourth.id]);
    assert_eq!(lines(&now[3..]), [r#"["user: Now"]"#]);
}

#[test]
fn a_member_s_conversations_with_nothing_copied_show_nothing_and_an_unknown_handle_is_refused() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let member = ledger
        .add_member(
            project.id,
            &NewMember {
                model: None,
                effort: None,
                model_name: Some("zeus".into()),
                harness: "pi".into(),
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            },
        )
        .expect("a member");
    let first = ledger
        .start_conversation(member.id, "pi")
        .expect("a conversation");
    ledger.bind_conversation(first.id, "p-1").expect("bound");
    ledger
        .follow_conversation(member.id, "pi", "p-2")
        .expect("cleared");
    // The chief's switch is the chief's: the member's conversations say nothing of it.
    ledger
        .switch_chief(
            project.id,
            &ChiefSwitch {
                harness: "codex".into(),
                model: "maia".into(),
                effort: None,
                cut: false,
                model_name: None,
            },
        )
        .expect("switched");
    let history = everything(&ledger, project.id, "zeus").expect("the history");
    assert_eq!(lines(&history), Vec::<String>::new());
    let refused = everything(&ledger, project.id, "nobody").expect_err("no such participant");
    assert!(refused.to_string().contains("nobody"), "{refused}");
}

#[test]
fn a_summary_copied_before_the_mark_stays_the_user_s_item_it_was_copied_as() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let chief = id_of(&project, "chief");
    let conversation = ledger
        .start_conversation(chief, "claude-code")
        .expect("a conversation");
    // A ledger copied before the mark: the summary is the user's item it was read as.
    let before = item("s-1", "user", "Summary, as copied before the mark");
    let mut marked = item("s-2", "user", "Summary, marked");
    marked["compaction"] = true.into();
    ledger
        .copy_transcript(conversation.id, &[before.clone(), marked.clone()], 0)
        .expect("copied");
    // Read again whole, as a window is after a restart, with the mark on both now.
    let mut again = before;
    again["compaction"] = true.into();
    ledger
        .copy_transcript(conversation.id, &[again, marked], 0)
        .expect("copied");
    let history = everything(&ledger, project.id, "chief").expect("the history");
    let marks: Vec<bool> = history[0].1.iter().map(|item| item.compaction).collect();
    assert_eq!(
        marks,
        [false, true],
        "what was copied stays as it was copied"
    );
}
