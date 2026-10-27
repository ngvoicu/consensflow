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

#[test]
fn a_session_s_log_is_in_the_folder_named_for_it_under_a_date_and_never_a_child_s() {
    let home = tempfile::tempdir().unwrap();
    let env = env_of(&[("XDG_DATA_HOME", home.path().to_str().unwrap())]);
    let sessions = home.path().join("muse").join("sessions");
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
        opened_by(40_085, launched, &env),
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
    assert_eq!(opened_by(40_085, launched, &env), None);
    let first = "01a17bc7-6ab0-7000-8000-000000000002";
    opened(&sessions, "2026/10/27", first, 40_085);
    assert_eq!(opened_by(40_085, launched, &env).as_deref(), Some(first));
    // A session it opened later (a /clear) is the one it shows, a day later too.
    let cleared = "01a17bd0-0000-7000-8000-000000000003";
    opened(&sessions, "2026/10/28", cleared, 40_085);
    assert_eq!(opened_by(40_085, launched, &env).as_deref(), Some(cleared));
    // An id that is no UUIDv7 says no time, and is never taken for the window's.
    opened(
        &sessions,
        "2026/10/28",
        "f47ac10b-58cc-4372-a567-0e02b2c3d479",
        40_085,
    );
    assert_eq!(opened_by(40_085, launched, &env).as_deref(), Some(cleared));
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
    assert_eq!(cleared_into(own, launched, &env), own, "never cleared");
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
    assert_eq!(cleared_into(own, launched, &env), second);
    assert_eq!(cleared_into(first, launched, &env), second);
    // A cleared session's log names no process: it is never found by the pid.
    assert_eq!(opened_by(40_085, launched, &env).as_deref(), Some(own));
}
