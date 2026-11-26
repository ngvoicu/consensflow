//! What a reader leaves in view of an earlier turn's refusal decides whether the dispatcher
//! holds the member and pauses its task (R1, T-1910, T-1932). Each case writes a native
//! record, reads it as the daemon does, gives the stand-in window what the reader saw, and
//! runs one pass. A refusal stays in view unless a new prompt of the person's, written after
//! it on the same branch, is the record's evidence.
use cf_base::env::Env;
use cf_engine::testing::{Context, Made};
use cf_harness::records::{self, Options, Reading, Settlement};
use cf_proto::agents::Harness;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

const SESSION: &str = "33383216-87a0-4e6d-a273-07c4b229cdb1";
const NOW: &str = "2026-11-21T12:00:00.000Z";
const RESET: i64 = 1_893_456_000;

/// What one pass does with a member whose record is `records`: the task's state and whether
/// the member is held.
fn outcome(kind: &str, records: &[Value]) -> (String, bool) {
    let dir = tempfile::tempdir().unwrap();
    let tmp = dir.path();
    let file = match kind {
        "claude-code" => tmp.join("projects").join(format!("{SESSION}.jsonl")),
        "pi" => tmp.join("pi").join(format!("{SESSION}.jsonl")),
        _ => tmp
            .join("sessions")
            .join(format!("rollout-{SESSION}.jsonl")),
    };
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let lines: String = records.iter().map(|record| format!("{record}\n")).collect();
    fs::write(&file, lines).unwrap();
    let env = Env::from_vars([
        ("CLAUDE_CONFIG_DIR", tmp.to_path_buf()),
        ("CODEX_HOME", tmp.to_path_buf()),
        ("PI_CODING_AGENT_SESSION_DIR", tmp.join("pi")),
    ]);
    let harness = Harness::from_kind(kind).unwrap();
    let reading = records::answers(
        harness,
        SESSION,
        &env,
        &Options::default(),
        &records::zone("UTC").unwrap(),
        2_000_000_000_000,
    );
    let Reading::Known(read) = &*reading else {
        panic!("record unreadable: {reading:?}")
    };
    // What a reader makes of a record is held at its first refusal here: the tries a refused
    // login gets first (`login_retries`) are not this case's point.
    let c = Context::made(Made {
        harnesses: vec!["claude-code", "pi", "codex"],
        turn_retry_ms: 0,
        ..Made::default()
    });
    let project = c
        .open_project(json!({
            "directory": "/work/app", "name": "Refusal readers",
            "chief": { "harness": "claude-code", "model": "claude-opus-5" },
            "staff": [{ "agent": "zeus", "harness": kind, "role": "worker", "tier": "standard" }],
        }))
        .unwrap()
        .unwrap();
    c.give(project.id, "zeus", "Read the record");
    for _ in 0..5 {
        c.pass().unwrap();
    }
    let working = c.task(project.id, 1).task;
    assert_eq!(working.state, "working");
    let window = working.assignee.unwrap();
    c.adapter.with(&window, |adapter| {
        adapter.items.extend(read.items.clone());
        adapter.settled = read.settlement == Settlement::Settled;
        adapter.failed = read.failed;
        adapter.quota = read.quota.clone();
        adapter.failure = read.failure.clone();
        adapter.failure_kind = read.failure_kind.clone();
    });
    c.pass().unwrap();
    let task = c.task(project.id, 1).task;
    let member = c
        .project(project.id)
        .participants
        .into_iter()
        .find(|member| member.handle == "zeus")
        .unwrap();
    (task.state, member.out_until.is_some())
}

fn held(kind: &str, name: &str, records: &[Value]) {
    assert_eq!(
        outcome(kind, records),
        ("paused".to_owned(), true),
        "{kind}: {name}"
    );
}

fn working(kind: &str, name: &str, records: &[Value]) {
    assert_eq!(
        outcome(kind, records),
        ("working".to_owned(), false),
        "{kind}: {name}"
    );
}

