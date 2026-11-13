//! The door's grounds, each acting and saying only: a say-only door writes its line and leaves
//! what it was asked to end or take, and an acting one refuses what has no ground.

// The tests start their own children, stamp their own folders by the clock the door reads
// ages against, and read the switch a child of theirs is started with.
#![allow(clippy::disallowed_methods)]

use std::time::Duration;

use super::*;

/// A say-only door that writes in a file of `folder`, and that file, made as the run makes its
/// own ([`Door::asked`]).
fn saying_in(folder: &Path) -> (Door, PathBuf) {
    let file = folder.join("audit.txt");
    let door = Door::asked(Some("1".as_ref()), Some(file.as_os_str()), None).unwrap();
    (door, file)
}

/// What the lines of the audit file say of acts: those that are no first lines of the run's own.
fn acts(file: &Path) -> Vec<String> {
    lines(file)
        .into_iter()
        .filter(|line| !line.starts_with('#'))
        .collect()
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
fn the_door_says_only_where_the_run_asks_and_writes_in_a_file_it_made_new() {
    assert_eq!(Door::asked(None, None, None), Ok(Door::acting()));
    assert_eq!(
        Door::asked(Some("0".as_ref()), None, None),
        Ok(Door::acting())
    );
    assert_eq!(
        Door::asked(Some("".as_ref()), None, None),
        Ok(Door::acting())
    );
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("said.txt");
    let door = Door::asked(Some("1".as_ref()), Some(file.as_os_str()), None).unwrap();
    assert_eq!(door, Door::saying(file.clone()));
    // The file is the run's own, made as it begins, with first lines that say what the run still does.
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.starts_with("# say-only run"), "{text}");
    assert!(text.contains("is no pass"), "{text}");
    for act in STILL_ACTS {
        assert!(text.contains(&format!("# still acts: {act}")), "{act}");
    }
    assert!(acts(&file).is_empty());
    // A file that is there is never added to: the run that finds one stops, and the file is as it was.
    fs::write(folder.path().join("older.txt"), "an older run's audit\n").unwrap();
    let older = folder.path().join("older.txt");
    let stopped = Door::asked(Some("1".as_ref()), Some(older.as_os_str()), None).unwrap_err();
    assert!(
        stopped.contains("cannot be made as a new file"),
        "{stopped}"
    );
    assert_eq!(
        fs::read_to_string(&older).unwrap(),
        "an older run's audit\n"
    );
    // So does a file that cannot be made at all.
    let nowhere = folder.path().join("no").join("such").join("folder.txt");
    assert!(Door::asked(Some("1".as_ref()), Some(nowhere.as_os_str()), None).is_err());
    // A program the run started (`made`) adds to the run's file and stops where it is not there.
    let child = Door::asked(
        Some("1".as_ref()),
        Some(file.as_os_str()),
        Some("1".as_ref()),
    )
    .unwrap();
    child.say("remove", "/x", "a ground");
    assert_eq!(acts(&file), ["remove\t/x\ta ground"]);
    let gone = folder.path().join("gone.txt");
    let missing = Door::asked(
        Some("1".as_ref()),
        Some(gone.as_os_str()),
        Some("1".as_ref()),
    )
    .unwrap_err();
    assert!(missing.contains("is not there to add to"), "{missing}");
    assert!(!gone.exists(), "a program of the run makes no audit file");
    // Without a file named, the run's own is one of the temp folder, named for its process.
    let own = Door::asked(Some("1".as_ref()), None, Some("1".as_ref()));
    assert!(
        own.is_err(),
        "a program of the run cannot add to a file the run never made"
    );
}

#[test]
fn a_say_only_line_holds_a_whole_command_line_and_is_one_line() {
    let folder = tempfile::tempdir().unwrap();
    let (door, file) = saying_in(folder.path());
    let long = format!("/bin/tool {}\tand\\then\nmore", "x".repeat(600));
    let all = [running(10, 100, 1, &long)];
    let chosen = Grounded::tree(&all, all[0].identity);
    door.end(&chosen);
    let said = acts(&file);
    assert_eq!(said.len(), 1, "{said:?}");
    // The whole command, not a cut of it, with a tab, a backslash and a line break escaped, so that
    // it is one field of one line.
    assert!(said[0].contains(&"x".repeat(600)), "{}", said[0]);
    assert!(said[0].contains("\\tand\\\\then\\nmore"), "{}", said[0]);
    assert_eq!(said[0].split('\t').count(), 3, "{}", said[0]);
}

