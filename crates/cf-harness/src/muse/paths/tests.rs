use std::fs;

use super::*;

fn env_of(vars: &[(&str, &str)]) -> Env {
    Env::from_vars(vars.iter().copied())
}

#[test]
fn the_data_folder_is_xdg_s_and_an_empty_one_is_none_as_muse_reads_it() {
    let at = |vars: &[(&str, &str)]| data(&env_of(vars)).unwrap();
    assert_eq!(
        at(&[("XDG_DATA_HOME", "/x"), ("HOME", "/h")]),
        PathBuf::from("/x/muse")
    );
    let default = PathBuf::from(path::join(&["/h", ".local", "share"])).join("muse");
    assert_eq!(at(&[("XDG_DATA_HOME", ""), ("HOME", "/h")]), default);
    assert_eq!(at(&[("HOME", "/h")]), default);
    assert_eq!(at(&[("USERPROFILE", "/h")]), default);
    assert_eq!(data(&env_of(&[])).unwrap_err(), "missing home in env");
    assert_eq!(
        index(&env_of(&[("XDG_DATA_HOME", "/x")])).unwrap(),
        PathBuf::from("/x/muse/session-index.db")
    );
}

/// Muse on Windows keeps its data in the user's profile (T-708:
/// `C:\Users\AGV\.local\share\muse`) and reads no `HOME` there, which Git's tools and
/// others set; and a folder given in Windows' verbatim form (`\\?\C:\…`, as Muse writes
/// some of its paths) is read as the plain path it names.
#[test]
fn on_windows_the_data_folder_is_in_the_profile_and_a_verbatim_folder_is_read_plain() {
    let windows = |vars: &[(&str, &str)]| {
        let mut all = vec![("OS", "Windows_NT")];
        all.extend_from_slice(vars);
        env_of(&all)
    };
    let profile = r"C:\Users\Zoë";
    let in_profile = PathBuf::from(path::join(&[profile, ".local", "share"])).join("muse");
    let env = windows(&[("HOME", "/c/Users/git"), ("USERPROFILE", profile)]);
    assert_eq!(data(&env).unwrap(), in_profile);
    assert_eq!(
        settings(&env).unwrap(),
        PathBuf::from(path::join(&[profile, ".config"]))
            .join("muse")
            .join("settings.json")
    );
    // A Windows with no profile named: the home all the same.
    let home_only = PathBuf::from(path::join(&["/h", ".local", "share"])).join("muse");
    assert_eq!(data(&windows(&[("HOME", "/h")])).unwrap(), home_only);
    // Elsewhere the home is HOME's, as Muse reads it.
    if !cfg!(windows) {
        assert_eq!(
            data(&env_of(&[("HOME", "/h"), ("USERPROFILE", profile)])).unwrap(),
            home_only
        );
    }
    // XDG's folder, given verbatim, as its plain path.
    let given = windows(&[("XDG_DATA_HOME", r"\\?\C:\Data"), ("USERPROFILE", profile)]);
    assert_eq!(
        data(&given).unwrap(),
        PathBuf::from(r"C:\Data").join("muse")
    );
    let config = windows(&[
        ("XDG_CONFIG_HOME", r"\\?\UNC\server\share\c"),
        ("USERPROFILE", profile),
    ]);
    assert_eq!(
        settings(&config).unwrap(),
        PathBuf::from(r"\\server\share\c")
            .join("muse")
            .join("settings.json")
    );
}

#[test]
fn a_verbatim_path_is_the_drive_or_share_path_it_names_and_any_other_is_as_it_is() {
    assert_eq!(plain(r"\\?\C:\Users\AGV\work"), r"C:\Users\AGV\work");
    assert_eq!(plain(r"\\?\d:"), "d:");
    assert_eq!(plain(r"\\?\UNC\nas\share\x"), r"\\nas\share\x");
    for kept in [
        r"C:\Users\AGV",
        r"\\nas\share",
        r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\x",
        r"\\?\1:\x",
        "/home/a",
        "",
    ] {
        assert_eq!(plain(kept), kept);
    }
}