#[test]
fn t1958_a_pi_error_during_its_retry_does_not_fail_the_working_task() {
    // T-1939's native prefix ends at Pi's first refused-connection error. Feed the
    // real reader's result to a working window and take one real dispatcher pass.
    let fixture = include_str!(
        "../../../cf-harness/tests/fixtures/connection-failures/mac-pi-refused-first-error-prefix.jsonl"
    );
    let home = tempfile::tempdir().unwrap();
    let pi = home.path().join("pi");
    fs::create_dir_all(&pi).unwrap();
    let file = pi.join(format!("{SESSION}.jsonl"));
    fs::write(&file, fixture).unwrap();
    let written = fs::metadata(&file)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let now_ms = i64::try_from(written).unwrap();
    let env = Env::from_vars([("PI_CODING_AGENT_SESSION_DIR", pi)]);
    let reading = records::answers(
        Harness::Pi,
        SESSION,
        &env,
        &Options::default(),
        &records::zone("UTC").unwrap(),
        now_ms,
    );
    let Reading::Known(read) = &*reading else {
        panic!("record unreadable: {reading:?}")
    };
    assert!(read.in_flight, "the recorded retry must be in flight");

    let c = Context::made(Made {
        harnesses: vec!["claude-code", "pi"],
        ..Made::default()
    });
    let project = c
        .open_project(json!({
            "directory": "/work/app", "name": "Pi retry",
            "chief": { "harness": "claude-code", "model": "claude-opus-5" },
            "staff": [{ "agent": "zeus", "harness": "pi", "role": "worker", "tier": "standard" }],
        }))
        .unwrap()
        .unwrap();
    c.give(project.id, "zeus", "Read the record");
    for _ in 0..5 {
        c.pass().unwrap();
    }
    let working = c.task(project.id, 1).task;
    assert_eq!(working.state, "working");
    let window = working.assignee.unwrap();
    c.adapter.with(&window, |adapter| {
        adapter.items.extend(read.items.clone());
        adapter.settled = read.settlement == Settlement::Settled;
        adapter.failed = read.failed;
        adapter.quota = read.quota.clone();
        adapter.failure = read.failure.clone();
        adapter.failure_kind = read.failure_kind.clone();
    });
    c.pass().unwrap();
    assert_eq!(
        c.task(project.id, 1).task.state,
        "working",
        "Pi's retry must leave the task working (reader failed={}, in_flight={})",
        read.failed,
        read.in_flight
    );
}

fn text(words: &str) -> Value {
    json!({ "type": "text", "text": words })
}

fn claude_user(uuid: &str, parent: Value, content: Value) -> Value {
    json!({ "type": "user", "uuid": uuid, "parentUuid": parent, "sessionId": SESSION,
        "isSidechain": false, "message": { "role": "user", "content": content } })
}

fn refusal_of_claude() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/engine/fixtures/completion/claude-code/provider-429.jsonl");
    let mut record: Value =
        serde_json::from_str(fs::read_to_string(path).unwrap().lines().next().unwrap()).unwrap();
    record["sessionId"] = json!(SESSION);
    record
}

#[test]
fn a_claude_refusal_stays_unless_a_prompt_of_its_own_branch_follows_it() {
    let refusal = refusal_of_claude();
    let a = refusal["uuid"].clone();
    held(
        "claude-code",
        "the refusal alone",
        std::slice::from_ref(&refusal),
    );
    // The refused turn's own prompt, written late, with the chain that names it after it.
    let mut late = refusal.clone();
    late["parentUuid"] = json!("x2");
    let attachment = json!({ "type": "attachment", "uuid": "x2", "parentUuid": "x1",
        "sessionId": SESSION, "isSidechain": false, "attachment": { "type": "environment" } });
    held(
        "claude-code",
        "late ancestors",
        &[
            late,
            claude_user("x1", Value::Null, json!("original")),
            attachment,
        ],
    );
    let result = json!({ "type": "tool_result", "tool_use_id": "t1", "content": "ok" });
    let mut companion = claude_user(
        "u2",
        a.clone(),
        json!([result, text("A note beside the result.")]),
    );
    companion["isMeta"] = json!(true);
    companion["turnCompanion"] = json!(true);
    held(
        "claude-code",
        "tool result with companion text",
        &[refusal.clone(), companion],
    );
    let command = claude_user(
        "c1",
        a.clone(),
        json!("<command-name>/reload-skills</command-name>\n<command-message>reload-skills</command-message>\n<command-args></command-args>"),
    );
    let printed = json!({ "type": "system", "subtype": "local_command", "isMeta": false,
        "isSidechain": false, "level": "info", "sessionId": SESSION, "uuid": "c2",
        "parentUuid": "c1",
        "content": "<local-command-stdout>Reloaded skills</local-command-stdout>" });
    held(
        "claude-code",
        "a local command",
        &[refusal.clone(), command, printed],
    );
    let mut sidechain = claude_user("u2", a.clone(), json!("Try again"));
    sidechain["isSidechain"] = json!(true);
    held(
        "claude-code",
        "a sidechain prompt",
        &[refusal.clone(), sidechain],
    );
    working(
        "claude-code",
        "a genuine prompt",
        &[refusal, claude_user("u2", a, json!("Try again"))],
    );
}

