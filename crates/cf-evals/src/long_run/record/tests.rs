//! Claude Code's record, read as the watch reads it: the shapes of 2026-10-15's
//! field reports, written as Claude Code writes them.

use serde_json::{json, Value};

use super::*;

/// A line of the record at `second` past 10:00 of 2026-10-15, with `fields`.
fn line(second: u32, fields: Value) -> String {
    let mut line =
        json!({ "timestamp": format!("2026-10-15T10:{:02}:{:02}.000Z", second / 60, second % 60) });
    if let (Some(line), Some(fields)) = (line.as_object_mut(), fields.as_object()) {
        line.extend(fields.clone());
    }
    line.to_string()
}

fn at(second: u32) -> i64 {
    clock::millis(&format!(
        "2026-10-15T10:{:02}:{:02}.000Z",
        second / 60,
        second % 60
    ))
    .unwrap()
}

fn prompt(second: u32, text: &str) -> String {
    line(
        second,
        json!({ "type": "user", "message": { "role": "user", "content": text } }),
    )
}

fn answer(second: u32, text: &str, stop: &str) -> String {
    line(
        second,
        json!({ "type": "assistant", "message": { "role": "assistant", "stop_reason": stop,
            "content": [{ "type": "text", "text": text }] } }),
    )
}

/// The root `turn_duration` Claude Code writes once a turn's stop hooks have run.
fn duration(second: u32) -> String {
    line(
        second,
        json!({ "type": "system", "subtype": "turn_duration", "isSidechain": false,
            "durationMs": 1000, "messageCount": 4 }),
    )
}

fn record(lines: &[String]) -> Record {
    Record::claude(&lines.join("\n"))
}

#[test]
fn a_turn_is_at_work_from_its_prompt_and_at_rest_from_its_end() {
    let record = record(&[
        prompt(0, "[ConsensFlow m-1 · T-1 · task from @chief]\nBuild it"),
        answer(5, "Building.", "tool_use"),
        line(
            6,
            json!({ "type": "assistant", "message": { "stop_reason": "tool_use",
            "content": [{ "type": "tool_use", "name": "Bash", "input": {} }] } }),
        ),
        line(
            9,
            json!({ "type": "user", "message": { "role": "user",
            "content": [{ "type": "tool_result", "content": "ok" }] } }),
        ),
        answer(12, "Built.", "end_turn"),
        duration(12),
    ]);
    assert_eq!(record.stood_at(at(3)).unwrap().0, Stood::InFlight);
    assert_eq!(record.stood_at(at(10)).unwrap().0, Stood::InFlight);
    assert_eq!(record.stood_at(at(13)).unwrap().0, Stood::AtRest);
    assert!(Record::default().stood_at(at(13)).is_none());
    assert_eq!(record.last_said(), Some(at(12)));
}

#[test]
fn an_idle_compaction_is_no_turn_and_leaves_a_turn_that_had_ended_at_rest() {
    // The A5 shape: a turn ends, Claude Code compacts the idle conversation (a
    // boundary and a summary that reads as the user's), and no turn runs.
    let record = record(&[
        prompt(0, "Plan the work."),
        answer(20, "Planned.", "end_turn"),
        duration(20),
        line(
            300,
            json!({ "type": "system", "subtype": "compact_boundary", "content": "Conversation compacted" }),
        ),
        line(
            301,
            json!({ "type": "user", "isCompactSummary": true, "isVisibleInTranscriptOnly": true,
            "message": { "role": "user", "content": "This session is being continued from a previous conversation." } }),
        ),
    ]);
    let (stood, last) = record.stood_at(at(400)).unwrap();
    assert_eq!(stood, Stood::AtRest, "{last:?}");
    assert_eq!(
        record
            .entries
            .iter()
            .filter(|entry| entry.kind == Kind::Compaction)
            .count(),
        2
    );
    // A prompt that came after the compaction and was not answered is a turn at work.
    let mut cut = record.clone();
    cut.entries
        .extend(Record::claude(&prompt(500, "Go on.")).entries);
    assert_eq!(cut.stood_at(at(600)).unwrap().0, Stood::InFlight);
}