#[test]
fn a_session_s_log_is_in_the_folder_named_for_it_under_a_date_and_never_a_child_s() {
    let dir = tempfile::tempdir().unwrap();
    // By its real path, as Muse's data folder is read (the temp folder may be reached
    // through a link), in its plain form (Windows' is verbatim).
    let home = PathBuf::from(plain(
        &fs::canonicalize(dir.path()).unwrap().to_string_lossy(),
    ));
    let env = env_of(&[("XDG_DATA_HOME", home.to_str().unwrap())]);
    let sessions = home.join("muse").join("sessions");
    let write = |relative: &str| {
        let file = sessions.join(relative);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "{}\n").unwrap();
        file
    };
    assert_eq!(transcript("s1", &env).unwrap(), None, "no sessions folder");
    // A child agent's log, a copy beside it, a folder of no date: none is the session's.
    write("2026/10/26/parent/subagent/s1/session.jsonl");
    write("2026/10/26/s1/session.jsonl.bak");
    write("drafts/10/26/s1/session.jsonl");
    write("2026/10/26/s10/session.jsonl");
    assert_eq!(transcript("s1", &env).unwrap(), None);
    let log = write("2026/10/26/s1/session.jsonl");
    assert_eq!(transcript("s1", &env).unwrap(), Some(log.clone()));
    // A later date's folder of the same id is not read before the first.
    write("2026/11/01/s1/session.jsonl");
    assert_eq!(transcript("s1", &env).unwrap(), Some(log));
    // Its folder is named as the session is written, on a disk that folds case too.
    assert_eq!(transcript("S1", &env).unwrap(), None);
    for odd in ["", ".", "..", "2026/10/26/s1", "a\\b"] {
        assert_eq!(transcript(odd, &env).unwrap(), None, "{odd:?}");
    }
}

#[test]
fn the_settings_are_xdg_config_s_and_the_home_s_config_folder_s_else() {
    let at = |vars: &[(&str, &str)]| settings(&env_of(vars)).unwrap();
    assert_eq!(
        at(&[("XDG_CONFIG_HOME", "/x"), ("HOME", "/h")]),
        PathBuf::from("/x/muse/settings.json")
    );
    let default = PathBuf::from(path::join(&["/h", ".config"]))
        .join("muse")
        .join("settings.json");
    assert_eq!(at(&[("XDG_CONFIG_HOME", ""), ("HOME", "/h")]), default);
    assert_eq!(at(&[("HOME", "/h")]), default);
}

/// A session log under `day` whose route facts name `pid`.
fn opened(sessions: &Path, day: &str, id: &str, pid: u32) {
    let folder = sessions.join(day).join(id);
    fs::create_dir_all(&folder).unwrap();
    let facts = serde_json::json!({
        "schema_version": 1, "stream": { "kind": "session", "id": id },
        "payload": { "kind": "route_facts", "record": { "cwd": "/work", "pid": pid } },
    });
    fs::write(
        folder.join("session.jsonl"),
        format!(
            "{{\"retained_frame\":true}}\n{{\"payload\":{{\"kind\":\"metadata\"}}}}\n{facts}\n"
        ),
    )
    .unwrap();
}

#[test]
fn a_window_s_session_is_the_newest_its_process_opened_since_its_launch() {
    let home = tempfile::tempdir().unwrap();
    let env = env_of(&[("XDG_DATA_HOME", home.path().to_str().unwrap())]);
    let sessions = home.path().join("muse").join("sessions");
    // UUIDv7s of 2026-10-27: 01a17bc7-6aab-… began at 0x01a17bc76aab ms.
    let launched = 0x01a1_7bc7_6aab_i64;
    assert_eq!(
        opened_by(40_085, launched, &env, &Heads::default()),
        None,
        "no sessions folder"
    );
    // Another process's session, and one of this pid from before the launch.
    opened(
        &sessions,
        "2026/10/27",
        "01a17bc7-6aab-75a0-bfae-b627eaa5ecd8",
        40_086,
    );
    opened(
        &sessions,
        "2026/10/26",
        "01a17000-0000-7000-8000-000000000001",
        40_085,
    );
    assert_eq!(opened_by(40_085, launched, &env, &Heads::default()), None);
    let first = "01a17bc7-6ab0-7000-8000-000000000002";
    opened(&sessions, "2026/10/27", first, 40_085);
    assert_eq!(
        opened_by(40_085, launched, &env, &Heads::default()).as_deref(),
        Some(first)
    );
    // A session it opened later (a /clear) is the one it shows, a day later too.
    let cleared = "01a17bd0-0000-7000-8000-000000000003";
    opened(&sessions, "2026/10/28", cleared, 40_085);
    assert_eq!(
        opened_by(40_085, launched, &env, &Heads::default()).as_deref(),
        Some(cleared)
    );
    // An id that is no UUIDv7 says no time, and is never taken for the window's.
    opened(
        &sessions,
        "2026/10/28",
        "f47ac10b-58cc-4372-a567-0e02b2c3d479",
        40_085,
    );
    assert_eq!(
        opened_by(40_085, launched, &env, &Heads::default()).as_deref(),
        Some(cleared)
    );
}

