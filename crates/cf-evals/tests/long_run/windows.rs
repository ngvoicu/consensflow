//! 6 (B2): a window open on the board with no process, a window that closed by
//! itself mid-task, a stranger; 8 (B1): a tool call skipped, a dialog left waiting.

use cf_evals::long_run::invariants::{check, unjudged, Keeper};
use cf_evals::long_run::look::{
    Activity, ClosedBy, Exit, OpenWindow, Process, Stranger, Unresolved,
};
use cf_evals::long_run::mask::Names;
use serde_json::json;

use crate::scene::{found, ms, t, Scene};

fn window(handle: &str, running: bool, hosted: bool) -> OpenWindow {
    OpenWindow {
        handle: handle.to_owned(),
        pane: format!("p1-{handle}-1"),
        generation: 3,
        pid: Some(5150),
        running,
        hosted,
        background: Vec::new(),
        background_total: 0,
        tree_unread: None,
        tail: vec!["❯ ".to_owned()],
    }
}

#[test]
fn a_window_open_on_the_board_with_no_process_breaks_the_sixth_once_two_looks_find_it() {
    let scene = Scene::new(&[("long-worker", "devin")]);
    let mut look = scene.look(ms(5, 0));
    look.windows.open.push(window("long-worker", false, true));
    let breaks = found(&look, 6);
    assert_eq!(breaks.len(), 1);
    assert!(breaks[0].twice);
    assert_eq!(
        breaks[0].said,
        "@long-worker's window is open on the board, and its process 5150 is gone"
    );
    let mut keeper = Keeper::default();
    assert!(
        keeper
            .keep(&look, check(&look, &Default::default()))
            .is_empty(),
        "one look is a moment"
    );
    look.at = ms(5, 10);
    let kept = keeper.keep(&look, check(&look, &Default::default()));
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].at, ms(5, 10));
    // The host has no such window at all.
    let mut look = scene.look(ms(5, 0));
    look.windows.open.push(window("long-worker", true, false));
    assert!(found(&look, 6)[0]
        .said
        .ends_with("and the pane host has no p1-long-worker-1"));
    // A window with its process, answered for, holds it.
    let mut look = scene.look(ms(5, 0));
    look.windows.open.push(window("long-worker", true, true));
    assert!(found(&look, 6).is_empty());
}

fn exit(closed_by: ClosedBy) -> Exit {
    Exit {
        handle: "long-worker".to_owned(),
        pane: "p1-long-worker-1".to_owned(),
        generation: 3,
        at: cf_evals::clock::millis(&t(5, 0)).unwrap(),
        code: None,
        signal: Some("SIGKILL".to_owned()),
        closed_by,
        tail: vec!["Running npm test…".to_owned()],
        ended: Vec::new(),
    }
}

#[test]
fn a_window_that_closed_by_itself_while_its_task_was_at_work_breaks_the_sixth() {
    let scene = Scene::new(&[("long-worker", "devin")]);
    scene.task(18, 3, "paused", &t(5, 1));
    scene.event(
        &t(1, 0),
        "task.state",
        &json!({ "task": 18, "from": "queued", "to": "working" }),
    );
    scene.event(
        &t(5, 1),
        "task.state",
        &json!({ "task": 18, "from": "working", "to": "paused", "reason": "window closed" }),
    );
    let mut look = scene.look(ms(6, 0));
    look.windows.exits.push(exit(ClosedBy::Itself));
    let breaks = found(&look, 6);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(
        breaks[0].said,
        "@long-worker's window closed by itself while T-18 was at work"
    );
    assert_eq!(breaks[0].evidence.screen, ["Running npm test…"]);
    // The daemon's close, and the run's own, are no fault of the window.
    for closer in [ClosedBy::Daemon, ClosedBy::Run] {
        let mut look = scene.look(ms(6, 0));
        look.windows.exits.push(exit(closer));
        assert!(found(&look, 6).is_empty(), "{closer:?}");
    }
    // A window that closed by itself with no task at work is no task's loss.
    let scene = Scene::new(&[("long-worker", "devin")]);
    scene.task(18, 3, "accepted", &t(4, 0));
    scene.event(
        &t(4, 0),
        "task.state",
        &json!({ "task": 18, "from": "done", "to": "accepted" }),
    );
    let mut look = scene.look(ms(6, 0));
    look.windows.exits.push(exit(ClosedBy::Itself));
    assert!(found(&look, 6).is_empty());
}

#[test]
fn a_process_left_when_its_window_is_gone_breaks_the_sixth() {
    let scene = Scene::new(&[]);
    let mut look = scene.look(ms(6, 0));
    look.windows.strangers.push(Stranger {
        process: Process {
            pid: 7777,
            command: "node scripts/longrun-dev-server.mjs".to_owned(),
        },
        pane: "p1-long-worker-1".to_owned(),
        since: ms(4, 0),
        began: "Fri Oct 16 00:29:15 2026".to_owned(),
    });
    let breaks = found(&look, 6);
    assert_eq!(breaks[0].key, "6:stranger:7777");
    assert!(breaks[0]
        .said
        .starts_with("process 7777 (node scripts/longrun-dev-server.mjs) still ran"));
    assert!(
        breaks[0]
            .said
            .ends_with("; it was left running, for the run ends only what it started"),
        "{}",
        breaks[0].said
    );
    assert_eq!(
        breaks[0].evidence.rows,
        ["7777 node scripts/longrun-dev-server.mjs (began Fri Oct 16 00:29:15 2026)"]
    );
}