#[test]
fn a_background_job_is_pending_from_its_start_until_claude_codes_own_notification_names_it() {
    // The A2 shape: `run_in_background`, the turn ends, and the job's end wakes a turn.
    let started = line(
        10,
        json!({ "type": "user", "toolUseResult": { "backgroundTaskId": "b7x2" },
            "message": { "role": "user", "content": [{ "type": "tool_result",
            "content": "Command running in background with ID: b7x2. Output is being written to: /tmp/b7x2.output" }] } }),
    );
    let quoted = prompt(
        40,
        "[ConsensFlow m-9 · note from @chief]\nIt said <task-notification><task-id>b7x2</task-id></task-notification> once.",
    );
    let queued = line(
        60,
        json!({ "type": "queue-operation", "operation": "enqueue",
            "content": "<task-notification>\n<task-id>b7x2</task-id>\n<status>completed</status>\n</task-notification>" }),
    );
    let woken = line(
        61,
        json!({ "type": "user", "turnOrigin": "task_notification", "message": { "role": "user",
            "content": "<task-notification>\n<task-id>b7x2</task-id>\n<status>completed</status>\n</task-notification>" } }),
    );
    let record = record(&[
        started,
        answer(12, "Running.", "end_turn"),
        quoted,
        queued,
        woken,
    ]);
    assert_eq!(record.jobs_pending_at(at(11)).len(), 1);
    assert_eq!(
        record.jobs_pending_at(at(11))[0].kind,
        Kind::JobStarted("b7x2".to_owned())
    );
    // A message that quotes a notification is no notification.
    assert_eq!(record.jobs_pending_at(at(50)).len(), 1);
    assert!(record.jobs_pending_at(at(60)).is_empty());
    assert!(record.jobs_pending_at(at(5)).is_empty());
}

#[test]
fn a_command_moved_to_the_background_names_its_job_in_the_words_of_its_result() {
    assert_eq!(
        jobs_started_in("Command running in background with ID: a34b16"),
        ["a34b16"]
    );
    assert_eq!(
        jobs_started_in("Command running in the background with ID:fd8bf7 (still going)"),
        ["fd8bf7"]
    );
    assert!(jobs_started_in("the build ran in the background").is_empty());
    assert_eq!(
        jobs_ended_in("<task-notification>\n<task-id> t1 </task-id></task-notification><task-notification><task-id>t2</task-id>"),
        ["t1", "t2"]
    );
    assert!(jobs_ended_in("<task-id>t1</task-id>").is_empty());
}

#[test]
fn an_interrupt_leaves_the_turn_at_rest_and_a_line_that_is_no_json_is_none() {
    let record = Record::claude(&format!(
        "{}\nnot json\n{}",
        prompt(0, "Work."),
        prompt(9, "[Request interrupted by user]")
    ));
    assert_eq!(record.entries.len(), 2);
    assert_eq!(record.stood_at(at(10)).unwrap().0, Stood::AtRest);
    let lines = record.lines_by(at(10), 1);
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].starts_with("2026-10-15T10:00:09.000Z Interrupted:"),
        "{lines:?}"
    );
}

#[test]
fn an_answer_is_no_end_of_its_turn_until_its_stop_hooks_have_run_and_one_that_prevented_it_ends_nothing(
) {
    // The answer is written; a stop hook that runs for a minute keeps the turn at work.
    let answered = [prompt(0, "Fix it."), answer(10, "Fixed.", "end_turn")];
    assert_eq!(
        record(&answered).stood_at(at(60)).unwrap().0,
        Stood::InFlight
    );
    // A hook's summary that prevented continuation: the agent goes on.
    let prevented = line(
        70,
        json!({ "type": "system", "subtype": "stop_hook_summary", "preventedContinuation": true }),
    );
    let mut lines = answered.to_vec();
    lines.push(prevented);
    assert_eq!(record(&lines).stood_at(at(80)).unwrap().0, Stood::InFlight);
    // One that did not: the turn is over.
    lines.push(line(
        90,
        json!({ "type": "system", "subtype": "stop_hook_summary", "preventedContinuation": false }),
    ));
    let replayed = record(&lines);
    let (stood, entry) = replayed.stood_at(at(95)).unwrap();
    assert_eq!((stood, entry.at), (Stood::AtRest, Some(at(90))));
}