/// A session under `day` whose log names no process (Muse on Windows writes no route
/// facts) and whose lock says `lock`, as Muse writes it.
fn locked(sessions: &Path, day: &str, id: &str, lock: &str) {
    let folder = sessions.join(day).join(id);
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("session.jsonl"),
        "{\"retained_frame\":true}\n{\"payload\":{\"kind\":\"metadata\",\"record\":{\"workspace_root\":\"C:\\\\w\"}}}\n",
    )
    .unwrap();
    fs::write(folder.join(".session.lock"), lock).unwrap();
}

/// Muse on Windows names no process in a session's log (T-708: no `route_facts`), and a
/// session's folder holds a lock its process keeps while the session is open, naming it
/// (`pid=50544`; a Mac's adds `host=`): a fresh window learns its session by the lock.
/// One closed (an empty lock), another process's, or one begun before the launch, is not
/// the window's; and a log that names its process is believed over its lock.
#[test]
fn where_the_log_names_no_process_the_session_s_lock_names_it() {
    let home = tempfile::tempdir().unwrap();
    let env = env_of(&[("XDG_DATA_HOME", home.path().to_str().unwrap())]);
    let sessions = home.path().join("muse").join("sessions");
    let launched = 0x01a1_7bc7_6aab_i64;
    let look = || opened_by(50_544, launched, &env, &Heads::default());
    locked(
        &sessions,
        "2026/10/26",
        "01a17000-0000-7000-8000-000000000001",
        "pid=50544\n",
    );
    locked(
        &sessions,
        "2026/10/27",
        "01a17bc7-6ab0-7000-8000-000000000002",
        "pid=50545\n",
    );
    locked(
        &sessions,
        "2026/10/27",
        "01a17bc7-6ab1-7000-8000-000000000003",
        "",
    );
    assert_eq!(look(), None);
    let windows = "01a17bc7-6ab2-7000-8000-000000000004";
    locked(&sessions, "2026/10/27", windows, "pid=50544\r\n");
    assert_eq!(look().as_deref(), Some(windows));
    let mac = "01a17bc7-6ab3-7000-8000-000000000005";
    locked(
        &sessions,
        "2026/10/27",
        mac,
        "pid=50544\nhost=a-mac.local\n",
    );
    assert_eq!(look().as_deref(), Some(mac));
    // The log's word first: a session whose route facts name another process is that one's.
    let named = "01a17bc7-6ab4-7000-8000-000000000006";
    opened(&sessions, "2026/10/27", named, 40_085);
    fs::write(
        sessions
            .join("2026/10/27")
            .join(named)
            .join(".session.lock"),
        "pid=50544\n",
    )
    .unwrap();
    assert_eq!(look().as_deref(), Some(mac));
    assert_eq!(
        opened_by(40_085, launched, &env, &Heads::default()).as_deref(),
        Some(named)
    );
}

