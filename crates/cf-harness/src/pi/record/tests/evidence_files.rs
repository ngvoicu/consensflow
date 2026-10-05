//! The files the extension writes beside a session: when they settle a turn
//! or say one is in flight, which launch they are read for, and what keeps
//! a file from being read at all.

use super::*;

/// The folder of the extension's evidence in a stage's home.
fn folder(stage: &Stage) -> PathBuf {
    stage.home.path().join("settled")
}

/// Options naming the folder `directory` and the launch `launch`.
fn options_of(directory: &Path, launch: &str) -> Options {
    Options {
        pi_settlement: Some(PiSettlement {
            directory: Some(directory.to_str().unwrap().to_owned()),
            launch_id: Some(launch.to_owned()),
        }),
    }
}

/// Options naming the stage's folder and the launch `launch`.
fn options(stage: &Stage, launch: &str) -> Options {
    options_of(&folder(stage), launch)
}

/// The file `name` of `directory`, holding `contents`.
fn put_in(directory: &Path, name: &str, contents: impl AsRef<[u8]>) {
    let file = directory.join(name);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, contents).unwrap();
}

/// The file `name` of the stage's folder, holding `contents`.
fn put(stage: &Stage, name: &str, contents: impl AsRef<[u8]>) {
    put_in(&folder(stage), name, contents);
}

/// What the extension writes when Pi settles on the leaf entry `frontier`.
fn settled(launch: &str, session: &str, frontier: &Value) -> String {
    json!({ "launchId": launch, "sessionId": session, "frontier": { "id": frontier } }).to_string()
}

/// A session whose last step stopped, as `a1`.
fn stopped() -> Vec<Value> {
    vec![header(), user("u1", "go"), assistant("a1", "stop", "done")]
}

/// Whether the look at `after` milliseconds says the turn is settled.
fn is_settled(stage: &mut Stage, after: i64, options: &Options) -> bool {
    known(&stage.look(after, options)).settlement == Settlement::Settled
}

#[test]
fn evidence_that_names_the_last_step_of_this_launch_and_session_settles_at_once() {
    let mut stage = Stage::with(&stopped());
    let named = options(&stage, "launch-1");
    assert!(!is_settled(&mut stage, 1_000, &named), "no evidence yet");
    put(
        &stage,
        "launch-1.json",
        settled("launch-1", SESSION, &json!("a1")),
    );
    assert!(is_settled(&mut stage, 1_000, &named));
    assert!(!known(&stage.look(1_000, &named)).in_flight);
    // Evidence of another leaf, session or launch, or that is not of a leaf, waits for the quiet.
    let frontier = json!("a1");
    for evidence in [
        settled("launch-1", SESSION, &json!("a0")),
        settled("launch-1", SESSION, &json!("")),
        settled("launch-1", SESSION, &json!(5)),
        settled("launch-1", SESSION, &json!(null)),
        settled("launch-1", "another-session", &frontier),
        settled("launch-2", SESSION, &frontier),
        json!({ "launchId": "launch-1", "sessionId": SESSION }).to_string(),
        json!({ "launchId": "launch-1", "sessionId": SESSION, "frontier": "a1" }).to_string(),
        json!({ "launchId": 1, "sessionId": SESSION, "frontier": { "id": "a1" } }).to_string(),
        json!(["a1"]).to_string(),
        json!("a1").to_string(),
        json!(null).to_string(),
    ] {
        put(&stage, "launch-1.json", &evidence);
        assert!(!is_settled(&mut stage, 1_000, &named), "{evidence}");
    }
}

