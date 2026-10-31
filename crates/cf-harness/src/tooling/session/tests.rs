//! The newest conversation of a folder, found in each harness's own store.

use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use cf_base::time::{Clock, SystemClock};
use rusqlite::Connection;
use serde_json::json;

use super::*;

/// The environment that has the vars `vars`.
fn env(vars: &[(&str, &str)]) -> Env {
    Env::from_vars(vars.iter().copied())
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
fn a_store_that_is_not_there_has_no_session_and_is_no_failure() {
    let root = tempfile::tempdir().unwrap();
    // Devin's store is in its data folder: `%APPDATA%` on Windows, `XDG_DATA_HOME` elsewhere.
    let data = |folder: &Path| {
        let folder = text(folder);
        env(&[
            ("XDG_DATA_HOME", &folder),
            ("APPDATA", &folder),
            ("HOME", "/nowhere"),
        ])
    };
    let found = newest(Harness::Devin, "devin", "/work/a", 0, &data(root.path()));
    assert_eq!(found, Ok(None));
    // A file that is no store either.
    let not_a_store = root.path().join("devin").join("cli").join("sessions.db");
    fs::create_dir_all(not_a_store.parent().unwrap()).unwrap();
    fs::write(&not_a_store, "this is no database").unwrap();
    let found = newest(Harness::Devin, "devin", "/work/a", 0, &data(root.path()));
    assert_eq!(found, Ok(None));
}

#[test]
fn devins_newest_session_of_a_folder_is_asked_for_by_seconds_and_not_milliseconds() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("devin").join("cli").join("sessions.db");
    fs::create_dir_all(store.parent().unwrap()).unwrap();
    let db = Connection::open(&store).unwrap();
    db.execute_batch(
        "CREATE TABLE sessions (id TEXT PRIMARY KEY, working_directory TEXT, created_at INTEGER);
         INSERT INTO sessions VALUES ('before', '/work/a', 1499);
         INSERT INTO sessions VALUES ('mine', '/work/a', 1500);
         INSERT INTO sessions VALUES ('later', '/work/a', 1700);
         INSERT INTO sessions VALUES ('other', '/work/b', 1800);",
    )
    .unwrap();
    drop(db);
    let data = text(root.path());
    // The store is where Devin keeps it on either system: `%APPDATA%` on Windows,
    // `XDG_DATA_HOME` elsewhere.
    let env = env(&[
        ("XDG_DATA_HOME", &data),
        ("APPDATA", &data),
        ("HOME", "/nowhere"),
    ]);
    let newest =
        |since_ms: i64| newest(Harness::Devin, "devin", "/work/a", since_ms, &env).unwrap();
    // Devin keeps seconds: a window opened at 1,500,900 ms is one of 1,500 s or after it,
    // the second it opened in too, which a conversation opened in it is dated.
    assert_eq!(newest(1_500_900).as_deref(), Some("later"));
    assert_eq!(newest(1_700_999).as_deref(), Some("later"));
    assert_eq!(newest(1_701_000), None, "the next second is after them all");
    assert_eq!(newest(0).as_deref(), Some("later"));
}

/// The time `seconds_ago` seconds ago, in milliseconds since the epoch.
fn ago(seconds_ago: i64) -> i64 {
    SystemClock.now_ms() - seconds_ago * 1000
}

/// A rollout in `folder`, whose first line says it is thread `id` of `cwd`, written
/// `seconds_ago` seconds ago.
fn rollout(folder: &Path, name: &str, id: &str, cwd: &str, seconds_ago: i64) -> PathBuf {
    let file = folder.join(name);
    fs::create_dir_all(folder).unwrap();
    let meta = json!({ "type": "session_meta", "payload": { "id": id, "cwd": cwd } });
    fs::write(&file, format!("{meta}\n{{\"type\":\"response_item\"}}\n")).unwrap();
    let at = UNIX_EPOCH + Duration::from_millis(u64::try_from(ago(seconds_ago)).unwrap());
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(at)
        .unwrap();
    file
}

#[test]
fn codexs_newest_rollout_of_a_folder_since_a_time_names_the_session_of_the_window() {
    let home = tempfile::tempdir().unwrap();
    let day = home
        .path()
        .join("sessions")
        .join("2026")
        .join("10")
        .join("11");
    rollout(&day, "rollout-1-older.jsonl", "older", "/work/a", 50);
    rollout(&day, "rollout-2-newer.jsonl", "newer", "/work/a", 10);
    rollout(&day, "rollout-3-elsewhere.jsonl", "elsewhere", "/work/b", 5);
    rollout(&day, "rollout-4-before.jsonl", "before", "/work/a", 600);
    rollout(&day, "notes.jsonl", "not-a-rollout", "/work/a", 1);
    // A day too deep for Codex's layout is not looked in.
    rollout(
        &day.join("12"),
        "rollout-5-deep.jsonl",
        "deep",
        "/work/a",
        1,
    );
    let codex_home = text(home.path());
    let env = env(&[("CODEX_HOME", &codex_home)]);
    let newest = |workspace: &str, since: i64| {
        newest(Harness::Codex, "codex", workspace, since, &env).unwrap()
    };
    assert_eq!(newest("/work/a", ago(100)).as_deref(), Some("newer"));
    assert_eq!(newest("/work/b", ago(100)).as_deref(), Some("elsewhere"));
    // A window opened half a minute ago has the one written since: never one before it.
    assert_eq!(newest("/work/a", ago(30)).as_deref(), Some("newer"));
    assert_eq!(newest("/work/a", ago(2)), None);
    assert_eq!(newest("/work/c", ago(100)), None);
    // Where Codex has no sessions yet there are none to find.
    let empty = tempfile::tempdir().unwrap();
    let none = newest_in(empty.path());
    assert_eq!(none, None);
}