#[test]
fn a_say_only_door_that_cannot_write_stops_the_run() {
    let folder = tempfile::tempdir().unwrap();
    let (door, file) = saying_in(folder.path());
    fs::remove_file(&file).unwrap();
    let stopped = std::panic::catch_unwind(|| door.say("remove", "/x", "a ground"));
    assert!(stopped.is_err(), "an audit that does not say is no audit");
    assert!(
        !file.exists(),
        "a line is added to the run's file and never makes one"
    );
}

#[test]
fn the_help_text_says_what_a_say_only_run_still_does_as_the_first_lines_do() {
    let help = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/xtask/src/live.rs"),
    )
    .unwrap();
    for act in STILL_ACTS {
        assert!(
            help.contains(act),
            "the help text of live does not say: {act}"
        );
    }
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
    let said = acts(&file);
    assert_eq!(said.len(), 4, "{said:?}");
    assert!(said[0].starts_with("end\tprocess 10 begun 100 (window)\tthe root process 10"));
    assert!(said[1].starts_with("end\tprocess 11 begun 101 (sleep 4711)\tdescends from process 10"));
    assert!(said[2].starts_with("stop\tprocess 10"));
}

#[test]
fn lines_said_by_many_at_once_into_one_file_are_each_whole() {
    // The programs of a run, and the test, add to one file: a line and its break written apart
    // could be cut between by another's line.
    let folder = tempfile::tempdir().unwrap();
    let (door, file) = saying_in(folder.path());
    let long = "x".repeat(3000);
    let door = &door;
    std::thread::scope(|scope| {
        for who in 0..8 {
            let long = &long;
            scope.spawn(move || {
                for at in 0..150 {
                    door.say("remove", &format!("{who}-{at}-{long}"), "ground");
                }
            });
        }
    });
    let said = acts(&file);
    assert_eq!(said.len(), 8 * 150, "a line was cut or run into another");
    for line in &said {
        let parts: Vec<&str> = line.split('\t').collect();
        assert_eq!(parts.len(), 3, "{}", &line[..line.len().min(80)]);
        assert_eq!(parts[0], "remove");
        assert_eq!(parts[2], "ground");
        assert!(parts[1].ends_with(&long), "a line was cut");
    }
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
    let said = acts(&file);
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
    let grouped = door.end_group_of_child("the test's child", &child);
    let quit = door.quit_own_terminal("its terminal", || asked.set(true));
    std::thread::sleep(Duration::from_millis(200));
    let still = child.try_wait().unwrap().is_none();
    let _ = child.kill();
    let _ = child.wait();

    assert!(!signalled && !stalled && !closed && !grouped && !quit && !asked.get());
    assert!(still, "a say-only door ended its child");
    let said = acts(&file);
    assert!(said[0].starts_with("signal Kill\tprocess "), "{said:?}");
    assert!(
        said.iter()
            .any(|line| line.starts_with("kill group\tprocess ")),
        "{said:?}"
    );
    assert!(
        said.last().unwrap().starts_with("quit\tits terminal"),
        "{said:?}"
    );
}

