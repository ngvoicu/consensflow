use cf_base::env::Env;

use super::*;

const SESSION: &str = "1a8340d5-4fae-474e-a6ff-a3af2182cbc1";

/// What a live run's transcript holds, with what is personal in it: a folder, a
/// branch, an address, ids.
fn records() -> Vec<Value> {
    vec![
        json!({ "type": "last-prompt", "lastPrompt": "Run it, jane", "leafUuid": "l1", "sessionId": SESSION }),
        json!({
            "type": "user",
            "uuid": "u1",
            "parentUuid": null,
            "message": { "role": "user", "content": "Run exactly this one shell command" },
            "cwd": "/Users/jane/Projects/app",
            "gitBranch": "jane/private-work",
            "session_id": SESSION,
            "sessionId": SESSION,
        }),
        json!({
            "type": "attachment",
            "uuid": "a1",
            "attachment": {
                "type": "hook_success",
                "hookName": "Stop",
                "hookEvent": "Stop",
                "exitCode": 0,
                "durationMs": 14,
                "command": "curl https://example.com/secret-token",
                "stdout": "secret output",
            },
            "sessionId": SESSION,
        }),
        json!({
            "type": "attachment",
            "uuid": "a2",
            "attachment": { "type": "session_context", "context": { "userEmail": "jane@example.com" } },
            "rendered": [{ "content": "jane@example.com" }],
            "sessionId": SESSION,
        }),
        json!({
            "type": "bridge-session",
            "sessionId": SESSION,
            "bridgeSessionId": "cse_01Secret",
            "ownerAccountUuid": "account-1",
            "ownerOrganizationUuid": "organization-1",
        }),
        json!({
            "type": "assistant",
            "uuid": "m1",
            "message": {
                "id": "msg_1",
                "role": "assistant",
                "content": [{ "type": "thinking", "thinking": "DONE", "signature": "c2lnbmF0dXJl" }],
            },
            "requestId": "req_01Secret",
            "sessionId": SESSION,
        }),
    ]
}

fn transcript(records: &[Value]) -> String {
    records
        .iter()
        .map(|record| record.to_string() + "\n")
        .collect()
}

