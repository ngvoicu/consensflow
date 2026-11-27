//! The word the extension writes beside a session (`<launch>.state.json`): which launch and
//! session it is read for, what is no word, and what keeps it from being read at all.

use super::*;

/// The folder of the extension's word in a stage's home.
pub(super) fn folder(stage: &Stage) -> PathBuf {
    stage.home.path().join("settled")
}

/// Options naming the folder `directory` and the launch `launch`.
fn options_of(directory: &Path, launch: &str) -> Options {
    Options {
        pi_settlement: Some(PiSettlement {
            directory: Some(directory.to_str().unwrap().to_owned()),
            launch_id: Some(launch.to_owned()),
        }),
        ..Options::default()
    }
}

/// Options naming the stage's folder and the launch `launch`.
pub(super) fn options(stage: &Stage, launch: &str) -> Options {
    options_of(&folder(stage), launch)
}

/// The file `name` of `directory`, holding `contents`.
fn put_in(directory: &Path, name: &str, contents: impl AsRef<[u8]>) {
    let file = directory.join(name);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, contents).unwrap();
}

/// The file `name` of the stage's folder, holding `contents`.
pub(super) fn put(stage: &Stage, name: &str, contents: impl AsRef<[u8]>) {
    put_in(&folder(stage), name, contents);
}

/// The word the extension writes: `state` at the leaf `leaf`, with `ends` ended runs, each
/// `(n, leaf, aborted)`, the last of them the count of runs ended; `working` with one run more
/// begun than ended (a run on).
pub(super) fn word(
    launch: &str,
    session: &str,
    state: &str,
    leaf: &Value,
    ends: &[(u64, Value, Value)],
) -> String {
    let ended = ends.last().map_or(0, |(n, _, _)| *n);
    json!({
        "launchId": launch, "sessionId": session, "state": state, "leaf": leaf,
        "begun": ended + u64::from(state == "working"), "ended": ended,
        "ends": ends.iter().map(|(n, leaf, aborted)| json!({ "n": n, "leaf": leaf, "aborted": aborted })).collect::<Vec<_>>(),
        "n": 1, "at": 1
    })
    .to_string()
}

