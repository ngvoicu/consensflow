//! The door's grounds, each acting and saying only: a say-only door writes its line and leaves
//! what it was asked to end or take, and an acting one refuses what has no ground.

// The tests start their own children, stamp their own folders by the clock the door reads
// ages against, and read the switch a child of theirs is started with.
#![allow(clippy::disallowed_methods)]

use std::time::Duration;

use super::*;

/// A say-only door that writes in a file of `folder`, and that file.
fn saying_in(folder: &Path) -> (Door, PathBuf) {
    let file = folder.join("audit.txt");
    (Door::saying(file.clone()), file)
}

fn lines(file: &Path) -> Vec<String> {
    fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn running(pid: u32, started: u64, ppid: u32, command: &str) -> Running {
    Running {
        identity: Identity { pid, started },
        ppid,
        command: command.to_owned(),
        exe: None,
    }
}

#[test]
fn the_door_says_only_where_the_run_asks_and_writes_where_it_is_told() {
    assert_eq!(Door::asked(None, None), Door::acting());
    assert_eq!(Door::asked(Some("0".as_ref()), None), Door::acting());
    assert_eq!(Door::asked(Some("".as_ref()), None), Door::acting());
    assert_eq!(
        Door::asked(Some("1".as_ref()), Some("/x/audit.txt".as_ref())),
        Door::saying(PathBuf::from("/x/audit.txt"))
    );
    let own = Door::asked(Some("1".as_ref()), None);
    assert!(own.says_only());
    assert!(own.says_in().unwrap().starts_with(std::env::temp_dir()));
}

#[test]
fn a_say_only_door_writes_each_process_it_would_end_or_stop_and_ends_none() {
    let folder = tempfile::tempdir().unwrap();
    let (door, file) = saying_in(folder.path());
    let all = [
        running(10, 100, 1, "window"),
        running(11, 101, 10, "sleep 4711"),
    ];
    let chosen = Grounded::tree(&all, all[0].identity);
    assert_eq!(chosen.len(), 2);

    let ended = door.end(&chosen);
    let paused = door.pause(&chosen, Pause::Stop);

    assert!(ended.ended.is_empty() && ended.gone.is_empty() && ended.failed.is_empty());
    assert_eq!(ended.said, chosen.processes());
    assert_eq!(paused.said, chosen.processes());
    assert_eq!(
        ended.problems().len(),
        2,
        "a say-only ending passes: {ended:?}"
    );
    let said = lines(&file);
    assert_eq!(said.len(), 4, "{said:?}");
    assert!(said[0].starts_with("end\tprocess 10 begun 100 (window)\tthe root process 10"));
    assert!(said[1].starts_with("end\tprocess 11 begun 101 (sleep 4711)\tdescends from process 10"));
    assert!(said[2].starts_with("stop\tprocess 10"));
}

#[test]
fn a_say_only_door_kills_no_child_of_its_own_and_closes_no_pane() {
    let folder = tempfile::tempdir().unwrap();
    let (door, file) = saying_in(folder.path());
    let me = std::env::current_exe().unwrap();
    let mut child = std::process::Command::new(&me)
        .args([
            "--exact",
            "door::tests::a_child_that_waits_a_minute",
            "--ignored",
        ])
        .env("CF_E2E_DOOR_CHILD", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let killed = door.end_child("the test's child", &mut child);
    let still = child.try_wait().unwrap().is_none();
    let asked = std::cell::Cell::new(false);
    let closed = door
        .close_pane(&serde_json::json!({ "id": "p1" }), |_, _| {
            asked.set(true);
            Ok(serde_json::json!({}))
        })
        .unwrap();
    // The test's own child is ended by the test, not the door.
    let _ = child.kill();
    let _ = child.wait();

    assert!(!killed);
    assert!(still, "a say-only door killed its child");
    assert!(
        !asked.get(),
        "a say-only door asked the host to close a pane"
    );
    assert_eq!(closed["said"], true);
    let said = lines(&file);
    assert!(said[0].starts_with(&format!("kill\tprocess {} (the test's child)", child.id())));
    assert!(said[1].starts_with("close\tpane \"p1\""));
}

#[test]
fn a_say_only_door_signals_stalls_and_closes_no_terminal_of_its_own() {
    let folder = tempfile::tempdir().unwrap();
    let (door, file) = saying_in(folder.path());
    let me = std::env::current_exe().unwrap();
    let mut child = std::process::Command::new(&me)
        .args([
            "--exact",
            "door::tests::a_child_that_waits_a_minute",
            "--ignored",
        ])
        .env("CF_E2E_DOOR_CHILD", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let signalled = door.signal_child("the test's child", &child, crate::process::Signal::Kill);
    #[cfg(unix)]
    let stalled = door.stall_child("the test's child", &child, Duration::from_millis(100));
    #[cfg(not(unix))]
    let stalled = false;
    let asked = std::cell::Cell::new(false);
    let closed = door.close_own_terminal("its terminal", || asked.set(true));
    std::thread::sleep(Duration::from_millis(200));
    let still = child.try_wait().unwrap().is_none();
    let _ = child.kill();
    let _ = child.wait();

    assert!(!signalled && !stalled && !closed && !asked.get());
    assert!(still, "a say-only door ended its child");
    let said = lines(&file);
    assert!(said[0].starts_with("signal Kill\tprocess "), "{said:?}");
    assert!(
        said.last().unwrap().starts_with("close\tits terminal"),
        "{said:?}"
    );
}

#[test]
fn a_file_named_by_this_run_is_taken_and_one_named_alike_is_left() {
    let folder = made("door-own-name-test-").unwrap();
    let own = folder.path().join(own_name("ticks", "txt"));
    fs::write(&own, "1").unwrap();
    // The same shape of name, but none this process made.
    let alike = folder
        .path()
        .join(format!("ticks-{}-1-999.txt", std::process::id()));
    fs::write(&alike, "1").unwrap();

    let said_folder = tempfile::tempdir().unwrap();
    let (saying, file) = saying_in(said_folder.path());
    assert_eq!(saying.remove_own_named(&own), Removed::Said(own.clone()));
    assert!(own.is_file());
    assert_eq!(lines(&file).len(), 1);

    let door = Door::acting();
    assert!(matches!(door.remove_own_named(&alike), Removed::Left(..)));
    assert_eq!(door.remove_own_named(&own), Removed::Removed(own.clone()));
    assert!(!own.exists() && alike.is_file());
    assert_eq!(door.remove_own_named(&own), Removed::Absent(own.clone()));
}

#[test]
fn only_what_is_below_the_kits_live_folder_of_a_home_is_taken_by_that_ground() {
    let home = tempfile::tempdir().unwrap();
    let live = crate::live::machine::live_root(home.path());
    let case = live.join("a-case");
    fs::create_dir_all(case.join("work")).unwrap();
    fs::write(case.join("log.txt"), "a log").unwrap();
    let beside = home.path().join("notes.txt");
    fs::write(&beside, "the owner's").unwrap();
    let door = Door::acting();

    assert!(matches!(
        door.remove_in_live_root(home.path(), &beside),
        Removed::Left(..)
    ));
    assert!(matches!(
        door.remove_in_live_root(home.path(), &live),
        Removed::Left(..)
    ));
    // Another home's live folder is no ground in this one.
    let other = tempfile::tempdir().unwrap();
    assert!(matches!(
        door.remove_in_live_root(other.path(), &case.join("log.txt")),
        Removed::Left(..)
    ));
    assert_eq!(
        door.remove_in_live_root(home.path(), &case.join("log.txt")),
        Removed::Removed(case.join("log.txt"))
    );
    assert_eq!(
        door.remove_in_live_root(home.path(), &case.join("work")),
        Removed::Removed(case.join("work"))
    );
    assert!(beside.is_file() && live.is_dir());
}

/// What the door's child runs: this test program again, which waits a minute where the test
/// started it and ends at once anywhere else.
#[test]
#[ignore = "a child the door's test starts as a program of its own"]
fn a_child_that_waits_a_minute() {
    if std::env::var_os("CF_E2E_DOOR_CHILD").is_some() {
        std::thread::sleep(Duration::from_secs(60));
    }
}

#[test]
fn an_acting_door_kills_its_child_and_asks_the_host_to_close_the_pane() {
    let door = Door::acting();
    let me = std::env::current_exe().unwrap();
    let mut child = std::process::Command::new(&me)
        .args([
            "--exact",
            "door::tests::a_child_that_waits_a_minute",
            "--ignored",
        ])
        .env("CF_E2E_DOOR_CHILD", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    assert!(door.end_child("the test's child", &mut child));
    assert!(child.wait().is_ok());
    let closed = door
        .close_pane(&serde_json::json!({ "id": "p1" }), |op, body| {
            Ok(serde_json::json!({ "op": op, "body": body }))
        })
        .unwrap();
    assert_eq!(closed["op"], "pane.kill");
    assert_eq!(closed["body"]["id"], "p1");
}

#[test]
fn a_folder_the_door_made_goes_when_it_is_dropped_and_one_kept_stays_marked() {
    let parent = tempfile::tempdir().unwrap();
    let made = made_in(parent.path(), "consensflow-door-test-").unwrap();
    let gone = made.path().to_path_buf();
    fs::write(gone.join("note"), "x").unwrap();
    drop(made);
    assert!(!gone.exists(), "a folder the door made was left");

    let mut kept = made_in(parent.path(), "consensflow-door-test-").unwrap();
    let there = kept.keep();
    drop(kept);
    assert!(there.join(KEPT).is_file(), "a kept folder is not marked");

    let handed = made_in(parent.path(), "consensflow-door-test-")
        .unwrap()
        .hand_on();
    assert!(handed.is_dir());
    assert_eq!(
        Door::acting().remove_made(&handed),
        Removed::Removed(handed.clone())
    );
    assert!(!handed.exists());
}

#[test]
fn the_door_takes_no_folder_it_did_not_make_nor_a_path_under_none() {
    let parent = tempfile::tempdir().unwrap();
    let other = parent.path().join("consensflow-integration-another-run");
    fs::create_dir_all(other.join("workspace")).unwrap();
    let door = Door::acting();

    assert!(matches!(door.remove_made(&other), Removed::Left(..)));
    assert!(matches!(
        door.remove_in_made(&other.join("workspace")),
        Removed::Left(..)
    ));
    assert!(other.join("workspace").is_dir());

    let made = made_in(parent.path(), "consensflow-door-test-").unwrap();
    let inside = made.path().join("labs");
    fs::create_dir_all(inside.join("one")).unwrap();
    // The made folder itself is not a path under it.
    assert!(matches!(
        door.remove_in_made(made.path()),
        Removed::Left(..)
    ));
    assert_eq!(
        door.remove_in_made(&inside),
        Removed::Removed(inside.clone())
    );
    assert!(!inside.exists());
    // A link under it that leads out is taken as the link alone.
    #[cfg(unix)]
    {
        let link = made.path().join("out");
        std::os::unix::fs::symlink(&other, &link).unwrap();
        assert_eq!(door.remove_in_made(&link), Removed::Removed(link.clone()));
        assert!(
            other.join("workspace").is_dir(),
            "what a link led to was taken"
        );
    }
}

#[test]
fn a_say_only_door_leaves_every_path_it_is_asked_to_take_and_writes_each() {
    let parent = tempfile::tempdir().unwrap();
    let (door, file) = saying_in(parent.path());
    let made = made_in(parent.path(), "consensflow-door-test-").unwrap();
    let inside = made.path().join("labs");
    fs::create_dir_all(&inside).unwrap();
    let records = parent.path().join("records");
    let own = made_in(parent.path(), "consensflow-door-own-").unwrap();
    let record = Record::of(&records, own.path()).unwrap();
    fs::create_dir_all(record.path()).unwrap();

    assert_eq!(door.remove_in_made(&inside), Removed::Said(inside.clone()));
    assert_eq!(door.remove_record(&record), Removed::Said(record.path()));
    assert_eq!(
        door.remove_made(made.path()),
        Removed::Said(made.path().to_path_buf())
    );

    assert!(inside.is_dir() && record.path().is_dir() && made.path().is_dir());
    assert!(
        made.path().join(KEPT).is_file(),
        "a said folder is not marked kept"
    );
    let said = lines(&file);
    assert_eq!(said.len(), 3, "{said:?}");
    assert!(said
        .iter()
        .all(|line| line.starts_with("remove\t") && line.contains("(there)")));
    assert!(said[1].ends_with("the record named for the renewal's own folder"));
}

#[test]
fn a_record_is_named_for_the_real_path_of_the_folder_it_is_of() {
    let parent = tempfile::tempdir().unwrap();
    let own = parent.path().join("consensflow-integration-renew-ab_c.1");
    fs::create_dir(&own).unwrap();
    let record = Record::of(Path::new("/records"), &own).unwrap();
    let real = canonical(&own).unwrap();
    assert_eq!(
        record.path(),
        Path::new("/records").join(crate::rig::project_name(&real.to_string_lossy()))
    );
    assert!(record.path().to_string_lossy().ends_with("renew-ab-c-1"));
    assert_eq!(
        Record::of(Path::new("/records"), &parent.path().join("none")),
        None
    );
}

#[test]
fn a_set_of_processes_is_grounded_only_by_the_descent() {
    let all = [
        running(10, 100, 1, "window"),
        running(11, 101, 10, "sh -c sleep 4711"),
        running(12, 102, 11, "sleep 4711"),
        running(13, 90, 10, "sleep 4711"),
        running(20, 200, 1, "sleep 4711 of another run"),
    ];
    let window = all[0].identity;
    let holding = Grounded::holding(&all, window, "4711");
    let pids: Vec<u32> = holding
        .processes()
        .iter()
        .map(|one| one.identity.pid)
        .collect();
    assert_eq!(pids, [11, 12]);
    assert!(!holding.contains(all[4].identity));
    assert!(Grounded::tree(
        &all,
        Identity {
            pid: 99,
            started: 1
        }
    )
    .is_empty());

    let job = running(30, 300, 1, "sleep 61 /rig/jobs/7.job");
    assert!(Grounded::stand_in_job(&job, "/rig/jobs/7.job", 300).is_some());
    assert!(Grounded::stand_in_job(&job, "/rig/jobs/8.job", 300).is_none());
    assert!(Grounded::stand_in_job(&job, "/rig/jobs/7.job", 301).is_none());
    assert!(Grounded::stand_in_job(&job, "", 0).is_none());

    let mut both = holding.clone();
    both.add(holding);
    both.add(Grounded::stand_in_job(&job, "/rig/jobs/7.job", 300).unwrap());
    assert_eq!(both.len(), 3, "a process is held once");
}