#[test]
fn a_duration_of_a_sidechain_or_with_no_counts_or_with_no_answer_before_it_ends_no_turn() {
    let answered = [prompt(0, "Fix it."), answer(10, "Fixed.", "end_turn")];
    for duration in [
        json!({ "type": "system", "subtype": "turn_duration", "isSidechain": true, "durationMs": 9, "messageCount": 2 }),
        // The product's root is a record that says it is none of a sidechain's.
        json!({ "type": "system", "subtype": "turn_duration", "durationMs": 9, "messageCount": 2 }),
        json!({ "type": "system", "subtype": "turn_duration", "isSidechain": false, "durationMs": 9 }),
        json!({ "type": "system", "subtype": "turn_duration", "isSidechain": false, "durationMs": 9, "messageCount": 2, "pendingWorkflowCount": "two" }),
    ] {
        let mut lines = answered.to_vec();
        lines.push(line(20, duration.clone()));
        assert_eq!(
            record(&lines).stood_at(at(30)).unwrap().0,
            Stood::InFlight,
            "{duration}"
        );
    }
    // A boundary with no answer of the turn before it: the agent called a tool last.
    let tooled = record(&[
        prompt(0, "Fix it."),
        line(
            5,
            json!({ "type": "assistant", "message": { "stop_reason": "tool_use",
            "content": [{ "type": "tool_use", "name": "Bash" }] } }),
        ),
        duration(9),
    ]);
    assert_eq!(tooled.stood_at(at(30)).unwrap().0, Stood::InFlight);
    // A sidechain's own lines are none of the main turn's.
    let side = record(&[
        prompt(0, "Fix it."),
        answer(10, "Fixed.", "end_turn"),
        duration(11),
        line(
            12,
            json!({ "type": "user", "isSidechain": true, "message": { "role": "user", "content": "subagent work" } }),
        ),
    ]);
    assert_eq!(side.stood_at(at(30)).unwrap().0, Stood::AtRest);
}

#[test]
fn a_record_ends_in_a_compaction_until_a_turn_comes_after_it() {
    let compacted = [
        prompt(0, "Plan it."),
        answer(10, "Planned.", "end_turn"),
        duration(11),
        line(
            20,
            json!({ "type": "system", "subtype": "compact_boundary" }),
        ),
        line(
            21,
            json!({ "type": "user", "isCompactSummary": true, "message": { "role": "user", "content": "This session is being continued." } }),
        ),
    ];
    assert!(record(&compacted).ends_in_compaction());
    assert!(!record(&compacted[..3]).ends_in_compaction());
    let mut went_on = compacted.to_vec();
    went_on.push(prompt(30, "Go on."));
    assert!(!record(&went_on).ends_in_compaction());
}

/// A user's record `uuid`, the child of `parent`, saying `text`, with `fields`.
fn said_by_claude(second: u32, uuid: &str, parent: &str, text: &str, fields: Value) -> String {
    let mut record = json!({ "type": "user", "uuid": uuid, "parentUuid": parent, "isSidechain": false,
        "message": { "role": "user", "content": text } });
    if let (Some(record), Some(fields)) = (record.as_object_mut(), fields.as_object()) {
        record.extend(fields.clone());
    }
    line(second, record)
}

