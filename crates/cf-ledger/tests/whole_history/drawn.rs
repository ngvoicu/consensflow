//! What the history draws of an item besides its words (chief-harness CHF-16, CHF-18):
//! kept with the item, compared as its words are, read from its head (a change an edit
//! made from a row of its own), and none for an item copied before migration 0032, which
//! draws as a plain one. An item only the history shows is in the history alone: no
//! reader of the conversation's words (`cf history`, a task's transcript) reads it.

use cf_ledger::{NewMember, NewTask, TRANSCRIPT_ITEM_MAX};

use super::*;

/// A tool's item `t-1`, its call's detail saying `status` and `took`.
fn call(status: &str, took: u64, text: &str) -> Value {
    let mut said = item("t-1", "tool", text);
    said["detail"] = json!({
        "kind": "tool", "name": "Bash", "header": "$ cargo test", "status": status,
        "tookMs": took,
    });
    said
}

/// An item only the history shows: of `role`, of `kind`.
fn drawn_only(id: &str, role: &str, kind: &str, text: &str) -> Value {
    let mut said = item(id, role, text);
    said["detail"] = json!({ "kind": kind });
    said["drawnOnly"] = true.into();
    if kind == "compaction" {
        said["compaction"] = true.into();
    }
    said
}

/// A chief's conversation, of `project`.
fn chiefs_conversation(ledger: &mut Ledger, project: &ProjectView) -> i64 {
    let chief = id_of(project, "chief");
    let conversation = ledger
        .start_conversation(chief, "claude-code")
        .expect("a conversation");
    conversation.id
}

/// The chief's history as one timeline, each item as its page's JSON says it.
fn timeline(ledger: &Ledger, project: i64) -> Vec<Value> {
    everything(ledger, project, "chief")
        .expect("the history")
        .into_iter()
        .flat_map(|(_, items)| items)
        .map(|item| serde_json::to_value(item).expect("json"))
        .collect()
}

#[test]
fn a_calls_detail_is_kept_and_written_again_when_it_says_more_with_the_same_words() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let conversation = chiefs_conversation(&mut ledger, &project);
    let running = call("running", 0, "");
    assert_eq!(
        ledger
            .copy_transcript(conversation, std::slice::from_ref(&running), 0)
            .expect("copied"),
        1
    );
    // The same again writes nothing.
    assert_eq!(
        ledger
            .copy_transcript(conversation, &[running], 0)
            .expect("copied"),
        0
    );
    // It ended: its words are the same (none yet), and what is drawn of it is not.
    let ended = call("failed", 1_200, "");
    assert_eq!(
        ledger
            .copy_transcript(conversation, std::slice::from_ref(&ended), 0)
            .expect("copied"),
        1,
        "a status or a time with the same words is written again"
    );
    assert_eq!(
        ledger
            .copy_transcript(conversation, &[ended], 0)
            .expect("copied"),
        0
    );
    assert_eq!(
        timeline(&ledger, project.id),
        [json!({
            "id": "t-1", "role": "tool", "text": "", "complete": true, "at": null,
            "kind": "tool", "name": "Bash", "header": "$ cargo test", "status": "failed",
            "tookMs": 1_200,
        })]
    );
}

#[test]
fn an_item_copied_before_its_detail_keeps_its_row_and_the_others_stay_plain() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let conversation = chiefs_conversation(&mut ledger, &project);
    let before = [
        item("u-1", "user", "Run the tests"),
        item("a-1", "assistant", "Running"),
        item("t-1", "tool", "ok"),
    ];
    ledger
        .copy_transcript(conversation, &before, 0)
        .expect("copied");
    let plain = timeline(&ledger, project.id);
    // Every old item draws as a plain one: nothing of a detail in its JSON.
    assert!(plain
        .iter()
        .all(|item| item.get("kind").is_none() && item.get("header").is_none()));
    // A build that reads more of the record copies its last item again, from there.
    ledger
        .copy_transcript(conversation, &[call("ok", 5, "ok")], 2)
        .expect("copied");
    let after = timeline(&ledger, project.id);
    assert_eq!(after.len(), 3, "no item copied twice");
    assert_eq!(after[..2], plain[..2]);
    assert_eq!(after[2]["header"], "$ cargo test");
    let rows: i64 = ledger
        .sqlite()
        .query_row("SELECT count(*) FROM transcript", [], |row| row.get(0))
        .expect("counted");
    assert_eq!(rows, 3);
}

