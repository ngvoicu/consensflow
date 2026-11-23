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
        write(dir.path(), &format!("{pid}.json"), &row(status));
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
        &format!("{pid}.json"),
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
    let name = format!("{pid}.json");
    for row in &rows {
        write(dir.path(), &name, row);
        assert_eq!(statuses(&dir.path().to_string_lossy()), [], "{row}");
    }
    fs::write(dir.path().join(&name), "{not json").unwrap();
    assert_eq!(statuses(&dir.path().to_string_lossy()), []);
    assert_eq!(statuses(&dir.path().join("missing").to_string_lossy()), []);
}

/// A live process of the test's own besides the test, ended with it.
struct Other(std::process::Child);

impl Other {
    #[allow(clippy::disallowed_methods)] // The test starts what it ends.
    fn start() -> Self {
        let mut command = if cfg!(windows) {
            let mut ping = std::process::Command::new("ping");
            ping.args(["-n", "60", "127.0.0.1"])
                .stdout(std::process::Stdio::null());
            ping
        } else {
            let mut sleep = std::process::Command::new("sleep");
            sleep.arg("60");
            sleep
        };
        // It lives beside the other tests, which open sockets and pipes: what it was left of
        // theirs it would hold for its minute.
        cf_process::hold_only_the_streams(&mut command);
        Self(command.spawn().unwrap())
    }
}

/// The process of the test's own holds nothing of the test's but its streams: a descriptor left
/// open and not close-on-exec as it starts is not among its own, as `lsof` says.
#[cfg(target_os = "macos")]
#[test]
#[allow(clippy::disallowed_methods)] // `lsof`, asked what it holds.
fn a_process_of_the_tests_own_holds_nothing_of_the_tests_but_its_streams() {
    use std::os::fd::AsRawFd;

    use nix::fcntl::{fcntl, FcntlArg, FdFlag};

    let stray = fs::File::open("/dev/null").unwrap();
    let fd = stray.as_raw_fd();
    fcntl(fd, FcntlArg::F_SETFD(FdFlag::empty())).unwrap();
    let other = Other::start();
    let held = std::process::Command::new("/usr/sbin/lsof")
        .args(["-a", "-p", &other.0.id().to_string(), "-d", &fd.to_string()])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&held.stdout).contains("/dev/null"),
        "it holds it"
    );
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
    let other = Other::start();
    let mut scanner = scanner_of(&[1, 2, 3, 4, me(), other.0.id()]);
    assert_eq!(scanner.statuses(&dir.path().to_string_lossy()), []);
    write(dir.path(), "1.json", &row(me(), "first"));
    write(dir.path(), "2.json", &row(other.0.id(), "second"));
    write(dir.path(), "3.json", &row(me(), "third"));
    let said: Vec<(u32, String)> = scanner
        .statuses(&dir.path().to_string_lossy())
        .into_iter()
        .map(|(pid, status)| (pid, status.session))
        .collect();
    assert_eq!(
        said,
        [(me(), "third".to_owned()), (other.0.id(), "second".to_owned())],
        "a process keeps the place its first file gave it, with its last file's status, as a Map sets a key"
    );
}

#[test]
fn the_holders_of_a_conversation_are_the_live_claudes_that_show_it_but_a_parked_job() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().to_string_lossy().into_owned();
    let pid = me();
    let mut scanner = scanner_of(&[1, 2, pid]);
    assert_eq!(scanner.holders(&folder, "S"), Some(Vec::new()), "none yet");
    write(
        dir.path(),
        "1.json",
        &json!({ "pid": pid, "sessionId": "S", "status": "busy" }),
    );
    write(
        dir.path(),
        "2.json",
        &json!({ "pid": pid, "sessionId": "T", "status": "idle" }),
    );
    assert_eq!(scanner.holders(&folder, "S"), Some(vec![pid]));
    assert_eq!(scanner.holders(&folder, "U"), Some(Vec::new()));
    write(
        dir.path(),
        "1.json",
        &json!({ "pid": pid, "sessionId": "S", "status": "busy", "parkedJobId": "job-1" }),
    );
    assert_eq!(
        scanner.holders(&folder, "S"),
        Some(Vec::new()),
        "a parked job"
    );
    write(
        dir.path(),
        "1.json",
        &json!({ "pid": u32::MAX - 1, "sessionId": "S", "status": "busy" }),
    );
    assert_eq!(
        scanner.holders(&folder, "S"),
        Some(Vec::new()),
        "a process gone"
    );
    let missing = dir.path().join("none").to_string_lossy().into_owned();
    assert_eq!(scanner.holders(&missing, "S"), None, "no folder to read");
}