/// Windows has no `exec`: a launcher runs Muse as its child, and the window's process is
/// the launcher's. Where no session names the window's process, the one whose lock names a
/// process it started is its own; one that names the window's process is believed first.
#[test]
#[allow(clippy::disallowed_methods)] // The test's own program, ended by it.
fn a_session_of_a_process_the_window_s_process_started_is_its_own_where_none_names_it() {
    let home = tempfile::tempdir().unwrap();
    let env = env_of(&[("XDG_DATA_HOME", home.path().to_str().unwrap())]);
    let sessions = home.path().join("muse").join("sessions");
    let launched = 0x01a1_7bc7_6aab_i64;
    let mut child = if cfg!(windows) {
        let mut command = std::process::Command::new("cmd.exe");
        command.args(["/d", "/c", "ping -n 30 127.0.0.1 >nul"]);
        command.spawn().unwrap()
    } else {
        std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap()
    };
    let me = std::process::id();
    let look = || opened_by(me, launched, &env, &Heads::default());
    assert_eq!(look(), None);
    let launched_muse = "01a17bc7-6ab2-7000-8000-000000000004";
    locked(
        &sessions,
        "2026/10/27",
        launched_muse,
        &format!("pid={}\n", child.id()),
    );
    // Another's child, and the process's own child, are told apart by whose it is.
    assert_eq!(
        opened_by(me + 1, launched, &env, &Heads::default()),
        None,
        "a process {} did not start",
        me + 1
    );
    assert_eq!(look().as_deref(), Some(launched_muse));
    let own = "01a17bc7-6ab1-7000-8000-000000000003";
    locked(&sessions, "2026/10/27", own, &format!("pid={me}\n"));
    assert_eq!(look().as_deref(), Some(own), "its own first, though older");
    child.kill().unwrap();
    child.wait().unwrap();
}

/// A closed session's lock is readable and names nobody: it is that, and Windows' Restart
/// Manager is never asked who holds it, at any look (T-985 P2-2: a query of hundreds of
/// milliseconds per closed session begun since the launch, at every readiness look, for
/// a window's life). Only a lock whose words cannot be read (Muse holds it so on Windows)
/// is asked of it; one not there at all names nobody.
#[test]
fn a_closed_session_s_lock_names_nobody_unasked_and_only_one_that_cannot_be_read_is_asked_who_holds_it(
) {
    let home = tempfile::tempdir().unwrap();
    let sessions = home.path().join("muse").join("sessions");
    let day = "2026/10/27";
    let asked = std::cell::Cell::new(0);
    let holders = |_: &Path| {
        asked.set(asked.get() + 1);
        vec![4_242]
    };
    let closed = "01a17bc7-6ab1-7000-8000-000000000003";
    locked(&sessions, day, closed, "");
    let log = |id: &str| sessions.join(day).join(id).join(LOG);
    for _ in 0..2 {
        assert_eq!(head_by(&log(closed), &holders).pid, None);
    }
    assert_eq!(asked.get(), 0, "a closed session's lock is asked of no one");
    let gone = "01a17bc7-6ab2-7000-8000-000000000004";
    locked(&sessions, day, gone, "");
    fs::remove_file(sessions.join(day).join(gone).join(LOCK)).unwrap();
    assert_eq!(head_by(&log(gone), &holders).pid, None);
    assert_eq!(asked.get(), 0, "nor is a lock not there");
    // A lock that cannot be read: here a folder in its place, which no system reads as a
    // file.
    let held = "01a17bc7-6ab3-7000-8000-000000000005";
    locked(&sessions, day, held, "");
    let lock = sessions.join(day).join(held).join(LOCK);
    fs::remove_file(&lock).unwrap();
    fs::create_dir(&lock).unwrap();
    assert_eq!(head_by(&log(held), &holders).pid, Some(4_242));
    assert_eq!(asked.get(), 1);
}

/// Muse on Windows keeps its lock's words from every other reader while it holds it
/// (T-792's probe: "another process has locked a portion of the file"): the process that
/// holds the lock open is the session's. Here a child of the test's holds it, as Muse does
/// as the child of a launcher; a lock nobody holds names nobody.
#[cfg(windows)]
#[test]
#[allow(clippy::disallowed_methods)] // The test's own program, ended by it.
fn on_windows_a_lock_whose_words_cannot_be_read_is_named_by_the_process_that_holds_it() {
    let home = tempfile::tempdir().unwrap();
    let env = env_of(&[("XDG_DATA_HOME", home.path().to_str().unwrap())]);
    let sessions = home.path().join("muse").join("sessions");
    let launched = 0x01a1_7bc7_6aab_i64;
    let closed = "01a17bc7-6ab1-7000-8000-000000000003";
    locked(&sessions, "2026/10/27", closed, "");
    let held = "01a17bc7-6ab2-7000-8000-000000000004";
    locked(&sessions, "2026/10/27", held, "");
    let lock = sessions.join("2026/10/27").join(held).join(".session.lock");
    // One process holds it, as Muse alone holds its lock: no shell with a child that
    // would hold it too. Shared with no reader, so its words cannot be read, as Muse's
    // byte-range lock keeps them: a lock that can be read is a closed session's.
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .append(true)
            .share_mode(0)
            .open(&lock)
            .unwrap()
    };
    assert!(
        fs::read_to_string(&lock).is_err(),
        "its words cannot be read"
    );
    let mut child = std::process::Command::new(r"C:\Windows\System32\PING.EXE")
        .args(["-n", "30", "127.0.0.1"])
        .stdout(file)
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let look = |pid: u32| opened_by(pid, launched, &env, &Heads::default());
    while look(child.id()).is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(look(child.id()).as_deref(), Some(held));
    assert_eq!(
        look(std::process::id()).as_deref(),
        Some(held),
        "its launcher's"
    );
    child.kill().unwrap();
    child.wait().unwrap();
}

