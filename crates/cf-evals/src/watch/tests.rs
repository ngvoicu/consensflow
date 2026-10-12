//! What a look at a project says, from a ledger and a trace written by hand.

use std::fs;

use serde_json::json;

use super::tokens::{used, Used};
use super::*;
use crate::ledger_file::{add_project, Chief, Member, NewProject};

/// A home with a ledger of one project whose chief and one worker are on Claude
/// Code, and the trace `events` (a line each).
fn home(events: &[Value]) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let db = crate::ledger_file::open(&home.path().join("consensflow.db"), false).unwrap();
    let worker = Member {
        agent: "zeus",
        harness: "claude-code",
        roles: &["worker"],
        tier: "standard",
    };
    add_project(
        &db,
        &NewProject {
            directory: "/work/site",
            name: "poker-lab",
            chief: Chief {
                harness: "claude-code",
                agent: None,
            },
            staff: &[worker],
        },
    )
    .unwrap();
    // Zeus holds a task, and a message is on its way to him.
    db.execute_batch(
        "INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state, created_at, updated_at)
         VALUES (1, 1, 'Write the parser, and then some more words to make a long title for the list', 'b', 2, 3, 'working', '2026-10-08T10:00:00.000Z', '2026-10-08T10:20:00.000Z');
         INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state, created_at, updated_at)
         VALUES (1, 2, 'Done long ago', 'b', 2, 3, 'accepted', '2026-10-08T10:00:00.000Z', '2026-10-08T10:20:00.000Z');
         INSERT INTO message (project_id, recipient_id, sender_id, kind, task_id, body, state, created_at)
         VALUES (1, 3, 2, 'task', 1, 'Please\n  write   it', 'queued', '2026-10-08T10:00:00.000Z');",
    )
    .unwrap();
    drop(db);
    let lines: Vec<String> = events.iter().map(Value::to_string).collect();
    fs::write(
        home.path().join("events.jsonl"),
        format!("{}\n", lines.join("\n")),
    )
    .unwrap();
    home
}

fn looking_at(home: &tempfile::TempDir, project: &str) -> Asked {
    Asked {
        project: project.to_owned(),
        minutes: "60".to_owned(),
        tokens: false,
        home: home.path().to_path_buf(),
    }
}

