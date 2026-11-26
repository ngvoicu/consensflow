//! A line a person types into a Claude window that begins no turn (T-2330, T-2342): what the
//! window's look says of it. The records are what Claude Code 2.1.332 wrote in a live run for
//! `/usage`, `/help`, `/context` and `!echo hi` (`typed-line-*.jsonl`), after a turn that ended; the
//! borrowed shape of a listed command's echo and output (`/cost` and the like, not recorded)
//! holds the case the reader's closed list does not end.

use cf_harness::contract::Over;

use super::*;

const SESSION: &str = "c4b2d0e1-3a57-4f68-9b21-7e5d8a1f6c03";

fn line(n: u32, parent: Option<u32>, content: &str, meta: bool) -> String {
    record(
        SESSION,
        n,
        &json!({
            "type": "user",
            "isSidechain": false,
            "isMeta": meta,
            "parentUuid": parent.map(|parent| format!("{SESSION}-{parent}")),
            "message": { "role": "user", "content": content },
        }),
    )
}

/// A turn that ended: the user's `Hello`, the answer, the stop hooks' summary.
fn at_rest() -> Vec<String> {
    vec![
        line(1, None, "Hello", false),
        answer_line(SESSION, 2, "Hi"),
        stop_line(SESSION, 3),
    ]
}

/// A turn that ended, then a command outside the list typed at the idle prompt, in the shape
/// the listed commands were recorded in (the repository holds no recording of `/cost`): the
/// caveat, the echo, and what it printed.
fn typed() -> Vec<String> {
    let mut lines = at_rest();
    lines.extend([
        line(
            4,
            Some(3),
            "<local-command-caveat>The messages below were made by a command the user ran; do not answer them.</local-command-caveat>",
            true,
        ),
        line(
            5,
            Some(4),
            "<command-name>/cost</command-name>\n            <command-message>cost</command-message>\n            <command-args></command-args>",
            false,
        ),
        line(
            6,
            Some(5),
            "<local-command-stdout>Total cost: $0.00</local-command-stdout>",
            false,
        ),
    ]);
    lines
}

/// The turn that ended, then the records Claude Code 2.1.332 wrote for `/usage`, `/help` or
/// `/context` (a live run): the first's parent is the turn's last record.
fn recorded(name: &str) -> Vec<String> {
    let text = match name {
        "usage" => include_str!("../../fixtures/claude/typed-line-usage.jsonl"),
        "help" => include_str!("../../fixtures/claude/typed-line-help.jsonl"),
        "context" => include_str!("../../fixtures/claude/typed-line-context.jsonl"),
        "shell" => include_str!("../../fixtures/claude/typed-line-shell.jsonl"),
        other => panic!("no recording of /{other}"),
    };
    let mut lines = at_rest();
    for (at, text) in text.lines().enumerate() {
        let mut record: Value = serde_json::from_str(text).unwrap();
        record["sessionId"] = json!(SESSION);
        if at == 0 {
            record["parentUuid"] = json!(format!("{SESSION}-3"));
        }
        lines.push(format!("{record}\n"));
    }
    lines
}

fn look(window: &Rc<dyn Window>) -> cf_harness::contract::Observed {
    finished(window.observe()).unwrap()
}

/// Claude's own status says `idle` (its prompt is up and takes words), and the record holds
/// the turn the command opened: the look reads the window at work. Claude's status is no
/// second signal that ends it; nothing but a turn's end does.
#[test]
fn claudes_idle_status_does_not_end_a_turn_the_record_holds_open_after_such_a_command() {
    let home = Home::new();
    let lines = typed();
    home.transcript(SESSION, &lines[..1]);
    let window = window(&home.adapter(), &Request::resumed(SESSION));
    home.transcript(SESSION, &lines);
    home.status(SESSION, &json!({ "status": "idle" }), me());
    for _ in 0..3 {
        let observed = look(&window);
        assert!(!observed.settled, "the record holds the turn open");
        assert!(!observed.takes_words, "nothing is pasted into it");
        assert!(observed.at_prompt, "Claude's own word: its prompt is up");
        assert_eq!(observed.over, Over::No);
    }
}

