use super::*;
use serde_json::json;

/// This process's id: a process alive.
fn me() -> u32 {
    std::process::id()
}

fn write(folder: &Path, name: &str, row: &Value) {
    fs::write(folder.join(name), serde_json::to_vec(row).unwrap()).unwrap();
}

/// `sessions` in `root` made whole against the working folder.
fn whole(root: &str) -> String {
    let root = std::path::absolute(root).unwrap();
    path::join(&[&root.to_string_lossy(), "sessions"])
}

#[test]
fn the_folder_is_claude_s_config_folder_s_else_the_home_s_made_whole_once() {
    let env = Env::from_vars([("CLAUDE_CONFIG_DIR", "/c"), ("HOME", "/h")]);
    assert_eq!(folder(&env), Ok(whole("/c")));
    let env = Env::from_vars([("CLAUDE_CONFIG_DIR", ""), ("HOME", "/h")]);
    assert_eq!(
        folder(&env),
        Ok(whole(".")),
        "an empty one too: the working folder"
    );
    let env = Env::from_vars([("CLAUDE_CONFIG_DIR", "claude/../config"), ("HOME", "/h")]);
    let said = folder(&env).unwrap();
    assert!(Path::new(&said).is_absolute(), "{said}");
    assert_eq!(
        said,
        whole("config"),
        "its `..` taken off as Node's resolve took it"
    );
    let env = Env::from_vars([("HOME", "/h")]);
    assert_eq!(folder(&env), Ok(whole(&path::join(&["/h", ".claude"]))));
    let env = Env::from_vars([("USERPROFILE", "/profile")]);
    assert_eq!(
        folder(&env),
        Ok(whole(&path::join(&["/profile", ".claude"]))),
        "Windows' own home, where HOME is not set"
    );
    assert_eq!(
        folder(&Env::from_vars::<&str, &str>([])),
        Err("missing home in env".to_owned())
    );
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

/// A live process of the test's own besides the test, ended with it.
struct Other(std::process::Child);

impl Other {
    #[allow(clippy::disallowed_methods)] // The test starts what it ends.
    fn start() -> Self {
        let child = if cfg!(windows) {
            std::process::Command::new("ping")
                .args(["-n", "60", "127.0.0.1"])
                .stdout(std::process::Stdio::null())
                .spawn()
        } else {
            std::process::Command::new("sleep").arg("60").spawn()
        };
        Self(child.unwrap())
    }
}

impl Drop for Other {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn only_a_file_named_by_digits_is_read_and_a_process_keeps_its_first_place() {
    let dir = tempfile::tempdir().unwrap();
    let row =
        |pid: u32, session: &str| json!({ "pid": pid, "sessionId": session, "status": "idle" });
    write(dir.path(), "notes.json", &row(me(), "notes"));
    write(dir.path(), "1.json.bak", &row(me(), "backup"));
    write(dir.path(), ".json", &row(me(), "unnamed"));
    fs::create_dir(dir.path().join("4.json")).unwrap();
    assert_eq!(statuses(&dir.path().to_string_lossy()), []);
    let other = Other::start();
    write(dir.path(), "1.json", &row(me(), "first"));
    write(dir.path(), "2.json", &row(other.0.id(), "second"));
    write(dir.path(), "3.json", &row(me(), "third"));
    let said: Vec<(u32, String)> = statuses(&dir.path().to_string_lossy())
        .into_iter()
        .map(|(pid, status)| (pid, status.session))
        .collect();
    assert_eq!(
        said,
        [(me(), "third".to_owned()), (other.0.id(), "second".to_owned())],
        "a process keeps the place its first file gave it, with its last file's status, as a Map sets a key"
    );
}
