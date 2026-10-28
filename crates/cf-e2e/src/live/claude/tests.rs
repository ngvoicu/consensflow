//! What Claude's status and transcript are read as, and the processes of the
//! machine a listing of `ps` makes.

use super::*;

fn sample_of(at_ms: i64, status: Option<&str>, waiting: Option<&str>) -> Sample {
    Sample {
        at_ms,
        status: status.map(str::to_owned),
        waiting_for: waiting.map(str::to_owned),
        changed_at_ms: None,
    }
}

#[test]
fn the_status_of_a_session_is_in_the_file_named_by_a_process_that_names_the_session() {
    let config = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        crate::files::write(&config.path().join("sessions").join(name), text).unwrap();
    };
    write("101.json", r#"{"pid":101,"sessionId":"a","status":"idle"}"#);
    write(
        "102.json",
        r#"{"pid":102,"sessionId":"b","status":"busy","statusUpdatedAt":1791711000123}"#,
    );
    write("notes.json", r#"{"sessionId":"b","status":"idle"}"#);
    write("103.json", "{ not json");
    write("104.txt", r#"{"sessionId":"b"}"#);
    let found = status_of(config.path(), "b").unwrap();
    assert_eq!(found["pid"], 102);
    assert_eq!(status_of(config.path(), "c"), None);
    assert_eq!(status_of(&config.path().join("not-there"), "b"), None);
    let look = sample(config.path(), "b", false);
    assert_eq!(
        (look.status.as_deref(), look.changed_at_ms),
        (Some("busy"), Some(1_791_711_000_123))
    );
    // A session with no status file is read as having none.
    let none = sample(config.path(), "c", false);
    assert_eq!(
        (none.status, none.waiting_for, none.changed_at_ms),
        (None, None, None)
    );
}

#[test]
fn the_file_a_killed_window_left_is_not_the_status_of_the_window_resumed_on_its_conversation() {
    let config = tempfile::tempdir().unwrap();
    // The killed window's file sorts first, as the older of two process ids may; its process
    // is gone (no process has an id this large). The resumed window's is this process's.
    let (gone, here) = ("004000000000".to_owned(), std::process::id().to_string());
    for (name, status) in [(&gone, "busy"), (&here, "idle")] {
        let file = config.path().join("sessions").join(format!("{name}.json"));
        let text = format!(r#"{{"sessionId":"s","status":"{status}"}}"#);
        crate::files::write(&file, text).unwrap();
    }
    let word = |status: Option<Value>| status.map(|status| status["status"].to_string());
    // Read as the files come, the dead window's word is the first: a stale `busy`.
    assert_eq!(
        word(status_of(config.path(), "s")).as_deref(),
        Some("\"busy\"")
    );
    // Read of the window that runs, the file of a process that is gone says nothing.
    assert_eq!(
        word(running_status_of(config.path(), "s")).as_deref(),
        Some("\"idle\"")
    );
    assert_eq!(
        sample(config.path(), "s", true).status.as_deref(),
        Some("idle")
    );
    assert_eq!(
        sample(config.path(), "s", false).status.as_deref(),
        Some("busy")
    );
    // A conversation only a dead window names is none at all.
    crate::files::write(
        &config.path().join("sessions/004000000001.json"),
        r#"{"sessionId":"left","status":"busy"}"#,
    )
    .unwrap();
    assert!(status_of(config.path(), "left").is_some());
    assert_eq!(running_status_of(config.path(), "left"), None);
    assert_eq!(sample(config.path(), "left", true).status, None);
    // The watch of the window that runs reads the same.
    let watch = Status::watch_running(config.path(), "s");
    let read = super::super::until(Duration::from_secs(10), Duration::from_millis(10), || {
        watch.samples().into_iter().next()
    });
    assert_eq!(read.unwrap().status.as_deref(), Some("idle"));
}

#[test]
fn a_time_with_a_fraction_of_a_millisecond_is_that_millisecond() {
    let config = tempfile::tempdir().unwrap();
    crate::files::write(
        &config.path().join("sessions/7.json"),
        r#"{"sessionId":"s","status":"idle","statusUpdatedAt":1791711000123.6}"#,
    )
    .unwrap();
    assert_eq!(
        sample(config.path(), "s", false).changed_at_ms,
        Some(1_791_711_000_124)
    );
}

#[test]
fn a_watch_reads_the_status_as_it_changes_and_stops_when_it_is_told_to() {
    let config = tempfile::tempdir().unwrap();
    let file = config.path().join("sessions/9.json");
    crate::files::write(&file, r#"{"sessionId":"s","status":"busy"}"#).unwrap();
    let status = Status::watch(config.path(), "s");
    let first = super::super::until(Duration::from_secs(10), Duration::from_millis(10), || {
        status
            .samples()
            .into_iter()
            .find(|look| look.status.as_deref() == Some("busy"))
    });
    assert!(first.is_some(), "it read the status the file held");
    crate::files::write(
        &file,
        r#"{"sessionId":"s","status":"idle","waitingFor":"input"}"#,
    )
    .unwrap();
    let idle = super::super::until(Duration::from_secs(10), Duration::from_millis(10), || {
        status
            .samples()
            .into_iter()
            .find(|look| look.status.as_deref() == Some("idle"))
    });
    assert_eq!(idle.unwrap().waiting_for.as_deref(), Some("input"));
    let all = status.stop();
    assert!(all.len() >= 2 && all.windows(2).all(|pair| pair[0].at_ms <= pair[1].at_ms));
}

#[test]
fn samples_that_read_alike_are_a_run_with_the_time_it_ran_and_how_many_reads_it_took() {
    let samples = [
        sample_of(1_000, Some("busy"), None),
        sample_of(1_150, Some("busy"), Some("")),
        sample_of(13_300, Some("busy"), None),
        sample_of(13_500, Some("waiting"), Some("approval")),
        sample_of(13_600, None, None),
    ];
    assert_eq!(
        runs(&samples, 1_000),
        [
            "busy 0.0–12.3 s (3 reads)",
            "waiting (approval) 12.5–12.5 s (1 reads)",
            "no status file 12.6–12.6 s (1 reads)",
        ]
    );
    assert!(runs(&[], 0).is_empty());
}

#[test]
fn a_transcript_is_found_in_whichever_project_folder_holds_it() {
    let config = tempfile::tempdir().unwrap();
    let at = |relative: &str, text: &str| {
        crate::files::write(&config.path().join(relative), text).unwrap();
    };
    at("projects/-Users-me-a/other.jsonl", "{}\n");
    at("projects/-Users-me-b/s-1.jsonl", "{}\n");
    assert_eq!(
        transcript_file(config.path(), "s-1"),
        Some(config.path().join("projects/-Users-me-b/s-1.jsonl"))
    );
    assert_eq!(transcript_file(config.path(), "s-2"), None);
    assert_eq!(transcript_file(&config.path().join("none"), "s-1"), None);
}

#[test]
fn a_transcript_is_read_as_records_with_what_each_says() {
    let config = tempfile::tempdir().unwrap();
    let user = r#"{"type":"user","timestamp":"2026-10-11T09:30:00.123Z","message":{"role":"user","content":"Reply with the sum"}}"#;
    let answer = r#"{"type":"assistant","timestamp":"2026-10-11T09:30:04.000Z","message":{"role":"assistant","model":"claude-haiku-5-5","content":[{"type":"text","text":"5555"},{"type":"tool_use","name":"Bash","input":{"command":"ls"}},{"type":"thinking"}]}}"#;
    let result = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","is_error":true,"content":"no such file"}]}}"#;
    let system = r#"{"type":"system","subtype":"turn_duration","durationMs":5}"#;
    let request =
        r#"{"type":"api-request-blob","message":{"role":"user","content":"Reply with the sum"}}"#;
    // The line Claude is writing as the transcript is read is not yet a record.
    let torn = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"te"#;
    crate::files::write(
        &config.path().join("projects/-p/s.jsonl"),
        format!("{user}\n{answer}\n{result}\n{system}\n{request}\n{torn}"),
    )
    .unwrap();
    let entries = record(config.path(), "s");
    let shown: Vec<(&str, &str)> = entries
        .iter()
        .map(|entry| (entry.kind.as_str(), entry.text.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("user", "Reply with the sum"),
            (
                "assistant",
                r#"5555 ⏎ tool_use Bash {"command":"ls"} ⏎ thinking"#
            ),
            ("user", "tool_result (error) no such file"),
            ("system/turn_duration", ""),
            // What Claude Code kept of a request it sent is no message of the user's.
            ("api-request-blob", "Reply with the sum"),
        ]
    );
    assert_eq!(entries[0].at_ms, Some(1_791_711_000_123));
    assert_eq!(entries[3].at_ms, None);
    // The model that answered is the assistant's record's, and no other says one.
    let models: Vec<Option<&str>> = entries.iter().map(|entry| entry.model.as_deref()).collect();
    assert_eq!(models, [None, Some("claude-haiku-5-5"), None, None, None]);
    assert_eq!(entries[0].raw, user);
    assert!(record(config.path(), "none").is_empty());
}

#[test]
fn a_listing_of_processes_is_cut_to_the_ones_that_name_the_text_but_the_listings_own() {
    let listing = "\
  101     1 /usr/bin/login -pf me
  202   101 /Users/me/.local/bin/claude --session-id abc --model fake-chief
 3030   202 sh -c cf hook claude   --session-id abc
 4040   101 ps -axo pid=,ppid=,command= --session-id abc
 not a line
";
    assert_eq!(
        listed(listing, "--session-id abc"),
        [
            Process {
                pid: 202,
                ppid: 101,
                command: "/Users/me/.local/bin/claude --session-id abc --model fake-chief"
                    .to_owned()
            },
            Process {
                pid: 3030,
                ppid: 202,
                command: "sh -c cf hook claude   --session-id abc".to_owned()
            },
        ]
    );
    assert!(listed(listing, "no-such-text").is_empty());
}

#[cfg(unix)]
#[test]
fn the_processes_of_the_machine_are_listed_with_this_one_among_them() {
    let own = std::process::id();
    let found = processes_with("").unwrap();
    assert!(found.iter().any(|process| process.pid == own), "{found:?}");
}