#[test]
fn a_ledger_from_before_migration_0032_opens_and_its_items_read_as_they_did() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let conversation = chiefs_conversation(&mut ledger, &project);
    ledger
        .copy_transcript(
            conversation,
            &[
                item("u-1", "user", "Hello"),
                item("a-1", "assistant", "Hi"),
                item("t-1", "tool", "out"),
            ],
            0,
        )
        .expect("copied");
    let read = timeline(&ledger, project.id);
    let history = ledger.chief_history(project.id).expect("cf history");
    drop(ledger);
    {
        let db = rusqlite::Connection::open(dir.path().join("consensflow.db")).expect("the file");
        cf_ledger::testing::undo_migration_0035(&db).expect("undone");
        cf_ledger::testing::undo_migration_0034(&db).expect("undone");
        cf_ledger::testing::undo_migration_0033(&db).expect("undone");
        cf_ledger::testing::undo_migration_0032(&db).expect("undone");
        db.execute_batch("PRAGMA user_version = 31").expect("31");
    }
    let ledger = ledger_in(&dir);
    assert_eq!(timeline(&ledger, project.id), read);
    assert_eq!(
        ledger.chief_history(project.id).expect("cf history"),
        history
    );
}

#[test]
fn the_items_only_the_history_shows_are_in_it_where_they_stand_and_in_no_reader_of_words() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let conversation = chiefs_conversation(&mut ledger, &project);
    let words = [
        item("u-1", "user", "Why?"),
        item("a-1", "assistant", "Because."),
    ];
    ledger
        .copy_transcript(conversation, &words, 0)
        .expect("copied");
    let said = ledger.chief_history(project.id).expect("cf history");
    let timeline_of_words = timeline(&ledger, project.id);
    let with_drawn = [
        item("u-1", "user", "Why?"),
        drawn_only(
            "r-1:thinking:0",
            "custom",
            "thinking",
            "The human asks why.",
        ),
        item("a-1", "assistant", "Because."),
        drawn_only("c-1", "custom", "compaction", ""),
    ];
    ledger
        .copy_transcript(conversation, &with_drawn, 0)
        .expect("copied");
    let read = timeline(&ledger, project.id);
    let kinds = |read: &[Value]| {
        read.iter()
            .map(|item| {
                (
                    item["id"].as_str().unwrap_or_default().to_owned(),
                    item["kind"].as_str().unwrap_or("plain").to_owned(),
                )
            })
            .collect::<Vec<_>>()
    };
    // The thinking stands before words copied already: a row of its own would stand after
    // them, so it is not copied (a ledger a build before this one wrote, read again from
    // its start). The compaction comes after every item copied, and is.
    assert_eq!(
        kinds(&read),
        [
            ("u-1".to_owned(), "plain".to_owned()),
            ("a-1".to_owned(), "plain".to_owned()),
            ("c-1".to_owned(), "compaction".to_owned()),
        ]
    );
    // In a conversation copied as it goes, it is where it stands.
    let fresh = chiefs_conversation(&mut ledger, &project);
    ledger
        .copy_transcript(fresh, &with_drawn[..2], 0)
        .expect("copied");
    ledger
        .copy_transcript(fresh, &with_drawn[1..], 1)
        .expect("copied");
    let read = timeline(&ledger, project.id);
    assert_eq!(
        kinds(&read[3..]),
        [
            ("u-1".to_owned(), "plain".to_owned()),
            ("r-1:thinking:0".to_owned(), "thinking".to_owned()),
            ("a-1".to_owned(), "plain".to_owned()),
            ("c-1".to_owned(), "compaction".to_owned()),
        ]
    );
    let read = read[3..].to_vec();
    chiefs_conversation(&mut ledger, &project);
    let history = ledger.chief_history(project.id).expect("cf history");
    let words_of = |at: usize| {
        history[at]
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(words_of(0), ["u-1", "a-1"]);
    assert_eq!(words_of(1), ["u-1", "a-1"]);
    assert_eq!(read[1]["role"], "custom");
    assert_eq!(read[1]["text"], "The human asks why.");
    // `cf history` is as it was: the words alone, of each conversation that ended.
    assert!(
        said.is_empty(),
        "the conversation is the chief's current one"
    );
    assert_eq!(
        [read[0].clone(), read[2].clone()],
        [timeline_of_words[0].clone(), timeline_of_words[1].clone()]
    );
}