/// The answer for a Codex home that holds nothing.
fn newest_in(codex_home: &Path) -> Option<String> {
    let home = text(codex_home);
    newest(
        Harness::Codex,
        "codex",
        "/work/a",
        0,
        &env(&[("CODEX_HOME", &home)]),
    )
    .unwrap()
}

#[test]
fn a_rollout_whose_first_line_is_not_a_session_meta_of_the_folder_is_not_the_windows() {
    let folder = tempfile::tempdir().unwrap();
    let wrong_type = folder.path().join("rollout-a.jsonl");
    fs::write(
        &wrong_type,
        "{\"type\":\"response_item\",\"payload\":{\"cwd\":\"/w\",\"id\":\"x\"}}\n",
    )
    .unwrap();
    let broken = folder.path().join("rollout-b.jsonl");
    fs::write(&broken, "{ not json\n").unwrap();
    let empty = folder.path().join("rollout-c.jsonl");
    fs::write(&empty, "").unwrap();
    let ours = rollout(folder.path(), "rollout-d.jsonl", "d", "/w", 0);
    for not_ours in [&wrong_type, &broken, &empty] {
        assert_eq!(session_of(not_ours, "/w"), None, "{}", not_ours.display());
    }
    assert_eq!(session_of(&ours, "/w").as_deref(), Some("d"));
    assert_eq!(session_of(&ours, "/other"), None);
    assert_eq!(session_of(&folder.path().join("missing.jsonl"), "/w"), None);
}

#[test]
fn claude_and_pi_open_on_an_id_they_are_given_so_there_is_none_to_find() {
    for (harness, kind) in [(Harness::Claude, "claude-code"), (Harness::Pi, "pi")] {
        let failed = newest(harness, kind, "/w", 0, &env(&[])).unwrap_err();
        assert_eq!(
            failed,
            format!("{kind} opens on a session id of its own choosing: none to find")
        );
    }
}

#[cfg(unix)]
#[test]
fn a_folder_reached_through_a_link_is_the_same_folder_a_store_names_by_its_real_path() {
    // T-783: a Mac's temporary folders are reached through `/var`, a link to `/private/var`,
    // and a window opens in the folder as the system names it. A store that names either
    // spelling is of the folder asked for by the other.
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    fs::create_dir(&real).unwrap();
    let linked = root.path().join("linked");
    std::os::unix::fs::symlink(&real, &linked).unwrap();
    let (real, linked) = (text(&fs::canonicalize(&real).unwrap()), text(&linked));
    let elsewhere = text(&fs::canonicalize(root.path()).unwrap());

    let data = tempfile::tempdir().unwrap();
    let store = data.path().join("devin").join("cli").join("sessions.db");
    fs::create_dir_all(store.parent().unwrap()).unwrap();
    let db = Connection::open(&store).unwrap();
    db.execute(
        "CREATE TABLE sessions (id TEXT PRIMARY KEY, working_directory TEXT, created_at INTEGER)",
        [],
    )
    .unwrap();
    for (id, folder, at) in [
        ("real", &real, 10),
        ("linked", &linked, 20),
        ("other", &elsewhere, 30),
    ] {
        db.execute("INSERT INTO sessions VALUES (?1, ?2, ?3)", (id, folder, at))
            .unwrap();
    }
    drop(db);
    let devin_data = text(data.path());
    let devin_env = env(&[("XDG_DATA_HOME", &devin_data), ("APPDATA", &devin_data)]);
    for asked in [&real, &linked] {
        assert_eq!(
            newest(Harness::Devin, "devin", asked, 0, &devin_env)
                .unwrap()
                .as_deref(),
            Some("linked"),
            "Devin, asked by {asked}"
        );
    }

    let home = tempfile::tempdir().unwrap();
    let day = home
        .path()
        .join("sessions")
        .join("2026")
        .join("10")
        .join("31");
    rollout(&day, "rollout-1-real.jsonl", "codex-real", &real, 10);
    let codex_home = text(home.path());
    let codex_env = env(&[("CODEX_HOME", &codex_home)]);
    assert_eq!(
        newest(Harness::Codex, "codex", &linked, ago(100), &codex_env)
            .unwrap()
            .as_deref(),
        Some("codex-real"),
        "Codex, its rollout naming the real folder, asked by the link"
    );
}