#[test]
fn evidence_settles_what_an_error_ended_and_never_what_a_call_holds_open() {
    // The turn ended in a failure the extension saw Pi settle on.
    let mut stage = Stage::with(&[header(), user("u1", "go"), assistant("a1", "error", "down")]);
    let named = options(&stage, "launch-1");
    put(
        &stage,
        "launch-1.json",
        settled("launch-1", SESSION, &json!("a1")),
    );
    let record = known(&stage.look(0, &named));
    assert!(record.failed && !record.in_flight);
    assert_eq!(record.settlement, Settlement::Settled);
    // The step stopped, and a call of the turn is still open.
    let mut stage = Stage::with(&[
        header(),
        user("u1", "go"),
        calling("a0", &json!("c1")),
        assistant("a1", "stop", "done"),
    ]);
    let named = options(&stage, "launch-1");
    put(
        &stage,
        "launch-1.json",
        settled("launch-1", SESSION, &json!("a1")),
    );
    let record = known(&stage.look(0, &named));
    assert!(record.in_flight);
    assert_eq!(record.settlement, Settlement::InFlight);
}

#[test]
fn the_launch_is_what_the_options_say_and_the_environment_says_what_they_leave_none() {
    let evidence = tempfile::tempdir().unwrap();
    let dir = evidence.path().to_str().unwrap();
    let mut stage = Stage::with_vars(&[
        ("CF_DELIVERY_SETTLED", dir),
        ("CF_DELIVERY_LAUNCH_ID", "launch-env"),
    ]);
    stage.write(&stopped());
    for launch in ["launch-env", "launch-opt"] {
        put_in(
            evidence.path(),
            &format!("{launch}.json"),
            settled(launch, SESSION, &json!("a1")),
        );
    }
    let mut settles = |directory: Option<&str>, launch: Option<&str>| {
        let options = Options {
            pi_settlement: Some(PiSettlement {
                directory: directory.map(str::to_owned),
                launch_id: launch.map(str::to_owned),
            }),
        };
        is_settled(&mut stage, 1_000, &options)
    };
    // Each field is the options' when they say it, and the environment's when not.
    assert!(settles(None, None));
    assert!(settles(Some(dir), None));
    assert!(settles(None, Some("launch-opt")));
    assert!(settles(Some(dir), Some("launch-opt")));
    // An empty text is a text, as `??` takes it: it does not fall back to the environment.
    assert!(!settles(None, Some("")));
    assert!(!settles(Some("/no/such/folder"), None));
    assert!(!settles(None, Some("launch-none")));
    // Told nothing at all, it is the environment's.
    assert!(is_settled(&mut stage, 1_000, &Options::default()));
}

#[test]
fn a_launch_that_is_no_one_path_segment_reads_no_file() {
    let mut stage = Stage::with(&stopped());
    let evidence = |launch: &str| settled(launch, SESSION, &json!("a1"));
    // Each name, and where it would lead if it were read: a file that
    // settles the turn waits there.
    for (launch, file) in [
        ("../x", "../x.json"),
        ("./x", "x.json"),
        ("a b", "a b.json"),
        ("a/b", "a/b.json"),
        ("", ".json"),
        ("\u{E9}", "\u{E9}.json"),
    ] {
        put(&stage, file, evidence(launch));
        let named = options(&stage, launch);
        assert!(!is_settled(&mut stage, 1_000, &named), "{launch:?}");
    }
    // Names no file of every system can have.
    for launch in ["a\\b", "x ", "x\n"] {
        let named = options(&stage, launch);
        assert!(!is_settled(&mut stage, 1_000, &named), "{launch:?}");
    }
    // A name of letters, digits, dots, underscores and hyphens is read.
    for launch in ["x", "A.b_c-d", "..", "0"] {
        put(&stage, &format!("{launch}.json"), evidence(launch));
        let named = options(&stage, launch);
        assert!(is_settled(&mut stage, 1_000, &named), "{launch:?}");
    }
}