/// A `/compact` typed into an idle conversation, as Claude Code 2.1.303 writes it (the
/// long run of 2026-10-19, whose chief read at work after it): the bare command as it is
/// typed, then, once it compacted, the boundary, the summary, the words before the
/// command, the command, and what it printed (`printed`, a child of the command).
fn compact_typed(printed: String) -> Vec<String> {
    vec![
        prompt(0, "Plan the work."),
        answer(20, "Planned.", "end_turn"),
        duration(20),
        said_by_claude(30, "c0", "d1", "/compact", json!({})),
        line(
            150,
            json!({ "type": "system", "subtype": "compact_boundary", "uuid": "b1", "parentUuid": null }),
        ),
        said_by_claude(
            150,
            "s1",
            "b1",
            "This session is being continued.",
            json!({ "isCompactSummary": true }),
        ),
        said_by_claude(
            150,
            "k1",
            "s1",
            "<local-command-caveat>Run by the user.</local-command-caveat>",
            json!({ "isMeta": true }),
        ),
        said_by_claude(
            150,
            "n1",
            "k1",
            "<command-name>/compact</command-name>\n<command-message>compact</command-message>\n<command-args></command-args>",
            json!({}),
        ),
        printed,
        line(
            151,
            json!({ "type": "attachment", "uuid": "t1", "parentUuid": "o1",
                "attachment": { "type": "hook_success", "hookEvent": "SessionStart" } }),
        ),
    ]
}

/// A command the system writes whole, its own record and what it printed (2.1.282's
/// `/plugin`, `/mcp` dismissed), `name` and `parent` the command's.
fn written_whole(second: u32, name: &str, parent: &str) -> [String; 2] {
    let system = |uuid: &str, parent: &str, content: &str| {
        line(
            second,
            json!({ "type": "system", "subtype": "local_command", "uuid": uuid,
                "parentUuid": parent, "isMeta": false, "level": "info", "isSidechain": false,
                "content": content }),
        )
    };
    [
        system(
            "w1",
            parent,
            &format!("<command-name>/{name}</command-name>"),
        ),
        system(
            "w2",
            "w1",
            "<local-command-stdout>Dialog dismissed</local-command-stdout>",
        ),
    ]
}

#[test]
fn a_command_the_system_writes_whole_after_a_compaction_opens_no_turn_and_the_record_ends_in_it() {
    // Astra's look at compact-rest round 2: its output, unmatched, read as a prompt, a
    // phantom turn.
    let printed = said_by_claude(
        150,
        "o1",
        "n1",
        "<local-command-stdout>Compacted</local-command-stdout>",
        json!({}),
    );
    let mut lines = compact_typed(printed);
    lines.extend(written_whole(200, "mcp", "t1"));
    let read = record(&lines);
    assert_eq!(read.stood_at(at(300)).unwrap().0, Stood::AtRest);
    assert!(read.ends_in_compaction());
    // And with no compaction before it, a turn that ended stays at rest.
    let mut answered = vec![
        prompt(0, "Plan it."),
        answer(10, "Planned.", "end_turn"),
        duration(11),
    ];
    answered.extend(written_whole(20, "plugin", "d1"));
    let read = record(&answered);
    assert_eq!(read.stood_at(at(30)).unwrap().0, Stood::AtRest);
    assert!(!read.ends_in_compaction());
}

