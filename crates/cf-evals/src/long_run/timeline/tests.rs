//! What the timeline tells from one look to the next, and how it is written.

use super::*;
use crate::long_run::ledger::{Participant, Task};
use crate::long_run::look::{Exit, OpenWindow};

fn task(id: i64, state: &str) -> Task {
    Task {
        id,
        number: id,
        title: format!("Task {id}"),
        state: state.to_owned(),
        assignee: Some(3),
        requester: 2,
        updated_at: String::new(),
        deleted: false,
    }
}

fn look(at: i64, tasks: Vec<Task>) -> Look {
    let mut look = Look {
        at,
        ..Look::default()
    };
    look.ledger.tasks = tasks;
    look.ledger.participants.push(Participant {
        id: 3,
        handle: "long-claude-1".to_owned(),
        role: "worker".to_owned(),
        harness: Some("claude-code".to_owned()),
        agent: None,
    });
    look
}

#[test]
fn a_tasks_new_state_a_window_opened_and_one_that_exited_are_told_from_one_look_to_the_next() {
    let mut timeline = Timeline::new(0);
    let first = look(10_000, vec![task(1, "queued")]);
    timeline.changes(None, &first);
    let mut second = look(20_000, vec![task(1, "working"), task(2, "queued")]);
    second.windows.open.push(OpenWindow {
        handle: "long-claude-1".to_owned(),
        pane: "p1-long-claude-1-1".to_owned(),
        generation: 1,
        pid: Some(5),
        running: true,
        hosted: true,
        background: Vec::new(),
        background_total: 0,
        tree_unread: None,
        tail: Vec::new(),
    });
    timeline.changes(Some(&first), &second);
    let mut third = look(3_725_000, vec![task(1, "working"), task(2, "queued")]);
    third.windows.exits.push(Exit {
        handle: "long-claude-1".to_owned(),
        pane: "p1-long-claude-1-1".to_owned(),
        generation: 1,
        at: 3_725_000,
        code: None,
        signal: Some("SIGKILL".to_owned()),
        closed_by: ClosedBy::Run,
        tail: Vec::new(),
        ended: Vec::new(),
    });
    timeline.changes(Some(&second), &third);
    let said: Vec<(&str, &str, String)> = timeline
        .moments
        .iter()
        .map(|moment| {
            (
                moment.kind,
                moment.text.as_str(),
                timeline.offset(moment.at),
            )
        })
        .collect();
    assert_eq!(
        said,
        [
            (
                "task",
                "T-1 queued @long-claude-1: Task 1",
                "+0m10s".to_owned()
            ),
            (
                "task",
                "T-1 queued → working @long-claude-1",
                "+0m20s".to_owned()
            ),
            (
                "task",
                "T-2 queued @long-claude-1: Task 2",
                "+0m20s".to_owned()
            ),
            (
                "window",
                "@long-claude-1's window p1-long-claude-1-1 opened",
                "+0m20s".to_owned()
            ),
            (
                "window",
                "@long-claude-1's window p1-long-claude-1-1 exited (killed by the run, SIGKILL)",
                "+1h02m05s".to_owned()
            ),
        ]
    );
}

#[test]
fn the_timeline_is_kept_as_json_and_written_as_a_markdown_table() {
    let mut timeline = Timeline::new(1_791_712_800_000);
    timeline.add(1_791_712_812_000, "owner", "typed the ask | all of it");
    let kept = timeline.to_value();
    assert_eq!(kept[0]["at"], "2026-10-11T10:00:12.000Z");
    assert_eq!(kept[0]["offset"], "+0m12s");
    assert_eq!(kept[0]["kind"], "owner");
    assert_eq!(
        timeline.markdown(),
        "| when | what | |\n|---|---|---|\n| +0m12s | owner | typed the ask \\| all of it |\n"
    );
}

#[test]
fn moments_told_out_of_their_order_are_written_in_it() {
    // A disruption that takes minutes is told after the look before it was taken.
    let mut timeline = Timeline::new(0);
    timeline.add(400_000, "disruption", "compact: done");
    timeline.add(100_000, "task", "T-1 working → done");
    let kept = timeline.to_value();
    assert_eq!(kept[0]["offset"], "+1m40s");
    assert_eq!(kept[1]["offset"], "+6m40s");
    assert!(
        timeline.markdown().find("T-1").unwrap() < timeline.markdown().find("compact").unwrap()
    );
}