/// The word that Pi rests at `leaf` after one run ended there, Pi's flag false.
pub(super) fn rest(launch: &str, leaf: &str) -> String {
    word(
        launch,
        SESSION,
        "rest",
        &json!(leaf),
        &[(1, json!(leaf), json!(false))],
    )
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
fn a_word_of_this_launch_and_session_decides_and_any_other_is_no_word() {
    let mut stage = Stage::with(&stopped());
    let named = options(&stage, "launch-1");
    assert!(!is_settled(&mut stage, 1_000, &named), "no word: the quiet");
    put(&stage, "launch-1.state.json", rest("launch-1", "a1"));
    assert!(is_settled(&mut stage, 1_000, &named));
    // A word of another session or launch, or one that says no state the extension writes or
    // no count of its runs, is no word: the file's quiet decides.
    for word in [
        word("launch-1", "another-session", "rest", &json!("a1"), &[]),
        word("launch-2", SESSION, "rest", &json!("a1"), &[]),
        json!({ "launchId": "launch-1", "sessionId": SESSION, "state": "idle", "ended": 0 })
            .to_string(),
        json!({ "launchId": "launch-1", "sessionId": SESSION, "state": "rest" }).to_string(),
        json!({ "launchId": "launch-1", "sessionId": SESSION, "state": "rest", "ended": -1 })
            .to_string(),
        json!({ "launchId": 1, "sessionId": SESSION, "state": "rest", "ended": 0 }).to_string(),
        json!(["rest"]).to_string(),
        json!(null).to_string(),
    ] {
        put(&stage, "launch-1.state.json", &word);
        let record = known(&stage.look(1_000, &named));
        assert!(
            record.settlement != Settlement::Settled,
            "{word}: {record:?}"
        );
        assert_eq!(
            record.lines.settles_at,
            Some(stage.written + 120_000),
            "{word}"
        );
        assert!(
            stage.at(121_000).settlement == Settlement::Settled,
            "{word}"
        );
    }
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
            &format!("{launch}.state.json"),
            rest(launch, "a1"),
        );
    }
    let mut settles = |directory: Option<&str>, launch: Option<&str>| {
        let options = Options {
            pi_settlement: Some(PiSettlement {
                directory: directory.map(str::to_owned),
                launch_id: launch.map(str::to_owned),
            }),
            ..Options::default()
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
    // Each name, and where it would lead if it were read: a word at rest waits there.
    for (launch, file) in [
        ("../x", "../x.state.json"),
        ("./x", "x.state.json"),
        ("a b", "a b.state.json"),
        ("a/b", "a/b.state.json"),
        ("", ".state.json"),
        ("\u{E9}", "\u{E9}.state.json"),
    ] {
        put(&stage, file, rest(launch, "a1"));
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
        put(&stage, &format!("{launch}.state.json"), rest(launch, "a1"));
        let named = options(&stage, launch);
        assert!(is_settled(&mut stage, 1_000, &named), "{launch:?}");
    }
}

#[test]
fn a_word_that_is_no_json_is_none_and_one_that_cannot_be_read_fails_the_look() {
    let mut stage = Stage::with(&stopped());
    let named = options(&stage, "launch-1");
    for contents in ["", "not json", "{\"launchId\":", "[1,"] {
        put(&stage, "launch-1.state.json", contents);
        assert!(!is_settled(&mut stage, 1_000, &named), "{contents:?}");
    }
    // Bytes that are no UTF-8 read as U+FFFD, as Node decoded the file.
    let mut bytes = br#"{"launchId":"launch-1","sessionId":"hazy-ridge","state":"rest","leaf":"a1","ended":0,"note":"#
        .to_vec();
    bytes.extend_from_slice(b"\"\xFF\"}");
    put(&stage, "launch-1.state.json", bytes);
    assert!(is_settled(&mut stage, 1_000, &named));
    // A folder where the file should be is no file: neither missing nor JSON.
    fs::remove_file(folder(&stage).join("launch-1.state.json")).unwrap();
    fs::create_dir(folder(&stage).join("launch-1.state.json")).unwrap();
    let failed = stage.look(1_000, &named);
    assert!(reason(&failed).starts_with("unreadable: "), "{failed:?}");
    assert!(
        reason(&failed).contains("launch-1.state.json"),
        "{failed:?}"
    );
}

#[test]
#[cfg(unix)]
fn a_folder_of_the_word_that_is_a_file_fails_the_look() {
    // Node: ENOTDIR, which is not the ENOENT that is no word. Windows says the path is not
    // found, as libuv's ENOENT.
    let mut stage = Stage::with(&stopped());
    fs::write(folder(&stage), "").unwrap();
    let named = options(&stage, "launch-1");
    assert!(reason(&stage.look(1_000, &named)).starts_with("unreadable: "));
}

#[test]
fn with_no_file_yet_the_word_says_the_turn_and_no_word_says_no_session() {
    let mut stage = Stage::new();
    let named = options(&stage, "launch-1");
    assert_eq!(
        reason(&stage.look(0, &named)),
        "unreadable: no pi session hazy-ridge"
    );
    // A first request that hangs: Pi has written no file, the word says a run is on.
    put(
        &stage,
        "launch-1.state.json",
        word("launch-1", SESSION, "working", &json!("m1"), &[]),
    );
    let record = known(&stage.look(0, &named));
    assert!(record.items.is_empty() && record.in_flight && !record.failed && !record.asking);
    assert_eq!(record.settlement, Settlement::InFlight);
    // A new session at its empty prompt: Pi keeps its first leaf in memory and writes no file
    // until a conversation (D5). At rest, with nothing to read.
    put(
        &stage,
        "launch-1.state.json",
        word("launch-1", SESSION, "rest", &json!("m1"), &[]),
    );
    let record = known(&stage.look(0, &named));
    assert!(record.items.is_empty() && !record.in_flight && !record.aborted && !record.neither());
    assert_eq!(record.settlement, Settlement::Settled);
    // Busy: neither.
    put(
        &stage,
        "launch-1.state.json",
        word("launch-1", SESSION, "busy", &json!("m1"), &[]),
    );
    assert!(known(&stage.look(0, &named)).neither());
    // A word that cannot be read fails the look: it is not the sentence of no session.
    fs::remove_file(folder(&stage).join("launch-1.state.json")).unwrap();
    fs::create_dir(folder(&stage).join("launch-1.state.json")).unwrap();
    let failed = stage.look(0, &named);
    assert!(reason(&failed).starts_with("unreadable: "));
    assert!(!reason(&failed).contains("no pi session"), "{failed:?}");
}

#[test]
fn the_files_an_older_extension_wrote_say_nothing() {
    // `<launch>.json` (its settlement) and `<launch>.working.json` (its working marker): a window
    // an older ConsensFlow opened is read by the file's quiet alone.
    let mut stage = Stage::with(&stopped());
    let named = options(&stage, "launch-1");
    put(
        &stage,
        "launch-1.json",
        json!({ "launchId": "launch-1", "sessionId": SESSION, "frontier": { "id": "a1" } })
            .to_string(),
    );
    put(
        &stage,
        "launch-1.working.json",
        json!({ "launchId": "launch-1", "sessionId": SESSION, "startedAt": 1 }).to_string(),
    );
    let record = known(&stage.look(1_000, &named));
    assert!(
        record.in_flight && record.lines.settles_at.is_some(),
        "{record:?}"
    );
    assert!(is_settled(&mut stage, 121_000, &named));
    let mut fresh = Stage::new();
    let named = options(&fresh, "launch-1");
    put(
        &fresh,
        "launch-1.working.json",
        json!({ "launchId": "launch-1", "sessionId": SESSION, "startedAt": 1 }).to_string(),
    );
    assert_eq!(
        reason(&fresh.look(0, &named)),
        "unreadable: no pi session hazy-ridge"
    );
}