#[test]
fn a_compact_typed_at_the_prompt_ends_at_what_it_printed_and_the_record_ends_in_its_compaction() {
    let printed = |parent: &str, text: &str| said_by_claude(150, "o1", parent, text, json!({}));
    let compacted = record(&compact_typed(printed(
        "n1",
        "<local-command-stdout>Compacted</local-command-stdout>",
    )));
    assert_eq!(
        compacted.stood_at(at(60)).unwrap().0,
        Stood::InFlight,
        "it compacts"
    );
    let (stood, last) = compacted.stood_at(at(200)).unwrap();
    assert_eq!(stood, Stood::AtRest, "{last:?}");
    assert_eq!(last.kind, Kind::Output);
    assert!(compacted.ends_in_compaction());
    // An error the command printed ends it too.
    let failed = printed(
        "n1",
        "<local-command-stderr>Not compacted</local-command-stderr>",
    );
    assert_eq!(
        record(&compact_typed(failed)).stood_at(at(200)).unwrap().0,
        Stood::AtRest
    );
    // A prompt after it is a turn at work, and the record no longer ends in the compaction.
    let mut went_on = compact_typed(printed(
        "n1",
        "<local-command-stdout>Compacted</local-command-stdout>",
    ));
    went_on.push(prompt(300, "Go on."));
    assert_eq!(
        record(&went_on).stood_at(at(400)).unwrap().0,
        Stood::InFlight
    );
    assert!(!record(&went_on).ends_in_compaction());
    // What is printed of no command that comes just before it ends nothing.
    let astray = record(&compact_typed(printed(
        "k1",
        "<local-command-stdout>Compacted</local-command-stdout>",
    )));
    assert_eq!(astray.stood_at(at(200)).unwrap().0, Stood::InFlight);
    let mut after_a_prompt = compact_typed(printed("n1", "x"));
    after_a_prompt.truncate(3);
    after_a_prompt.extend([
        said_by_claude(30, "u2", "d1", "Hello again", json!({})),
        said_by_claude(
            31,
            "o1",
            "u2",
            "<local-command-stdout>x</local-command-stdout>",
            json!({}),
        ),
    ]);
    assert_eq!(
        record(&after_a_prompt).stood_at(at(60)).unwrap().0,
        Stood::InFlight
    );
}

#[test]
fn a_compact_that_found_nothing_to_compact_ends_at_what_the_system_says_it_printed() {
    // Claude Code 2.1.304 (the live run of 2026-10-20): the words before the command, the
    // command, and what it printed as the system's record; no bare command, no compaction.
    let printed = |fields: Value| {
        let mut record = json!({ "type": "system", "subtype": "local_command", "uuid": "o1",
            "parentUuid": "n1", "isMeta": false, "level": "info", "isSidechain": false,
            "content": "<local-command-stdout>Nothing to compact yet.</local-command-stdout>" });
        if let (Some(record), Some(fields)) = (record.as_object_mut(), fields.as_object()) {
            record.extend(fields.clone());
        }
        line(40, record)
    };
    let refused = |output: String| {
        record(&[
            prompt(0, "Plan the work."),
            answer(20, "Planned.", "end_turn"),
            duration(20),
            said_by_claude(
                30,
                "k1",
                "d1",
                "<local-command-caveat>Run by the user.</local-command-caveat>",
                json!({ "isMeta": true }),
            ),
            said_by_claude(
                30,
                "n1",
                "k1",
                "<command-name>/compact</command-name>\n<command-message>compact</command-message>\n<command-args></command-args>",
                json!({}),
            ),
            output,
        ])
    };
    let read = refused(printed(json!({})));
    assert_eq!(read.stood_at(at(35)).unwrap().0, Stood::InFlight, "it runs");
    let (stood, last) = read.stood_at(at(60)).unwrap();
    assert_eq!(stood, Stood::AtRest, "{last:?}");
    assert_eq!(last.kind, Kind::Printed);
    assert!(!read.ends_in_compaction(), "nothing was compacted");
    // A system record of another kind, a meta one, or one that printed no output ends nothing.
    for (why, fields) in [
        ("another subtype", json!({ "subtype": "away_summary" })),
        ("a meta record", json!({ "isMeta": true })),
        ("a warning", json!({ "level": "warning" })),
        ("no output", json!({ "content": "Nothing to compact yet." })),
        ("of no command", json!({ "parentUuid": "k1" })),
    ] {
        assert_eq!(
            refused(printed(fields)).stood_at(at(60)).unwrap().0,
            Stood::InFlight,
            "{why}"
        );
    }
}