fn codex_event(name: &str, fields: Value) -> Value {
    let mut payload = fields;
    payload["type"] = json!(name);
    json!({ "timestamp": NOW, "type": "event_msg", "payload": payload })
}

fn codex_start(turn: &str) -> Value {
    codex_event(
        "task_started",
        json!({ "turn_id": turn, "root_turn_id": turn, "model_context_window": 100_000 }),
    )
}

fn codex_end(turn: &str) -> Value {
    codex_event(
        "task_complete",
        json!({ "turn_id": turn, "last_agent_message": null }),
    )
}

fn codex_words(words: &str) -> Value {
    json!({ "type": "response_item", "payload": { "type": "message", "role": "user",
        "id": format!("u-{}", words.len()),
        "content": [{ "type": "input_text", "text": words }] } })
}

#[test]
fn a_codex_count_that_says_the_limit_was_reached_stays_unless_the_person_asks_in_a_new_turn() {
    let (a, b) = (
        "aaaaaaaa-0000-4000-8000-000000000001",
        "bbbbbbbb-0000-4000-8000-000000000002",
    );
    let reached = codex_event(
        "token_count",
        json!({ "rate_limits": { "primary": { "used_percent": 100, "window_minutes": 300,
            "resets_at": RESET }, "rate_limit_reached_type": "rate_limit_reached" } }),
    );
    let closed = vec![
        codex_start(a),
        codex_words("original"),
        reached.clone(),
        codex_end(a),
    ];
    held("codex", "the count alone", &closed);
    held(
        "codex",
        "the same turn started again",
        &[vec![codex_start(a), reached], vec![codex_start(a)]].concat(),
    );
    let compacted = json!({ "type": "compacted", "timestamp": NOW,
        "payload": { "message": "compact summary" } });
    held(
        "codex",
        "a compaction",
        &[
            closed.clone(),
            vec![codex_start(b), compacted, codex_end(b)],
        ]
        .concat(),
    );
    let shell = codex_words("<user_shell_command>\n<command>pwd</command>\n</user_shell_command>");
    held(
        "codex",
        "a standalone shell command",
        &[closed.clone(), vec![codex_start(b), shell, codex_end(b)]].concat(),
    );
    working(
        "codex",
        "a new turn with the person's words",
        &[closed, vec![codex_start(b), codex_words("retry")]].concat(),
    );
}

fn pi_row(role: &str, id: &str, parent: Value, content: Value, extra: Value) -> Value {
    let mut message = json!({ "role": role, "content": content });
    for (key, value) in extra.as_object().unwrap() {
        message[key] = value.clone();
    }
    json!({ "type": "message", "id": id, "parentId": parent,
        "timestamp": "2026-08-24T18:00:01.000Z", "message": message })
}

#[test]
fn a_pi_refusal_stays_where_the_retry_is_on_another_branch_than_the_one_pi_is_on() {
    let header = json!({ "type": "session", "id": SESSION, "version": 3 });
    let first = pi_row("user", "u1", Value::Null, json!([text("work")]), json!({}));
    let refused = pi_row(
        "assistant",
        "a1",
        json!("u1"),
        json!([]),
        json!({ "stopReason": "error", "errorMessage": "401: Authentication failed" }),
    );
    let retry = pi_row("user", "u2", json!("a1"), json!([text("retry")]), json!({}));
    let label = |parent: &str| {
        json!({ "type": "label", "id": "l1", "parentId": parent, "targetId": parent,
            "label": "prior result", "timestamp": "2026-08-24T18:00:03.000Z" })
    };
    let summary = json!({ "type": "branch_summary", "id": "s1", "parentId": "a1", "fromId": "a1",
        "summary": "older branch", "timestamp": "2026-08-24T18:00:03.000Z" });
    let base = vec![header, first, refused];
    held("pi", "the refusal alone", &base);
    let with = |more: Vec<Value>| [base.clone(), more].concat();
    working("pi", "a genuine retry", &with(vec![retry.clone()]));
    held(
        "pi",
        "the refused step labelled",
        &with(vec![retry.clone(), label("a1")]),
    );
    held(
        "pi",
        "a summary on the refused step",
        &with(vec![retry.clone(), summary]),
    );
    working("pi", "the retry labelled", &with(vec![retry, label("u2")]));
}