#[test]
fn a_command_the_pane_host_cut_in_the_middle_of_a_password_is_kept_with_it_hidden() {
    // The host holds a command to 512 characters, its end marked: the password's
    // closing quote was past the cut.
    let command = format!(
        "{}--password=\"correct horse battery staple and the r…",
        "node deploy.mjs ".repeat(28)
    );
    let scene = Scene::new(&[]);
    let mut look = scene.look(ms(6, 0));
    look.windows.strangers.push(Stranger {
        process: Process { pid: 7777, command },
        pane: "p1-long-worker-1".to_owned(),
        since: ms(4, 0),
        began: "Fri Oct 16 00:29:15 2026".to_owned(),
    });
    let kept = Keeper::default().keep(&look, check(&look, &Default::default()));
    let row = &kept[0].evidence.rows[0];
    assert!(!row.contains("horse"), "{row}");
    assert!(row.contains("--password=\"[masked]"), "{row}");
}

#[test]
fn a_tree_the_pane_host_could_not_say_twice_and_processes_it_does_not_name_are_gaps() {
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let mut look = scene.look(ms(5, 0));
    let mut unread = window("long-worker", true, true);
    unread.tree_unread = Some("(could not be asked: timed out)".to_owned());
    look.windows.open.push(unread);
    let mut many = window("long-claude-2", true, true);
    many.background = (0..16)
        .map(|n| Process {
            pid: 9000 + n,
            command: "node worker.mjs".to_owned(),
        })
        .collect();
    many.background_total = 21;
    look.windows.open.push(many);
    let gaps = unjudged(&look);
    assert_eq!(
        gaps.iter().map(|gap| gap.key.as_str()).collect::<Vec<_>>(),
        [
            "6:tree:p1-long-worker-1:3",
            "6:unnamed:p1-long-claude-2-1:3"
        ]
    );
    assert!(gaps[1]
        .said
        .contains("runs 21 processes its turns started, and the pane host names 16"));
    // The unread tree counts at the second look in a row; the unnamed at once.
    let mut keeper = Keeper::default();
    let kept = keeper.keep_unjudged(look.at, gaps, &Names::default());
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].key, "6:unnamed:p1-long-claude-2-1:3");
    look.at = ms(5, 10);
    let kept = keeper.keep_unjudged(look.at, unjudged(&look), &Names::default());
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(
        kept[0].said.contains("could not be read"),
        "{}",
        kept[0].said
    );
    assert_eq!(keeper.unjudged.len(), 2);
    // A window whose exit was told is gone, and its tree no gap: its exit lists what
    // its close ended.
    let mut look = scene.look(ms(5, 0));
    let mut gone = window("long-worker", true, true);
    gone.tree_unread = Some("(the host said: no such pane)".to_owned());
    look.windows.open.push(gone);
    look.windows.exits.push(exit(ClosedBy::Daemon));
    assert!(unjudged(&look).is_empty());
}

#[test]
fn a_screen_line_that_quotes_a_skipped_call_is_none() {
    // A chief showing the words as a Markdown quote, or in quotes, skipped nothing.
    let scene = Scene::new(&[("long-worker", "devin")]);
    for quoted in [
        "> User skipped this tool call",
        "  ⎿ > User skipped this tool call",
        "\"User skipped this tool call\"",
        "│ “User skipped this tool call” │",
        "`User skipped this tool call`",
    ] {
        let mut look = scene.look(ms(2, 0));
        let mut shown = window("long-worker", true, true);
        shown.tail = vec![quoted.to_owned()];
        look.windows.open.push(shown);
        assert!(found(&look, 8).is_empty(), "{quoted}");
    }
    // The dialog's own line, in its box, is one.
    let mut look = scene.look(ms(2, 0));
    let mut shown = window("long-worker", true, true);
    shown.tail = vec!["│ ⎿  User skipped this tool call │".to_owned()];
    look.windows.open.push(shown);
    assert_eq!(found(&look, 8).len(), 1);
}

#[test]
fn a_process_the_system_could_not_be_asked_of_is_unresolved_and_a_break() {
    let scene = Scene::new(&[]);
    let mut look = scene.look(ms(6, 0));
    look.windows.unresolved.push(Unresolved {
        process: Process {
            pid: 7777,
            command: "node serve.mjs".to_owned(),
        },
        pane: "p1-long-worker-1".to_owned(),
        since: ms(4, 0),
        why: "the system could not be asked whether it still runs: ps ran out of its time"
            .to_owned(),
    });
    let breaks = found(&look, 6);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].key, "6:unresolved:7777");
    assert!(breaks[0].said.ends_with("could not be judged: the system could not be asked whether it still runs: ps ran out of its time"));
}