/// A summary-only compaction (2026-10-15's), then a command and what it printed.
fn compacted_then(command: &str) -> Vec<String> {
    vec![
        prompt(0, "Plan it."),
        answer(10, "Planned.", "end_turn"),
        duration(11),
        line(
            20,
            json!({ "type": "system", "subtype": "compact_boundary", "uuid": "b1" }),
        ),
        said_by_claude(
            21,
            "s1",
            "b1",
            "This session is being continued.",
            json!({ "isCompactSummary": true }),
        ),
        said_by_claude(
            30,
            "k1",
            "s1",
            "<local-command-caveat>Run by the user.</local-command-caveat>",
            json!({ "isMeta": true }),
        ),
        said_by_claude(30, "n1", "k1", command, json!({})),
        said_by_claude(
            31,
            "o1",
            "n1",
            "<local-command-stdout>Done</local-command-stdout>",
            json!({}),
        ),
    ]
}

#[test]
fn a_command_that_may_go_on_to_the_model_ends_no_turn_and_no_compaction_is_the_last_thing_after_it()
{
    // Sol's look at round 1: 2.1.303's `/plan <description>` prints, then asks the model.
    let plan = "<command-name>/plan</command-name>\n<command-message>plan</command-message>\n<command-args>a plan</command-args>";
    let planned = record(&compacted_then(plan));
    assert_eq!(planned.stood_at(at(60)).unwrap().0, Stood::InFlight);
    assert!(!planned.ends_in_compaction());
    // A name that only begins as a quiet one's is none of them.
    let compacted_more = record(&compacted_then("<command-name>/compacted</command-name>"));
    assert_eq!(compacted_more.stood_at(at(60)).unwrap().0, Stood::InFlight);
    assert!(!compacted_more.ends_in_compaction());
    // One that runs no model leaves the record at rest, and ending in the compaction.
    let model = record(&compacted_then(
        "<command-name>/model</command-name>\n<command-args>haiku</command-args>",
    ));
    assert_eq!(model.stood_at(at(60)).unwrap().0, Stood::AtRest);
    assert!(model.ends_in_compaction());
}

#[test]
fn a_refusal_of_the_provider_ends_the_turn_natively() {
    let refused = record(&[
        prompt(0, "Fix it."),
        line(
            5,
            json!({ "type": "assistant", "isApiErrorMessage": true,
            "message": { "content": [{ "type": "text", "text": "API Error: 529 overloaded" }] } }),
        ),
    ]);
    assert_eq!(refused.stood_at(at(10)).unwrap().0, Stood::AtRest);
}

fn queue(second: u32, operation: &str, content: &str) -> String {
    line(
        second,
        json!({ "type": "queue-operation", "operation": operation, "content": content }),
    )
}

#[test]
fn a_message_waiting_in_claudes_queue_is_a_turn_not_over() {
    // The turn ended at its boundary; then ConsensFlow's next message went into the
    // queue. A restart now cuts what the queue holds: a cut note is right.
    let answered = [
        prompt(0, "Plan it."),
        answer(10, "Planned.", "end_turn"),
        duration(11),
    ];
    let mut lines = answered.to_vec();
    lines.push(queue(
        20,
        "enqueue",
        "[ConsensFlow m-9 · result from @w]\nDone.",
    ));
    assert_eq!(record(&lines).stood_at(at(30)).unwrap().0, Stood::InFlight);
    // Taken for a turn, which begins with it, answers and ends: at rest.
    lines.extend([
        queue(31, "dequeue", ""),
        line(
            32,
            json!({ "type": "user", "promptSource": "queued",
                    "message": { "role": "user", "content": "[ConsensFlow m-9 · result from @w]\nDone." } }),
        ),
        answer(40, "Accepted.", "end_turn"),
        duration(41),
    ]);
    assert_eq!(record(&lines).stood_at(at(50)).unwrap().0, Stood::AtRest);
    // Forgotten before it was taken: nothing waits.
    let mut removed = answered.to_vec();
    removed.extend([
        queue(20, "enqueue", "never mind"),
        queue(21, "remove", "never mind"),
    ]);
    assert_eq!(record(&removed).stood_at(at(30)).unwrap().0, Stood::AtRest);
}