/// What watching says of the project `project` of `home`.
fn said(home: &tempfile::TempDir, project: &str) -> String {
    let mut out = Vec::new();
    watch(&looking_at(home, project), 60.0, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

fn activity(at: i64, state: &str, reason: Option<&str>) -> Value {
    let when = clock::iso(Timestamp::from_millisecond(at).unwrap());
    let mut event = json!({ "at": when, "project": 1, "kind": "window.activity", "participant": "zeus", "state": state });
    if let Some(reason) = reason {
        event["reason"] = json!(reason);
    }
    event
}

#[test]
fn a_project_is_found_by_its_name_or_its_id_and_one_that_is_not_there_is_named() {
    let home = home(&[]);
    assert!(
        said(&home, "poker-lab").starts_with("poker-lab (project 1, open); the last 60 minutes\n")
    );
    assert!(said(&home, "1").starts_with("poker-lab (project 1, open)"));
    let mut out = Vec::new();
    let failed = watch(&looking_at(&home, "nothing"), 60.0, &mut out).unwrap_err();
    assert_eq!(failed.to_string(), "no project nothing");
    let failed = watch(&looking_at(&home, "0"), 60.0, &mut out).unwrap_err();
    assert_eq!(failed.to_string(), "no project 0");
}

#[test]
fn says_the_tasks_not_finished_and_who_holds_each_and_the_messages_on_their_way() {
    let text = said(&home(&[]), "poker-lab");
    let tasks: Vec<&str> = text
        .lines()
        .skip_while(|line| *line != "Tasks not finished")
        .skip(1)
        .take_while(|line| !line.is_empty())
        .collect();
    assert_eq!(tasks.len(), 1, "the accepted one is finished: {text}");
    // The state is padded to eight, the handle to twenty-six and the harness to twelve.
    assert!(
        tasks[0].starts_with(&format!(
            "  T-1 working  zeus{}claude-code  since ",
            " ".repeat(23)
        )),
        "{}",
        tasks[0]
    );
    // The title is cut at fifty characters.
    assert!(
        tasks[0].ends_with("  Write the parser, and then some more words to make"),
        "{}",
        tasks[0]
    );
    let waiting: Vec<&str> = text
        .lines()
        .skip_while(|line| *line != "Messages on their way")
        .skip(1)
        .take_while(|line| !line.is_empty())
        .collect();
    assert_eq!(waiting.len(), 1);
    assert!(
        waiting[0].starts_with("  m-1 task queued chief -> zeus T-1 since ")
            && waiting[0].ends_with(": Please write it"),
        "{}",
        waiting[0]
    );
}

#[test]
fn says_each_windows_changes_and_flags_one_that_flips_between_working_and_idle() {
    let now = Timestamp::now().as_millisecond();
    // Twelve changes a second apart: all of them quick but the first.
    let mut events: Vec<Value> = (0..12)
        .map(|n| {
            activity(
                now - 60_000 + n * 1000,
                if n % 2 == 0 { "working" } else { "idle" },
                None,
            )
        })
        .collect();
    events.push(json!({ "at": clock::iso(Timestamp::from_millisecond(now - 1000).unwrap()), "project": 2, "kind": "window.activity", "participant": "other", "state": "idle" }));
    let text = said(&home(&events), "poker-lab");
    let line = text
        .lines()
        .find(|line| line.starts_with("  zeus"))
        .unwrap_or_default();
    assert!(
        line.starts_with(&format!(
            "  zeus{}12 changes (11 within 5 s), now idle since ",
            " ".repeat(25)
        )) && line.ends_with("  FLIPPING"),
        "{line}"
    );
    assert!(
        !text.contains("other"),
        "another project's window is not this one's"
    );
}

#[test]
fn a_window_that_stopped_on_a_prompt_says_why_and_a_calm_one_is_not_flagged() {
    let now = Timestamp::now().as_millisecond();
    let events = [
        activity(now - 600_000, "working", None),
        activity(now - 300_000, "waiting", Some("a permission prompt")),
        activity(now - 100_000, "waiting", None),
    ];
    let text = said(&home(&events), "poker-lab");
    assert!(
        text.contains(" 3 changes (0 within 5 s), now waiting since "),
        "{text}"
    );
    assert!(!text.contains("FLIPPING"));
    assert!(text.contains("\nStopped on a prompt\n"), "{text}");
    let prompts: Vec<&str> = text
        .lines()
        .skip_while(|line| *line != "Stopped on a prompt")
        .skip(1)
        .collect();
    assert!(
        prompts[0].ends_with(" zeus: a permission prompt"),
        "{}",
        prompts[0]
    );
    assert!(prompts[1].ends_with(" zeus: waiting"), "{}", prompts[1]);
}

#[test]
fn events_older_than_the_minutes_asked_for_are_not_looked_at() {
    let now = Timestamp::now().as_millisecond();
    let events = [activity(now - 3 * 3_600_000, "working", None)];
    let text = said(&home(&events), "poker-lab");
    assert!(!text.contains(" changes ("), "{text}");
    assert!(text.contains("\nWindows\n"));
}

#[test]
fn deliveries_held_retried_or_failed_are_listed_with_the_message_and_the_reason() {
    let now = Timestamp::now().as_millisecond();
    let at = clock::iso(Timestamp::from_millisecond(now - 1000).unwrap());
    let events = [
        json!({ "at": at, "project": 1, "kind": "delivery.held", "message": 4, "participant": "zeus", "reason": "the window is busy" }),
        json!({ "at": at, "project": 1, "kind": "delivery.failed", "data": { "message": 5, "reason": "gone" } }),
        json!({ "at": at, "project": 1, "kind": "delivery.begun", "message": 6 }),
    ];
    let text = said(&home(&events), "poker-lab");
    let lines: Vec<&str> = text
        .lines()
        .skip_while(|line| *line != "Deliveries held, retried or failed")
        .skip(1)
        .take_while(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(
        lines[0].ends_with(" held    m-4 zeus the window is busy"),
        "{}",
        lines[0]
    );
    assert!(lines[1].ends_with(" failed  m-5  gone"), "{}", lines[1]);
}

#[test]
fn a_member_out_of_quota_is_named_with_when_it_is_back_and_none_when_all_are_in() {
    let home = home(&[]);
    assert!(!said(&home, "poker-lab").contains("Out of quota"));
    let db = Connection::open(home.path().join("consensflow.db")).unwrap();
    db.execute(
        "UPDATE participant SET out_until = '2999-01-01T00:00:00.000Z' WHERE handle = 'zeus'",
        [],
    )
    .unwrap();
    drop(db);
    let text = said(&home, "poker-lab");
    assert!(text.contains("\nOut of quota\n  zeus until "), "{text}");
}

#[test]
fn a_flag_that_is_missing_or_unknown_is_the_humans_mistake_and_the_usage_says_so() {
    let failed = asked_of(&[]).unwrap_err();
    assert!(matches!(failed, Error::Usage(_)));
    assert!(failed
        .to_string()
        .starts_with("usage: cargo xtask watch --project"));
    assert!(asked_of(&["--project", "5", "--nope", "1"]).is_err());
    let asked = asked_of(&[
        "--project",
        "5",
        "--minutes",
        "30",
        "--tokens",
        "--home",
        "/h",
    ])
    .unwrap();
    assert_eq!(
        (
            asked.project.as_str(),
            asked.minutes.as_str(),
            asked.tokens,
            asked.home
        ),
        ("5", "30", true, PathBuf::from("/h"))
    );
}

fn asked_of(words: &[&str]) -> Result<Asked> {
    asked(
        &words
            .iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<_>>(),
    )
}

#[test]
fn a_conversation_is_counted_a_message_once_and_its_first_turns_context_apart() {
    let line = |kind: &str, id: &str, read: u64, created: u64, input: u64, output: u64| {
        json!({
            "type": kind,
            "message": { "id": id, "usage": {
                "cache_read_input_tokens": read,
                "cache_creation_input_tokens": created,
                "input_tokens": input,
                "output_tokens": output,
            } },
        })
        .to_string()
    };
    let record = [
        line("assistant", "m1", 100, 50, 5, 10),
        // The same message again, for its next content block.
        line("assistant", "m1", 100, 50, 5, 10),
        line("user", "u1", 1, 1, 1, 1),
        line("assistant", "m2", 200, 20, 3, 30),
        "{\"type\":\"assistant\"}".to_owned(),
        "not json with \"usage\" in it".to_owned(),
    ]
    .join("\n");
    assert_eq!(
        used(&record),
        Used {
            turns: 2,
            first: Some(155),
            read: 300,
            written: 78,
            output: 40,
        }
    );
    assert_eq!(used(""), Used::default());
}