#[test]
fn a_tool_call_skipped_in_a_record_or_on_a_screen_breaks_the_eighth() {
    let scene = Scene::new(&[("long-worker", "devin")]);
    let conversation = scene.conversation(3, "devin", "devin-1");
    scene.item(
        conversation,
        0,
        "tool",
        "User skipped this tool call",
        true,
        &t(1, 0),
    );
    let breaks = found(&scene.look(ms(2, 0)), 8);
    assert_eq!(breaks.len(), 1);
    assert_eq!(
        breaks[0].said,
        "a tool call of @long-worker's came back \"User skipped this tool call\""
    );
    let scene = Scene::new(&[("long-worker", "devin")]);
    let mut look = scene.look(ms(2, 0));
    let mut shown = window("long-worker", true, true);
    shown.tail = vec![
        "● Edit(src/money.mjs)".to_owned(),
        "  ⎿ User skipped this tool call".to_owned(),
    ];
    look.windows.open.push(shown);
    assert_eq!(found(&look, 8).len(), 1);
    assert!(found(&scene.look(ms(2, 0)), 8).is_empty());
}

#[test]
fn a_skipped_call_is_found_in_any_case_and_a_quote_of_the_words_is_none() {
    // Devin's words in another case, where a tool's call comes back and on a screen.
    let scene = Scene::new(&[("long-worker", "devin")]);
    let conversation = scene.conversation(3, "devin", "devin-1");
    scene.item(
        conversation,
        0,
        "tool",
        "user skipped this tool call",
        true,
        &t(1, 0),
    );
    scene.item(
        conversation,
        1,
        "tool",
        "User Skipped This Tool Call.",
        true,
        &t(1, 5),
    );
    assert_eq!(found(&scene.look(ms(2, 0)), 8).len(), 2);
    let mut look = scene.look(ms(2, 0));
    let mut shown = window("long-worker", true, true);
    shown.tail = vec!["│ ⎿  USER SKIPPED THIS TOOL CALL │".to_owned()];
    look.windows.open.push(shown);
    assert_eq!(found(&look, 8).len(), 3);
    // The words quoted by the person, by the agent, or in a sentence on a screen.
    let scene = Scene::new(&[("long-worker", "devin")]);
    let conversation = scene.conversation(3, "devin", "devin-1");
    scene.item(
        conversation,
        0,
        "user",
        "[ConsensFlow m-9 · T-18 · note from @chief]\nIf you see \"User skipped this tool call\", say so.",
        true,
        &t(1, 0),
    );
    scene.item(
        conversation,
        1,
        "assistant",
        "No call came back User skipped this tool call this time.",
        true,
        &t(1, 5),
    );
    let mut look = scene.look(ms(2, 0));
    let mut shown = window("long-worker", true, true);
    shown.tail = vec!["> If you see \"User skipped this tool call\", say so.".to_owned()];
    look.windows.open.push(shown);
    assert!(found(&look, 8).is_empty());
}

fn activity(handle: &str, minute: u32, state: &str) -> Activity {
    Activity {
        at: ms(minute, 0),
        handle: handle.to_owned(),
        state: state.to_owned(),
        reason: (state == "waiting").then(|| "a permission dialog".to_owned()),
    }
}

#[test]
fn a_dialog_that_waits_past_its_time_breaks_the_eighth_and_one_answered_or_young_holds_it() {
    let scene = Scene::new(&[("long-worker", "devin")]);
    let mut look = scene.look(ms(10, 0));
    look.windows.open.push(window("long-worker", true, true));
    look.windows.activity = vec![
        activity("long-worker", 1, "working"),
        activity("long-worker", 3, "waiting"),
    ];
    let breaks = found(&look, 8);
    assert_eq!(breaks.len(), 1);
    assert_eq!(
        breaks[0].said,
        "a dialog has waited in @long-worker's window for 7 min (a permission dialog)"
    );
    look.at = ms(6, 0);
    assert!(found(&look, 8).is_empty(), "three minutes");
    look.at = ms(10, 0);
    look.windows
        .activity
        .push(activity("long-worker", 4, "working"));
    assert!(found(&look, 8).is_empty(), "answered");
}

#[test]
fn a_dialog_whose_window_closed_before_its_time_is_not_timed() {
    // The worker waited at 10:03 and its window was closed at 10:05: the product's
    // close says no change of state in the trace, and nothing waits after it.
    let scene = Scene::new(&[("long-worker", "devin")]);
    let mut look = scene.look(ms(10, 0));
    look.windows.activity = vec![
        activity("long-worker", 1, "working"),
        activity("long-worker", 3, "waiting"),
    ];
    let mut closed = exit(ClosedBy::Daemon);
    closed.at = ms(5, 0);
    look.windows.exits.push(closed.clone());
    assert!(found(&look, 8).is_empty(), "no window");
    // Opened again since, on a new generation: the dialog was the old window's.
    look.windows.open.push(window("long-worker", true, true));
    assert!(found(&look, 8).is_empty(), "the old window's");
    // A close before the dialog is none of it.
    look.windows.exits[0].at = ms(2, 0);
    assert_eq!(found(&look, 8).len(), 1);
}