/// The listed command in the same lines: at rest at its output, so the window takes words.
#[test]
fn the_same_lines_of_a_listed_command_leave_the_window_at_rest() {
    let home = Home::new();
    let lines: Vec<String> = typed()
        .into_iter()
        .map(|line| line.replace("/cost</command-name>", "/model</command-name>"))
        .collect();
    home.transcript(SESSION, &lines[..1]);
    let window = window(&home.adapter(), &Request::resumed(SESSION));
    home.transcript(SESSION, &lines);
    home.status(SESSION, &json!({ "status": "idle" }), me());
    let observed = look(&window);
    assert!(observed.settled && observed.takes_words);
}

/// What a look reads of the window with `lines` written and Claude's status idle.
fn window_after(lines: &[String]) -> cf_harness::contract::Observed {
    let home = Home::new();
    home.transcript(SESSION, &lines[..1]);
    let window = window(&home.adapter(), &Request::resumed(SESSION));
    home.transcript(SESSION, lines);
    home.status(SESSION, &json!({ "status": "idle" }), me());
    look(&window)
}

/// `/usage` and `/help` as Claude Code 2.1.332 wrote them: the system's echo and output, no
/// turn. The window is at rest and takes words, with no entry in the reader's list.
#[test]
fn a_recorded_usage_or_help_leaves_the_window_at_rest_and_taking_words() {
    for name in ["usage", "help"] {
        let observed = window_after(&recorded(name));
        assert!(observed.settled && observed.takes_words, "/{name}");
        assert_eq!(observed.over, Over::No, "/{name}");
    }
}

/// `/context` as Claude Code 2.1.332 wrote it: the echo and output, then a meta `user`
/// record with the report for the model's next turn, and no model call. The record asks
/// nothing of the model: the window is at rest and takes words, with Claude idle at its
/// prompt.
#[test]
fn a_recorded_context_leaves_the_window_at_rest_and_taking_words() {
    let lines = recorded("context");
    assert!(lines
        .iter()
        .all(|line| !line.contains("\"type\":\"assistant\"")
            || line.contains(&format!("{SESSION}-2"))));
    let observed = window_after(&lines);
    assert!(observed.settled && observed.takes_words);
    assert!(observed.at_prompt);
    assert_eq!(observed.over, Over::No);
}

/// The gap after such a report, if the model is asked after it: nothing is written until the
/// model's first record, and Claude's own status says `busy` from the moment it asks (the
/// status is the second word the look takes before it says a window takes words). The window
/// takes no words then, though the transcript alone reads at rest.
#[test]
fn claudes_busy_status_covers_the_gap_before_the_models_first_record() {
    let home = Home::new();
    let lines = recorded("context");
    home.transcript(SESSION, &lines[..1]);
    let window = window(&home.adapter(), &Request::resumed(SESSION));
    home.transcript(SESSION, &lines);
    home.status(SESSION, &json!({ "status": "busy" }), me());
    let observed = look(&window);
    assert!(!observed.takes_words, "Claude says it works");
    assert!(!observed.at_prompt);
}

/// `!echo hi` as Claude Code 2.1.332 wrote it: the shell line and its output as `user`
/// records, then the model's answer to them and the turn's end. At the end the window is at
/// rest and takes words; with the answer not yet written it reads at work.
#[test]
fn a_recorded_shell_line_is_a_turn_that_ends_with_the_models_answer() {
    let lines = recorded("shell");
    let answered = lines
        .iter()
        .rposition(|line| line.contains("\"type\":\"assistant\""))
        .unwrap();
    let observed = window_after(&lines[..answered]);
    assert!(!observed.settled && !observed.takes_words);
    let observed = window_after(&lines);
    assert!(observed.settled && observed.takes_words);
    assert_eq!(observed.over, Over::No);
}

/// An empty Enter at an idle prompt: Claude Code 2.1.332 wrote no record (the screen drew
/// something), so the window stays at rest and takes words.
#[test]
fn an_empty_enter_writes_nothing_and_leaves_the_window_at_rest() {
    let observed = window_after(&at_rest());
    assert!(observed.settled && observed.takes_words);
}