#[test]
fn evidence_that_is_no_json_is_none_and_evidence_that_cannot_be_read_fails_the_look() {
    let mut stage = Stage::with(&stopped());
    let named = options(&stage, "launch-1");
    for contents in ["", "not json", "{\"launchId\":", "[1,"] {
        put(&stage, "launch-1.json", contents);
        assert!(!is_settled(&mut stage, 1_000, &named), "{contents:?}");
    }
    // Bytes that are no UTF-8 read as U+FFFD, as Node decoded the file.
    let mut bytes =
        br#"{"launchId":"launch-1","sessionId":"hazy-ridge","frontier":{"id":"a1"},"note":"#
            .to_vec();
    bytes.extend_from_slice(b"\"\xFF\"}");
    put(&stage, "launch-1.json", bytes);
    assert!(is_settled(&mut stage, 1_000, &named));
    // A folder where the file should be is no file: neither missing nor JSON.
    fs::remove_file(folder(&stage).join("launch-1.json")).unwrap();
    fs::create_dir(folder(&stage).join("launch-1.json")).unwrap();
    let failed = stage.look(1_000, &named);
    assert!(reason(&failed).starts_with("unreadable: "), "{failed:?}");
    assert!(reason(&failed).contains("launch-1.json"), "{failed:?}");
}

#[test]
#[cfg(unix)]
fn a_folder_of_evidence_that_is_a_file_fails_the_look() {
    // Node: ENOTDIR, which is not the ENOENT that is no evidence. Windows says
    // the path is not found, as libuv's ENOENT.
    let mut stage = Stage::with(&stopped());
    fs::write(folder(&stage), "").unwrap();
    let named = options(&stage, "launch-1");
    assert!(reason(&stage.look(1_000, &named)).starts_with("unreadable: "));
}

#[test]
fn a_working_marker_says_a_turn_whose_file_is_not_there_yet_is_in_flight() {
    let mut stage = Stage::new();
    let named = options(&stage, "launch-1");
    // No marker: no session, as ever.
    assert_eq!(
        reason(&stage.look(0, &named)),
        "unreadable: no pi session hazy-ridge"
    );
    let marker = |launch: &str, session: &str| {
        json!({ "launchId": launch, "sessionId": session, "startedAt": 1 }).to_string()
    };
    put(&stage, "launch-1.working.json", marker("launch-1", SESSION));
    let record = known(&stage.look(0, &named));
    assert!(record.items.is_empty() && record.in_flight && !record.failed && !record.asking);
    assert!(record.quota.is_none());
    assert_eq!(record.settlement, Settlement::InFlight);
    // A marker of another session or launch, that holds no text for either, or that is no JSON.
    for contents in [
        marker("launch-1", "another-session"),
        marker("launch-2", SESSION),
        json!({ "launchId": 1, "sessionId": SESSION }).to_string(),
        json!({ "sessionId": SESSION }).to_string(),
        json!(null).to_string(),
        "not json".to_owned(),
        String::new(),
    ] {
        put(&stage, "launch-1.working.json", &contents);
        assert_eq!(
            reason(&stage.look(0, &named)),
            "unreadable: no pi session hazy-ridge",
            "{contents}"
        );
    }
    // A marker that cannot be read fails the look: it is not the sentence of no session.
    fs::remove_file(folder(&stage).join("launch-1.working.json")).unwrap();
    fs::create_dir(folder(&stage).join("launch-1.working.json")).unwrap();
    let failed = stage.look(0, &named);
    assert!(reason(&failed).starts_with("unreadable: "));
    assert!(!reason(&failed).contains("no pi session"), "{failed:?}");
}

#[test]
fn a_working_marker_is_read_only_while_there_is_no_session_file() {
    let mut stage = Stage::with(&stopped());
    let named = options(&stage, "launch-1");
    // Even a marker that cannot be read: it is never looked at.
    fs::create_dir_all(folder(&stage).join("launch-1.working.json")).unwrap();
    assert_eq!(known(&stage.look(0, &named)).items.len(), 2);
    // An unsafe launch id reads no marker either.
    let mut stage = Stage::new();
    let unsafe_launch = options(&stage, "../x");
    put(
        &stage,
        "../x.working.json",
        json!({ "launchId": "../x", "sessionId": SESSION }).to_string(),
    );
    assert_eq!(
        reason(&stage.look(0, &unsafe_launch)),
        "unreadable: no pi session hazy-ridge"
    );
}
