use super::*;
use serde_json::json;

/// This process's id: a process alive.
fn me() -> u32 {
    std::process::id()
}

fn write(folder: &Path, name: &str, row: &Value) {
    fs::write(folder.join(name), serde_json::to_vec(row).unwrap()).unwrap();
}

#[test]
fn the_folder_is_claude_s_config_folder_s_else_the_home_s() {
    let env = Env::from_vars([("CLAUDE_CONFIG_DIR", "/c"), ("HOME", "/h")]);
    assert_eq!(folder(&env), Some(path::join(&["/c", "sessions"])));
    let env = Env::from_vars([("CLAUDE_CONFIG_DIR", ""), ("HOME", "/h")]);
    assert_eq!(
        folder(&env),
        Some("sessions".to_owned()),
        "an empty one too"
    );
    let env = Env::from_vars([("HOME", "/h")]);
    assert_eq!(
        folder(&env),
        Some(path::join(&["/h", ".claude", "sessions"]))
    );
    assert_eq!(folder(&Env::from_vars::<&str, &str>([])), None);
}

#[test]
fn each_live_claude_says_its_conversation_and_its_state() {
    let dir = tempfile::tempdir().unwrap();
    let pid = me();
    let row = |status: &str| json!({ "pid": pid, "sessionId": "S", "status": status });
    for (status, state) in [
        ("busy", State::Working),
        ("idle", State::Idle),
        ("shell", State::Idle),
        ("waiting", State::Waiting(None)),
    ] {
        write(dir.path(), "1.json", &row(status));
        let expected = Status {
            session: "S".to_owned(),
            state,
        };
        assert_eq!(
            statuses(&dir.path().to_string_lossy()),
            [(pid, expected)],
            "{status}"
        );
    }
    write(
        dir.path(),
        "1.json",
        &json!({ "pid": pid, "sessionId": "S", "status": "waiting", "waitingFor": "permission prompt" }),
    );
    let said = statuses(&dir.path().to_string_lossy());
    assert_eq!(
        said[0].1.state,
        State::Waiting(Some("permission prompt".to_owned()))
    );
}

#[test]
fn a_file_of_no_live_claude_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let pid = me();
    let rows = [
        json!({ "pid": 999_999, "sessionId": "S", "status": "idle" }),
        json!({ "pid": pid, "sessionId": 5, "status": "idle" }),
        json!({ "pid": pid, "status": "idle" }),
        json!({ "pid": f64::from(pid) + 0.5, "sessionId": "S", "status": "idle" }),
        json!({ "pid": "1", "sessionId": "S", "status": "idle" }),
        json!({ "pid": 0, "sessionId": "S", "status": "idle" }),
        json!({ "pid": -1, "sessionId": "S", "status": "idle" }),
        json!({ "pid": 4_294_967_296_u64, "sessionId": "S", "status": "idle" }),
        json!({ "pid": pid, "sessionId": "S", "status": "sleeping" }),
        json!({ "pid": pid, "sessionId": "S", "status": "constructor" }),
        json!({ "pid": pid, "sessionId": "S", "status": ["idle"] }),
        json!({ "pid": pid, "sessionId": "S" }),
        json!(null),
        json!([1]),
    ];
    for row in &rows {
        write(dir.path(), "1.json", row);
        assert_eq!(statuses(&dir.path().to_string_lossy()), [], "{row}");
    }
    fs::write(dir.path().join("1.json"), "{not json").unwrap();
    assert_eq!(statuses(&dir.path().to_string_lossy()), []);
    assert_eq!(statuses(&dir.path().join("missing").to_string_lossy()), []);
}

#[test]
fn only_a_file_named_by_digits_is_read_and_a_process_keeps_its_first_place() {
    let dir = tempfile::tempdir().unwrap();
    let pid = me();
    let row = |session: &str| json!({ "pid": pid, "sessionId": session, "status": "idle" });
    write(dir.path(), "notes.json", &row("notes"));
    write(dir.path(), "1.json.bak", &row("backup"));
    write(dir.path(), ".json", &row("unnamed"));
    fs::create_dir(dir.path().join("3.json")).unwrap();
    assert_eq!(statuses(&dir.path().to_string_lossy()), []);
    write(dir.path(), "1.json", &row("first"));
    write(dir.path(), "2.json", &row("second"));
    let said = statuses(&dir.path().to_string_lossy());
    assert_eq!(said.len(), 1, "one process, in one place");
    assert_eq!(
        said[0].1.session, "second",
        "the later file's status, as Node's Map set it"
    );
}