#[test]
fn a_removal_names_the_queued_message_whose_envelope_has_other_attributes() {
    // The reviewers' case, as the product reads it (`same_queued_content`): enqueued
    // with an attribute its removal does not say.
    let answered = [
        prompt(0, "Plan it."),
        answer(10, "Planned.", "end_turn"),
        duration(11),
    ];
    let mut removed = answered.to_vec();
    removed.extend([
        queue(20, "enqueue", r#"<m a="1">x</m>"#),
        queue(21, "remove", "<m>x</m>"),
    ]);
    assert_eq!(record(&removed).stood_at(at(30)).unwrap().0, Stood::AtRest);
    // Taken back by `popAll` so named, then sent and answered: at rest.
    let mut popped = answered.to_vec();
    popped.extend([
        queue(20, "enqueue", r#"<m a="1" hop-chain="s-2">x</m>"#),
        queue(21, "popAll", "<m>x</m>"),
        answer(25, "Noted.", "end_turn"),
        duration(26),
    ]);
    assert_eq!(record(&popped).stood_at(at(30)).unwrap().0, Stood::AtRest);
    // Another message, or no envelope, names nothing.
    let mut other = answered.to_vec();
    other.extend([
        queue(20, "enqueue", r#"<m a="1">x</m>"#),
        queue(21, "remove", "<m>y</m>"),
    ]);
    assert_eq!(record(&other).stood_at(at(30)).unwrap().0, Stood::InFlight);
    assert!(same_queued_content("<m a=\"1\">x</m>", "<m>x</m>"));
    assert!(!same_queued_content(" <m a=\"1\">x</m>", " <m>x</m>"));
}

#[test]
fn an_entrys_words_are_masked_whole_before_they_are_cut_and_matched_whole() {
    // A key whose value runs past what an entry keeps: cut first, seven characters of
    // it are left, which is no secret's shape, and they would be kept.
    let words = format!("{}xtoken=abcd1234efgh5678ijkl9012", "a ".repeat(293));
    assert_eq!(words.find("abcd"), Some(593));
    let names = Names::default();
    let mut record = Record::default();
    record.add_claude(&prompt(0, &words), &names);
    record.add_claude(&queue(5, "enqueue", &words), &names);
    record.add_claude(&queue(6, "remove", &words), &names);
    let entry = &record.entries[0];
    assert!(!entry.text.contains("abcd"), "{}", entry.text);
    assert!(entry.text.contains("xtoken=[mas"), "{}", entry.text);
    // What the queue's operations say is matched whole: the removal names the message.
    assert_eq!(record.stood_at(at(0)).unwrap().0, Stood::InFlight);
    record.add_claude(&answer(10, "Done.", "end_turn"), &names);
    record.add_claude(&duration(11), &names);
    assert_eq!(record.stood_at(at(20)).unwrap().0, Stood::AtRest);
}

#[test]
fn the_products_late_ancestor_fixture_reads_at_rest_as_the_product_reads_it() {
    // Claude Code 2.1.268 wrote the turn's prompt after its answer and its boundary:
    // the answer's parents, attachments all, lead to it. It begins no turn. A
    // capture of 2.1.268; no later version, up to 2.1.305, was seen to write an
    // answer whose parent comes after it.
    let fixture = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/engine/fixtures/completion/claude-code/v268-late-ancestors.jsonl"
    ))
    .unwrap();
    let record = Record::claude(&fixture);
    let (stood, _) = record.stood_at(i64::MAX).unwrap();
    assert_eq!(stood, Stood::AtRest);
    // A prompt that is no ancestor of the answer, after the same boundary, is a turn.
    let mut later = fixture.clone();
    later.push_str(
        &json!({ "type": "user", "uuid": "u-later", "parentUuid": "81d44d37-e4a2-4ae4-bc8c-31fda10d89ac",
                 "isSidechain": false, "timestamp": "2026-09-11T14:01:00.000Z",
                 "message": { "role": "user", "content": "And now?" } })
        .to_string(),
    );
    assert_eq!(
        Record::claude(&later).stood_at(i64::MAX).unwrap().0,
        Stood::InFlight
    );
}

#[test]
fn a_users_record_with_no_words_begins_no_turn() {
    let lines = [
        prompt(0, "Plan it."),
        answer(10, "Planned.", "end_turn"),
        duration(11),
        line(
            20,
            json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "image" }] } }),
        ),
    ];
    assert_eq!(record(&lines).stood_at(at(30)).unwrap().0, Stood::AtRest);
}