#[test]
fn a_window_opened_again_under_its_id_is_said_opened_again() {
    let window = |generation: i64| OpenWindow {
        handle: "long-claude-1".to_owned(),
        pane: "p1-long-claude-1-1".to_owned(),
        generation,
        pid: None,
        running: true,
        hosted: true,
        background: Vec::new(),
        background_total: 0,
        tree_unread: None,
        tail: Vec::new(),
    };
    let mut timeline = Timeline::new(0);
    let mut first = look(1_000, Vec::new());
    first.windows.open.push(window(1));
    let mut second = look(2_000, Vec::new());
    second.windows.open.push(window(1));
    let mut third = look(3_000, Vec::new());
    third.windows.open.push(window(2));
    timeline.changes(None, &first);
    timeline.changes(Some(&first), &second);
    timeline.changes(Some(&second), &third);
    let opened: Vec<i64> = timeline.moments.iter().map(|moment| moment.at).collect();
    assert_eq!(opened, [1_000, 3_000]);
}

#[test]
fn what_the_timeline_says_has_a_key_or_a_token_hidden() {
    let mut timeline = Timeline::new(0);
    timeline.add(
        1_000,
        "owner",
        "the owner's follow-up: deploy with DEPLOY_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789",
    );
    assert_eq!(
        timeline.moments[0].text,
        "the owner's follow-up: deploy with DEPLOY_TOKEN=[masked]"
    );
    assert!(!timeline.markdown().contains("ghp_"));
}

#[test]
fn a_tasks_title_is_masked_whole_before_the_timeline_cuts_it() {
    // The key's value runs past the 70 characters a new task's title is told by: cut
    // first, seven characters of it are left, which is no key's shape.
    let mut titled = task(1, "queued");
    titled.title = format!("{}DEPLOY_TOKEN=abcd1234efgh5678", "x ".repeat(25));
    let mut timeline = Timeline::new(0);
    timeline.changes(None, &look(1_000, vec![titled]));
    let text = &timeline.moments[0].text;
    assert!(!text.contains("abcd"), "{text}");
    assert!(text.contains("DEPLOY_TOKEN=[mas"), "{text}");
}

#[test]
fn the_timeline_says_the_runs_own_handles_and_window_ids_whole() {
    // The dry run's worker, whose handle and window id the rule of 24 characters hides.
    let mut first = look(1_000, vec![task(1, "working")]);
    first.ledger.project = Some(crate::long_run::ledger::Project {
        id: 1,
        name: "ledgerline".to_owned(),
        state: "open".to_owned(),
    });
    first.ledger.participants[0].handle = "long-claude-1-olive-valley".to_owned();
    let mut timeline = Timeline::new(0);
    timeline.changes(None, &first);
    timeline.add(
        2_000,
        "disruption",
        "kill: @long-claude-1-olive-valley's window p1-long-claude-1-olive-valley killed",
    );
    let said: Vec<&str> = timeline
        .moments
        .iter()
        .map(|moment| moment.text.as_str())
        .collect();
    assert_eq!(
        said,
        [
            "T-1 working @long-claude-1-olive-valley: Task 1",
            "kill: @long-claude-1-olive-valley's window p1-long-claude-1-olive-valley killed",
        ]
    );
    // A name no look showed is hidden as any token is.
    timeline.add(3_000, "owner", "cookie abc123-def456-ghi789-jkl012");
    assert_eq!(timeline.moments[2].text, "cookie [masked]");
}

#[test]
fn a_wake_up_call_the_chiefs_switch_and_the_projects_state_are_told_once_from_the_log() {
    let event = |id: i64, second: u32, kind: &str, data: serde_json::Value| Event {
        id,
        at: format!("2026-10-11T10:00:{second:02}.000Z"),
        kind: kind.to_owned(),
        data,
    };
    let mut first = look(1_791_712_810_000, Vec::new());
    first.ledger.events = vec![
        event(1, 1, "project.created", json!({ "name": "ledgerline" })),
        event(2, 5, "project.wake_up", json!({ "message": 15 })),
    ];
    let mut second = first.clone();
    second.at = 1_791_712_830_000;
    second.ledger.events.extend([
        event(
            3,
            20,
            "chief.switched",
            json!({ "to": { "harness": "codex", "agent": "long-codex-1" }, "cut": false }),
        ),
        event(
            4,
            25,
            "project.state",
            json!({ "from": "open", "to": "suspended" }),
        ),
        event(5, 26, "task.state", json!({ "task": 1, "to": "paused" })),
    ]);
    let mut timeline = Timeline::new(1_791_712_800_000);
    timeline.changes(None, &first);
    timeline.changes(Some(&first), &second);
    let said: Vec<(String, &str)> = timeline
        .moments
        .iter()
        .filter(|moment| moment.kind == "project")
        .map(|moment| (timeline.offset(moment.at), moment.text.as_str()))
        .collect();
    assert_eq!(
        said,
        [
            (
                "+0m05s".to_owned(),
                "a wake-up call nudged the chief (m-15)"
            ),
            ("+0m20s".to_owned(), "the chief is now codex (long-codex-1)"),
            ("+0m25s".to_owned(), "the project is suspended"),
        ]
    );
}