#[test]
fn thinking_is_cut_as_a_tools_output_is_and_a_change_as_well() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let conversation = chiefs_conversation(&mut ledger, &project);
    let long = "x".repeat(TRANSCRIPT_ITEM_MAX + 10);
    let mut edit = call("ok", 3, "The file a.rs has been updated.");
    edit["detail"]["diff"] = json!(format!("@@ -1,1 +1,1 @@\n-{long}\n+y"));
    edit["detail"]["header"] = json!(format!("edit {}", "é".repeat(400)));
    ledger
        .copy_transcript(
            conversation,
            &[
                drawn_only("r-1:thinking:0", "custom", "thinking", &long),
                edit,
            ],
            0,
        )
        .expect("copied");
    let read = timeline(&ledger, project.id);
    let thought: String = ledger
        .sqlite()
        .query_row(
            "SELECT text FROM transcript WHERE item_id = 'r-1:thinking:0'",
            [],
            |row| row.get(0),
        )
        .expect("the thinking");
    assert!(thought.ends_with("characters; cut here)"));
    assert!(thought.chars().count() < TRANSCRIPT_ITEM_MAX + 100);
    let diff = read[1]["diff"].as_str().unwrap_or_default();
    assert!(diff.starts_with("@@ -1,1 +1,1 @@\n-x"));
    assert!(diff.ends_with("characters; cut here)"));
    assert_eq!(
        read[1]["header"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .count(),
        256
    );
}

#[test]
fn a_kind_or_a_status_the_ledger_does_not_know_draws_as_a_plain_item_or_no_status() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let conversation = chiefs_conversation(&mut ledger, &project);
    let mut strange = item("x-1", "assistant", "words");
    strange["detail"] = json!({ "kind": "hologram", "header": "?" });
    let mut odd = call("melted", 1, "out");
    odd["id"] = json!("t-2");
    ledger
        .copy_transcript(conversation, &[strange, odd], 0)
        .expect("copied");
    let read = timeline(&ledger, project.id);
    assert_eq!(
        read[0],
        json!({ "id": "x-1", "role": "assistant", "text": "words", "complete": true, "at": null })
    );
    assert_eq!(read[1]["kind"], "tool");
    assert!(read[1].get("status").is_none());
}

#[test]
fn a_tasks_transcript_says_what_each_call_was_and_shows_nothing_only_the_history_shows() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("apollo".into()),
                effort: None,
                model_name: None,
            },
            staff: vec![NewMember {
                model: None,
                effort: None,
                model_name: Some("zeus".into()),
                harness: "claude-code".into(),
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            }],
            gate: false,
            wake_up: None,
            specs: None,
            staff_rules: None,
            compaction: None,
            advisor_tool: false,
        })
        .expect("a project");
    ledger
        .create_task(
            project.id,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Parser".into(),
                ..NewTask::default()
            },
        )
        .expect("a task");
    let zeus = id_of(&project, "zeus");
    let brief = ledger
        .assign_task(project.id, 1, zeus)
        .expect("assigned")
        .message
        .expect("its brief")
        .id;
    ledger.begin_delivery(brief).expect("the brief goes in");
    ledger
        .confirm_delivery(brief, Some(&json!({ "item": "i-1" })))
        .expect("the brief is confirmed");
    // The brief went to the window the task was given to: the member's session.
    let session: i64 = ledger
        .sqlite()
        .query_row(
            "SELECT recipient_id FROM message WHERE id = ?",
            [brief],
            |row| row.get(0),
        )
        .expect("the brief's window");
    let window = ledger
        .start_conversation(session, "claude-code")
        .expect("a conversation");
    let mut edit = call("ok", 7, "updated");
    edit["detail"]["diff"] = json!("-a\n+b");
    ledger
        .copy_transcript(
            window.id,
            &[
                item(
                    "i-1",
                    "user",
                    &format!("[ConsensFlow m-{brief} · T-1 · task from @chief]\nParser"),
                ),
                drawn_only(
                    "r-1:thinking:0",
                    "custom",
                    "thinking",
                    "Where is the parser?",
                ),
                edit,
                item("a-1", "assistant", "Done."),
            ],
            0,
        )
        .expect("copied");
    let transcript = ledger
        .transcript(project.id, 1, None)
        .expect("the transcript");
    assert_eq!(
        transcript.total, 3,
        "the thinking is no item of a task's transcript"
    );
    let said: Vec<Value> = transcript
        .items
        .iter()
        .map(|item| serde_json::to_value(item).expect("json"))
        .collect();
    assert_eq!(said[1]["header"], "$ cargo test");
    assert_eq!(said[1]["status"], "ok");
    assert_eq!(said[1]["tookMs"], 7);
    assert!(
        said[1].get("diff").is_none(),
        "a task's transcript shows no change"
    );
    assert!(said[0].get("kind").is_none() && said[2].get("kind").is_none());
}