/// A window that was ended left its status file, and its number went to another program that
/// began after the file was last written: Windows gives a number back at once and to any
/// program. That program is no Claude showing the conversation, and the next window on it is
/// its only holder; read as a second holder, its record was "shared" and no answer of the next
/// window's was taken for its task's result (zeewin, the live matrix's field7 and field6 pair,
/// T-1226). A Claude that began before its file was last written is the one that wrote it.
#[test]
fn a_status_file_whose_number_a_later_program_took_names_no_claude() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().to_string_lossy().into_owned();
    let left = Other::start();
    let pid = left.0.id();
    let name = format!("{pid}.json");
    write(
        dir.path(),
        &name,
        &json!({ "pid": pid, "sessionId": "S", "status": "idle" }),
    );
    // The file was last written an hour ago: by a process of that number then, not this one.
    let written = fs::metadata(dir.path().join(&name))
        .unwrap()
        .modified()
        .unwrap();
    fs::File::options()
        .write(true)
        .open(dir.path().join(&name))
        .unwrap()
        .set_modified(written - std::time::Duration::from_secs(3600))
        .unwrap();
    assert_eq!(holders(&folder, "S"), Some(Vec::new()), "a later program");
    assert_eq!(statuses(&folder), []);
    // Written now, while it runs: it is the one that wrote it.
    write(
        dir.path(),
        &name,
        &json!({ "pid": pid, "sessionId": "S", "status": "idle" }),
    );
    assert_eq!(holders(&folder, "S"), Some(vec![pid]));
    drop(left);
}