#[test]
fn an_acting_door_ends_the_group_a_child_leads_and_types_the_quit_it_is_given() {
    let me = std::env::current_exe().unwrap();
    let mut command = std::process::Command::new(&me);
    command
        .args([
            "--exact",
            "door::tests::a_child_that_waits_a_minute",
            "--ignored",
        ])
        .env("CF_E2E_DOOR_CHILD", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command.spawn().unwrap();
    let door = Door::acting();
    assert!(door.end_group_of_child("the test's child", &child));
    assert!(child.wait().is_ok());
    let typed = std::cell::Cell::new(false);
    assert!(door.quit_own_terminal("its terminal", || typed.set(true)));
    assert!(typed.get());
}

#[test]
fn a_stand_ins_own_status_file_is_taken_and_no_other_file_is() {
    let parent = tempfile::tempdir().unwrap();
    let sessions = parent.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let own = sessions.join(format!("{}.json", std::process::id()));
    let another = sessions.join(format!("{}.json", std::process::id() ^ 1));
    let elsewhere = parent.path().join(format!("{}.json", std::process::id()));
    for file in [&own, &another, &elsewhere] {
        fs::write(file, "{}").unwrap();
    }
    let acting = Door::acting();
    assert!(matches!(
        acting.remove_status_of_this_process(&another),
        Removed::Left(..)
    ));
    assert!(matches!(
        acting.remove_status_of_this_process(&elsewhere),
        Removed::Left(..)
    ));
    assert!(another.is_file() && elsewhere.is_file());
    assert!(matches!(
        acting.remove_status_of_this_process(&own),
        Removed::Removed(_)
    ));
    assert!(!own.exists());
    assert!(matches!(
        acting.remove_status_of_this_process(&own),
        Removed::Absent(_)
    ));
    // A say-only door says it and leaves it.
    fs::write(&own, "{}").unwrap();
    let (door, file) = saying_in(parent.path());
    assert!(matches!(
        door.remove_status_of_this_process(&own),
        Removed::Said(_)
    ));
    assert!(own.is_file());
    assert!(acts(&file)[0].starts_with("remove\t"), "{:?}", acts(&file));
}

#[test]
fn a_name_the_run_made_for_a_file_is_taken_only_below_a_folder_the_run_made() {
    let folder = made("door-own-name-test-").unwrap();
    let own = folder.path().join(own_name("ticks", "txt"));
    fs::write(&own, "1").unwrap();
    let outside = tempfile::tempdir().unwrap();
    let alike = outside.path().join(own_name("ticks", "txt"));
    fs::write(&alike, "1").unwrap();

    let said_folder = tempfile::tempdir().unwrap();
    let (saying, file) = saying_in(said_folder.path());
    assert_eq!(saying.remove_in_made(&own), Removed::Said(own.clone()));
    assert!(own.is_file());
    assert_eq!(acts(&file).len(), 1);

    let door = Door::acting();
    // The name proves nothing: a file of this process's own kind of name where this run made
    // no folder is left.
    assert!(matches!(door.remove_in_made(&alike), Removed::Left(..)));
    assert_eq!(door.remove_in_made(&own), Removed::Removed(own.clone()));
    assert!(!own.exists() && alike.is_file());
    assert_eq!(door.remove_in_made(&own), Removed::Absent(own.clone()));
}

#[test]
fn a_write_in_the_live_zone_goes_only_below_a_folder_this_run_made() {
    let home = tempfile::tempdir().unwrap();
    let live = crate::live::machine::live_root(home.path());
    let other = live.join("bench");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("AGENTS.md"), "another run's").unwrap();
    let wrong = |path: &Path| crate::files::guard(path).is_err();

    // Another run's folder, a name nobody made, and the live folder itself: refused.
    assert!(wrong(&other.join("AGENTS.md")));
    assert!(wrong(&other.join("evidence").join("new.txt")));
    assert!(wrong(&live.join("fresh").join("a")));
    assert!(wrong(&live));
    assert!(wrong(
        &live.join("..").join("live").join("bench").join("AGENTS.md")
    ));
    assert!(crate::files::write(&other.join("AGENTS.md"), "mine").is_err());
    assert!(crate::files::copy(&other.join("AGENTS.md"), &live.join("copy.txt")).is_err());
    assert!(crate::files::make_dir(&other.join("evidence")).is_err());
    assert_eq!(
        fs::read_to_string(other.join("AGENTS.md")).unwrap(),
        "another run's"
    );

    // A folder this run made there: its own, and what is below it.
    let mine = made_in(&live, "case-").unwrap();
    let file = mine.path().join("sub").join("note.txt");
    crate::files::write(&file, "mine").unwrap();
    crate::files::copy(&file, &mine.path().join("copy.txt")).unwrap();
    assert!(!wrong(&file));
    // The folder itself is no place for a file of its own making, and a link leads elsewhere.
    assert!(wrong(mine.path()));
    crate::files::make_dir(mine.path()).unwrap();
    #[cfg(unix)]
    {
        let link = mine.path().join("link");
        std::os::unix::fs::symlink(other.join("AGENTS.md"), &link).unwrap();
        assert!(crate::files::write(&link, "through").is_err());
        assert_eq!(
            fs::read_to_string(other.join("AGENTS.md")).unwrap(),
            "another run's"
        );
        let up = mine.path().join("up");
        std::os::unix::fs::symlink(&other, &up).unwrap();
        assert!(crate::files::write(&up.join("AGENTS.md"), "through").is_err());
    }

    // Outside the zone nothing changes.
    let elsewhere = tempfile::tempdir().unwrap();
    crate::files::write(&elsewhere.path().join("a").join("b"), "x").unwrap();
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
    let said = acts(&file);
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