/// A session log under `day` started from `previous` by a `/clear`: no route facts.
fn cleared(sessions: &Path, day: &str, id: &str, previous: &str) {
    let folder = sessions.join(day).join(id);
    fs::create_dir_all(&folder).unwrap();
    let started = serde_json::json!({
        "schema_version": 1, "stream": { "kind": "session", "id": id },
        "payload_type": "session.started",
        "payload": { "kind": "session_started", "previous_session_stream": { "kind": "session", "id": previous } },
    });
    fs::write(
        folder.join("session.jsonl"),
        format!("{{\"retained_frame\":true}}\n{started}\n"),
    )
    .unwrap();
}

#[test]
fn a_window_that_was_cleared_shows_the_last_session_of_the_chain_started_from_its_own() {
    let home = tempfile::tempdir().unwrap();
    let env = env_of(&[("XDG_DATA_HOME", home.path().to_str().unwrap())]);
    let sessions = home.path().join("muse").join("sessions");
    let launched = 0x01a1_7bc7_6aab_i64;
    let own = "01a17bc7-6ab0-7000-8000-000000000002";
    opened(&sessions, "2026/10/27", own, 40_085);
    assert_eq!(
        cleared_into(own, launched, &env, &Heads::default()),
        own,
        "never cleared"
    );
    let first = "01a17bc8-0000-7000-8000-000000000003";
    let second = "01a17bc9-0000-7000-8000-000000000004";
    cleared(&sessions, "2026/10/27", first, own);
    cleared(&sessions, "2026/10/27", second, first);
    // Another window's session cleared, and one older than the launch: neither is ours.
    cleared(
        &sessions,
        "2026/10/27",
        "01a17bca-0000-7000-8000-000000000005",
        "01a17000-0000-7000-8000-00000000000a",
    );
    cleared(
        &sessions,
        "2026/10/26",
        "01a17000-0000-7000-8000-000000000006",
        second,
    );
    assert_eq!(cleared_into(own, launched, &env, &Heads::default()), second);
    assert_eq!(
        cleared_into(first, launched, &env, &Heads::default()),
        second
    );
    // A cleared session's log names no process: it is never found by the pid.
    assert_eq!(
        opened_by(40_085, launched, &env, &Heads::default()).as_deref(),
        Some(own)
    );
}

#[test]
fn a_head_that_names_its_process_or_its_previous_session_is_read_once_and_one_that_names_neither_again(
) {
    let home = tempfile::tempdir().unwrap();
    let env = env_of(&[("XDG_DATA_HOME", home.path().to_str().unwrap())]);
    let sessions = home.path().join("muse").join("sessions");
    let launched = 0x01a1_7bc7_6aab_i64;
    let heads = Heads::default();
    // A log Muse has only begun names no process yet: it is read again at the next look.
    let own = "01a17bc7-6ab0-7000-8000-000000000002";
    let folder = sessions.join("2026/10/27").join(own);
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("session.jsonl"), "{\"retained_frame\":true}\n").unwrap();
    assert_eq!(opened_by(40_085, launched, &env, &heads), None);
    opened(&sessions, "2026/10/27", own, 40_085);
    assert_eq!(
        opened_by(40_085, launched, &env, &heads).as_deref(),
        Some(own)
    );
    // Once it names its process, its head never changes, and is not read again: a log
    // rewritten since (none is, by Muse) still says what it said.
    opened(&sessions, "2026/10/27", own, 1);
    assert_eq!(
        opened_by(40_085, launched, &env, &heads).as_deref(),
        Some(own)
    );
    assert_eq!(opened_by(40_085, launched, &env, &Heads::default()), None);
}