#[test]
fn a_summary_of_a_compaction_is_known_by_its_records_flag_and_never_by_its_words() {
    let words = "This session is being continued from a previous conversation.";
    let record = record(&[
        line(
            1,
            json!({ "type": "user", "uuid": "u-flagged", "isCompactSummary": true,
                "message": { "role": "user", "content": words } }),
        ),
        line(
            2,
            json!({ "type": "user", "uuid": "u-words",
                "message": { "role": "user", "content": words } }),
        ),
        line(
            3,
            json!({ "type": "assistant", "uuid": "a-flagged", "isCompactSummary": true,
                "message": { "role": "assistant", "content": [{ "type": "text", "text": words }] } }),
        ),
    ]);
    assert!(record.is_summary("u-flagged"));
    assert!(!record.is_summary("u-words"));
    assert!(!record.is_summary("a-flagged"));
    assert!(!record.is_summary("u-none"));
}

#[test]
fn the_rows_claude_code_writes_of_a_request_are_no_prompt() {
    // Claude Code 2.1.303 writes beside the prompt rows that echo the request it sent:
    // `api-request-blob` with the prompt as the user's message (made-up words here),
    // `api-request`, and `last-prompt`. None is a prompt, nor anything else.
    let header = "[ConsensFlow m-13 · note from @human]\nDo you still have work to do?";
    let echoes = [
        json!({ "type": "api-request-blob", "sessionId": "s-1", "hash": "h-1",
            "message": { "role": "user", "content": header } }),
        json!({ "type": "api-request", "sessionId": "s-1",
            "message": { "role": "user", "content": [{ "type": "text", "text": header }] } }),
        json!({ "type": "last-prompt", "sessionId": "s-1", "lastPrompt": header }),
    ];
    for echo in &echoes {
        assert!(
            record(&[line(0, echo.clone())]).entries.is_empty(),
            "{echo}"
        );
    }
    let mut lines = vec![answer(5, "Done.", "end_turn"), duration(5)];
    lines.extend(echoes.iter().map(|echo| line(9, echo.clone())));
    assert_eq!(record(&lines).stood_at(at(30)).unwrap().0, Stood::AtRest);
}

#[test]
fn the_agents_a_turn_left_are_that_turns_and_the_next_turn_counts_none_of_them() {
    let ended_with_agents = line(
        11,
        json!({ "type": "system", "subtype": "turn_duration", "isSidechain": false,
                "durationMs": 1000, "messageCount": 4, "pendingBackgroundAgentCount": 2 }),
    );
    let mut lines = vec![
        prompt(0, "Run the audit."),
        answer(10, "Agents started.", "end_turn"),
        ended_with_agents,
    ];
    assert_eq!(record(&lines).agents_pending_at(at(20)), 2);
    // The next turn ends at a stop hook's summary, which says nothing of agents.
    lines.extend([
        prompt(30, "And the docs?"),
        answer(40, "Done.", "end_turn"),
        line(
            41,
            json!({ "type": "system", "subtype": "stop_hook_summary", "preventedContinuation": false }),
        ),
    ]);
    assert_eq!(record(&lines).agents_pending_at(at(50)), 0);
    // While it ran, before its end, none were its own either.
    assert_eq!(record(&lines).agents_pending_at(at(35)), 0);
}