/// A pid no process holds: a child that was started and has ended.
#[allow(clippy::disallowed_methods)] // The test starts what it ends.
fn ended_pid() -> u32 {
    let mut child = std::process::Command::new(if cfg!(windows) { "cmd" } else { "true" });
    if cfg!(windows) {
        child.args(["/c", "exit"]);
    }
    let mut child = child.spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

#[test]
fn a_live_file_rewritten_in_place_is_never_skipped_whatever_its_listing_says() {
    // The file of a process found gone, then the number taken by a live Claude that writes the
    // file anew with the same byte count, and a time the listing cannot tell from the old one
    // (NTFS lists a stale time; a clock that has not moved): no stamp is a word on the file.
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().to_string_lossy().into_owned();
    let mut scanner = Scanner::default();
    let file = dir.path().join(format!("{}.json", me()));
    let dead = ended_pid();
    let gone = format!(r#"{{"pid":{dead},"sessionId":"S","status":"idle"}}"#);
    let live = format!(r#"{{"pid":{},"sessionId":"S","status":"busy"}}"#, me());
    // The same byte count, whatever the pids' widths: the shorter text is padded.
    let width = gone.len().max(live.len());
    let (gone, live) = (format!("{gone:width$}"), format!("{live:width$}"));
    assert_eq!(gone.len(), live.len(), "pids {dead} and {}", me());
    fs::write(&file, gone).unwrap();
    let stamp = fs::metadata(&file).unwrap().modified().unwrap();
    assert_eq!(scanner.statuses(&folder), []);
    fs::write(&file, live).unwrap();
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(stamp)
        .unwrap();
    let found = scanner.statuses(&folder);
    assert_eq!(found.len(), 1, "a live process's file is read: {found:?}");
}

#[cfg(unix)]
#[test]
fn a_status_link_whose_target_is_rewritten_is_read_as_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    fs::write(&target, r#"{"pid":999999,"sessionId":"S","status":"idle"}"#).unwrap();
    let folder = dir.path().join("sessions");
    fs::create_dir(&folder).unwrap();
    let link = folder.join(format!("{}.json", me()));
    std::os::unix::fs::symlink(&target, &link).unwrap();
    // The link is older than the live process, which a target's rewrite does not change.
    let written = fs::metadata(&target).unwrap().modified().unwrap();
    let hour_ago = nix::sys::time::TimeSpec::new(
        written
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            - 3600,
        0,
    );
    nix::sys::stat::utimensat(
        None,
        &link,
        &hour_ago,
        &hour_ago,
        nix::sys::stat::UtimensatFlags::NoFollowSymlink,
    )
    .unwrap();
    let folder = folder.to_string_lossy().into_owned();
    let mut scanner = Scanner::default();
    assert_eq!(scanner.holders(&folder, "S"), Some(Vec::new()));
    fs::write(
        &target,
        format!(r#"{{"pid":{},"sessionId":"S","status":"busy"}}"#, me()),
    )
    .unwrap();
    assert_eq!(scanner.holders(&folder, "S"), Some(vec![me()]));
}

/// The count above is every call only while every call is counted: each file-system call of the
/// scan sits beside a count of it (the metadata call too: the first count of this look, T-2052,
/// left it out and read 2 where a look made 1,026).
#[test]
fn every_file_system_call_of_the_scan_is_counted() {
    let source = include_str!("../status.rs");
    let from = source.find("    fn scan(&mut self").unwrap();
    let to = source[from..].find("\n/// How much later").unwrap() + from;
    let scan = &source[from..to];
    let calls: usize = [
        "entries(",
        "File::open(",
        "read_to_end(",
        ".metadata(",
        "fs::read(",
        "fs::metadata(",
        ".file_type(",
        "read_link(",
        "canonicalize(",
        "exists(",
    ]
    .iter()
    .map(|call| scan.matches(call).count())
    .sum();
    assert_eq!(calls, scan.matches("fs_calls += 1").count(), "{scan}");
}

/// A scanner that takes the numbers `alive` for alive and no other, the files it reads named by
/// them.
fn scanner_of(alive: &[u32]) -> Scanner {
    ALIVE.with(|held| *held.borrow_mut() = alive.to_vec());
    Scanner::asking(says_alive)
}

thread_local! {
    /// The numbers a test says are alive, for the scanner that asks [`says_alive`].
    static ALIVE: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn says_alive(pid: u32) -> bool {
    ALIVE.with(|alive| alive.borrow().contains(&pid))
}

/// One look at a folder holding the file of the one live Claude (this test's process) and 512 of
/// ended windows, each under a number of its own: every call it makes of the file system, and
/// every process it asks of.
#[test]
fn a_look_asks_once_of_each_file_and_opens_only_the_files_of_live_numbers() {
    let dir = tempfile::tempdir().unwrap();
    for number in 0..512u32 {
        let pid = 100_000 + number;
        let row = json!({ "pid": pid, "sessionId": "OLD", "status": "idle" });
        write(dir.path(), &format!("{pid}.json"), &row);
    }
    let own = json!({ "pid": me(), "sessionId": "S", "status": "busy" });
    write(dir.path(), &format!("{}.json", me()), &own);
    let folder = dir.path().to_string_lossy().into_owned();
    let mut scanner = scanner_of(&[me()]);
    for look in ["the first", "a warm one"] {
        let (calls, asks, reads) = (scanner.fs_calls, scanner.alive_asks, scanner.reads);
        let found = scanner.statuses(&folder);
        assert_eq!(found.len(), 1, "{look}: {found:?}");
        assert_eq!(
            scanner.fs_calls - calls,
            4,
            "{look}: the listing, and the live file's open, read and time"
        );
        assert_eq!(scanner.reads - reads, 1, "{look}: one file opened");
        assert_eq!(
            scanner.alive_asks - asks,
            512 + 1 + 1,
            "{look}: a question for each of the 513 names and one for the process the live file says"
        );
    }
}