/// The records of a fixture, as the tests that read it read them.
fn read(fixture: &Fixture) -> Vec<Value> {
    fixture
        .text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn a_transcript_made_a_fixture_keeps_every_record_in_its_order_and_the_words_of_the_conversation() {
    let given = records();
    let fixture = scrub(&transcript(&given)).unwrap();
    assert_eq!((fixture.records, fixture.session.as_str()), (6, SESSION));
    let kept = read(&fixture);
    let types = |records: &[Value]| -> Vec<String> {
        records
            .iter()
            .map(|record| record["type"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(types(&kept), types(&given));
    assert_eq!(
        kept[1]["message"]["content"],
        "Run exactly this one shell command"
    );
    assert_eq!(kept[5]["message"]["content"][0]["thinking"], "DONE");
    assert!(fixture.text.ends_with('\n') && !fixture.text.ends_with("\n\n"));
}

#[test]
fn a_transcript_made_a_fixture_loses_what_is_personal_in_it() {
    let fixture = scrub(&transcript(&records())).unwrap();
    for personal in [
        SESSION,
        "jane",
        "/Users",
        "private-work",
        "secret",
        "example.com",
        "cse_",
        "account-1",
        "organization-1",
        "c2lnbmF0dXJl",
    ] {
        assert!(
            !fixture.text.contains(personal),
            "{personal} is still there: {}",
            fixture.text
        );
    }
    let kept = read(&fixture);
    assert_eq!(kept[1]["sessionId"], "$SESSION");
    assert_eq!(kept[1]["session_id"], "$SESSION");
    assert_eq!(kept[1]["cwd"], "/work/app");
    assert_eq!(kept[1]["gitBranch"], "main");
    assert_eq!(kept[0]["lastPrompt"], "[scrubbed]");
    assert_eq!(
        kept[2]["attachment"],
        json!({
            "type": "hook_success",
            "hookName": "Stop",
            "hookEvent": "Stop",
            "exitCode": 0,
            "durationMs": 14,
            "command": "[scrubbed]",
        })
    );
    assert_eq!(kept[3]["attachment"], json!({ "type": "session_context" }));
    assert!(kept[3].get("rendered").is_none());
    assert_eq!(kept[4]["bridgeSessionId"], "[scrubbed]");
    assert_eq!(kept[4]["ownerAccountUuid"], "[scrubbed]");
    assert_eq!(kept[4]["ownerOrganizationUuid"], "[scrubbed]");
    assert_eq!(kept[5]["requestId"], "req_scrubbed");
    assert_eq!(kept[5]["message"]["content"][0]["signature"], "[scrubbed]");
}

#[test]
fn a_record_keeps_its_keys_where_they_were_whatever_is_taken_out_of_it() {
    let text = format!(
        r#"{{"type":"attachment","a":1,"rendered":[],"b":2,"renderedRole":"x","attachment":{{"type":"skill","body":"x"}},"c":3,"sessionId":"{SESSION}"}}"#
    );
    let fixture = scrub(&text).unwrap();
    assert_eq!(
        fixture.text,
        "{\"type\":\"attachment\",\"a\":1,\"b\":2,\"attachment\":{\"type\":\"skill\"},\"c\":3,\"sessionId\":\"$SESSION\"}\n"
    );
}

#[test]
fn the_session_is_taken_out_wherever_it_is_written_and_nothing_else_is() {
    let text = format!(
        r#"{{"sessionId":"{SESSION}","path":"/x/{SESSION}.jsonl","note":"$SESSION and {{}} and $1 are kept","n":1.5,"ok":true,"gone":null}}"#
    );
    assert_eq!(
        scrub(&text).unwrap().text,
        "{\"sessionId\":\"$SESSION\",\"path\":\"/x/$SESSION.jsonl\",\"note\":\"$SESSION and {} and $1 are kept\",\"n\":1.5,\"ok\":true,\"gone\":null}\n"
    );
}

#[test]
fn a_hook_that_was_cancelled_keeps_what_a_hook_that_ran_does_and_any_other_attachment_only_its_type(
) {
    let text = [
        json!({ "sessionId": SESSION, "attachment": { "type": "hook_cancelled", "hookName": "Stop", "timedOut": true, "stdout": "x" } }),
        json!({ "sessionId": SESSION, "attachment": { "type": "hook_non_blocking_error", "hookName": "Stop", "stderr": "x" } }),
        json!({ "sessionId": SESSION, "attachment": { "stdout": "no type" } }),
        json!({ "sessionId": SESSION, "attachment": "just a word" }),
        json!({ "sessionId": SESSION, "attachment": null, "lastPrompt": 7, "aiTitle": "a title" }),
        json!({ "sessionId": SESSION, "attachment": 0, "rendered": "kept" }),
    ];
    let kept = read(&scrub(&transcript(&text)).unwrap());
    assert_eq!(
        kept[0]["attachment"],
        json!({ "type": "hook_cancelled", "hookName": "Stop", "timedOut": true, "command": "[scrubbed]" })
    );
    assert_eq!(
        kept[1]["attachment"],
        json!({ "type": "hook_non_blocking_error" })
    );
    assert_eq!(kept[2]["attachment"], json!({}));
    assert_eq!(kept[3]["attachment"], json!({}));
    // What is no attachment (nothing, or what JavaScript takes for false) is left as it was.
    assert_eq!(kept[4]["attachment"], Value::Null);
    assert_eq!(kept[4]["lastPrompt"], 7);
    assert_eq!(kept[4]["aiTitle"], "[scrubbed]");
    assert_eq!(
        (&kept[5]["attachment"], &kept[5]["rendered"]),
        (&json!(0), &json!("kept"))
    );
}

#[test]
fn a_transcript_that_cannot_be_made_a_fixture_is_said_why() {
    let said = |text: &str| scrub(text).unwrap_err().to_string();
    assert_eq!(said(""), "no record names its session");
    assert_eq!(said("\n\n"), "no record names its session");
    assert_eq!(said(r#"{"type":"user"}"#), "no record names its session");
    assert_eq!(said(r#"{"sessionId":""}"#), "no record names its session");
    assert_eq!(said(r#"{"sessionId":7}"#), "no record names its session");
    assert_eq!(
        said("[1]"),
        "line 1 is no record: a record is a JSON object"
    );
    let text = format!("{{\"sessionId\":\"{SESSION}\"}}\n\n{{\"sessionId\":\n");
    let not_json = said(&text);
    // The third line of the file, whatever the second was (blank): where a person looks.
    assert!(
        not_json.starts_with("line 3 is not JSON: "),
        "blank lines are counted: {not_json}"
    );
    let not_record = said("\n[1]\n");
    assert_eq!(not_record, "line 2 is no record: a record is a JSON object");
}

#[test]
fn the_records_after_blank_lines_are_kept_and_lines_of_windows_are_read() {
    let text = format!(
        "\n{{\"sessionId\":\"{SESSION}\"}}\r\n\n{{\"sessionId\":\"{SESSION}\",\"n\":2}}\r\n"
    );
    let fixture = scrub(&text).unwrap();
    assert_eq!(fixture.records, 2);
    assert_eq!(
        fixture.text,
        "{\"sessionId\":\"$SESSION\"}\n{\"sessionId\":\"$SESSION\",\"n\":2}\n"
    );
}

fn context() -> Context {
    Context {
        root: std::path::PathBuf::from("checkout"),
        env: Env::default(),
    }
}

fn command(args: &[&Path]) -> (Result<i32, Failure>, String, String) {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = scrub_transcript(
        &context(),
        &args,
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
    );
    (
        status,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn the_command_makes_the_fixture_where_it_is_told_and_says_what_it_kept() {
    let dir = tempfile::tempdir().unwrap();
    let from = dir.path().join("transcript.jsonl");
    let to = dir.path().join("fixture.jsonl");
    fs::write(&from, transcript(&records())).unwrap();
    let (status, out, err) = command(&[&from, &to]);
    assert_eq!(status.unwrap(), 0);
    assert_eq!(
        out,
        format!("6 records of {SESSION} kept in {}\n", to.display())
    );
    assert_eq!(err, "");
    let kept = fs::read_to_string(&to).unwrap();
    assert_eq!(kept, scrub(&transcript(&records())).unwrap().text);
    assert!(!kept.contains(SESSION));
}

#[test]
fn the_command_says_what_went_wrong_and_ends_with_1_and_makes_no_fixture_of_what_it_could_not_read()
{
    let dir = tempfile::tempdir().unwrap();
    let (from, to) = (
        dir.path().join("none.jsonl"),
        dir.path().join("fixture.jsonl"),
    );
    let (status, out, err) = command(&[&from, &to]);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(out, "");
    assert!(
        err.starts_with(&format!(
            "scrub-transcript: could not read {}: ",
            from.display()
        )),
        "{err}"
    );

    let bad = dir.path().join("bad.jsonl");
    fs::write(&bad, "not json\n").unwrap();
    let (status, _, err) = command(&[&bad, &to]);
    assert_eq!(status.unwrap(), 1);
    assert!(
        err.starts_with("scrub-transcript: line 1 is not JSON: "),
        "{err}"
    );
    assert!(!to.exists());

    let (status, _, err) = command(&[&bad, &dir.path().join("no-folder").join("x.jsonl")]);
    assert_eq!(status.unwrap(), 1);
    assert!(
        err.contains("line 1 is not JSON"),
        "reading comes before writing: {err}"
    );
}

#[test]
fn the_command_takes_the_transcript_and_the_fixture_and_nothing_else() {
    for args in [
        &[][..],
        &[Path::new("a")],
        &[Path::new("a"), Path::new("b"), Path::new("c")],
    ] {
        let (status, out, err) = command(args);
        let said = status.unwrap_err().to_string();
        assert!(said.starts_with("scrub-transcript takes the transcript to scrub and the fixture to make: <transcript.jsonl> <fixture.jsonl>"), "{said}");
        assert_eq!((out.as_str(), err.as_str()), ("", ""));
    }
}
