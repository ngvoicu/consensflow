use super::*;

fn at(pid: u32, ppid: u32) -> Row {
    Row {
        pid,
        ppid,
        state: "S".into(),
        command: "x".into(),
    }
}

#[test]
fn reads_pss_rows_pid_parent_state_and_the_whole_command() {
    let rows = parse_table(
        "  101     1 Ss   /a/b app --flag\n 202   101 S+   /x y z\nnot a row\n  303   202 Z    (defunct)\n",
    );
    let row = |pid, ppid, state: &str, command: &str| Row {
        pid,
        ppid,
        state: state.into(),
        command: command.into(),
    };
    assert_eq!(
        rows,
        [
            row(101, 1, "Ss", "/a/b app --flag"),
            row(202, 101, "S+", "/x y z"),
            row(303, 202, "Z", "(defunct)"),
        ]
    );
}

#[test]
fn takes_no_line_that_is_not_a_row() {
    for line in [
        "",
        "   ",
        "101",
        "101 1",
        "101 1 S",
        "101 1 S\n",
        "a 1 S cmd",
        "101 b S cmd",
        "-1 1 S cmd",
    ] {
        assert_eq!(parse_table(line), [], "{line:?}");
    }
    // A command that has spaces, and one that has digits where a pid is, stay whole.
    let rows = parse_table("7 1 S  a  b 8 9\n");
    assert_eq!(rows[0].command, "a  b 8 9");
}

#[test]
fn finds_everything_under_a_process_its_children_theirs_a_chain_of_them() {
    let table = [at(1, 0), at(2, 1), at(3, 2), at(4, 3), at(5, 1), at(9, 8)];
    let mut under = descendants(&table, 1);
    under.sort_unstable();
    assert_eq!(under, [2, 3, 4, 5]);
    assert_eq!(descendants(&table, 3), [4]);
    assert_eq!(descendants(&table, 7), Vec::<u32>::new());
}

#[test]
fn a_wait_ends_with_what_it_found_or_says_what_did_not_happen() {
    let mut looked = 0;
    let found = until("a thing", Duration::from_secs(10), || {
        looked += 1;
        Ok((looked == 3).then_some(looked))
    })
    .unwrap();
    assert_eq!(found, 3);

    let said = until::<()>(
        "a thing that never comes",
        Duration::from_millis(250),
        || Ok(None),
    )
    .unwrap_err();
    assert_eq!(
        said.to_string(),
        "a thing that never comes did not happen within 250 ms"
    );

    // A check that fails ends the wait at once, with its own words.
    let failed = until::<()>("a thing", Duration::from_secs(10), || {
        Err(Error::new("the check failed"))
    })
    .unwrap_err();
    assert_eq!(failed.to_string(), "the check failed");
}

#[test]
fn the_waits_of_a_case_are_given_each_their_time_and_all_of_them_twice_that() {
    let waits = Waits::new(Duration::from_millis(200));
    assert_eq!(waits.each(), Duration::from_millis(200));
    // With no case to end, a wait is given what it is.
    assert!(waits.until::<()>("a", || Ok(None)).is_err());

    // A case that has used up its time leaves nothing to wait with.
    let spent = Waits {
        each: Duration::from_secs(600),
        deadline: Some(Instant::now()),
    };
    let started = Instant::now();
    let said = spent.until::<()>("a", || Ok(None)).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(said.to_string().starts_with("a did not happen within "));
    assert!(Waits::new(Duration::from_secs(1))
        .for_a_case()
        .deadline
        .is_some());
}

#[cfg(unix)]
mod on_unix {
    use std::process::Stdio;

    use super::*;

    /// The process a test started, ended with what is under it whatever the test found.
    struct Tree(Option<std::process::Child>);

    impl Drop for Tree {
        fn drop(&mut self) {
            if let Some(mut child) = self.0.take() {
                if let Some(started) = Started::of(child.id()) {
                    kill_tree(&started);
                }
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    /// How long the sleeps of a test run if nothing ends them: longer than any wait
    /// for them to be gone, or a kill that did nothing would pass by their own end.
    const SLEEP: &str = "600";

    #[test]
    fn ends_a_process_and_all_that_is_under_it_which_a_group_does_not_hold() {
        // A shell with two sleeps under it, and a shell under that with another: three levels.
        let shell = Invocation::new("/bin/sh", Path::new(".")).args([
            "-c",
            &format!("/bin/sleep {SLEEP} & /bin/sh -c \"/bin/sleep {SLEEP} & wait\" & wait"),
        ]);
        let started = process::spawn(&shell, &Env::default(), Stdio::null(), false).unwrap();
        let root = started.id();
        let _tree = Tree(Some(started));
        let patience = Duration::from_secs(60);
        let mut tree = vec![root];
        until("the tree is up", patience, || {
            let under = descendants(&process_table()?, root);
            if under.len() < 3 {
                return Ok(None);
            }
            tree.extend(under);
            Ok(Some(()))
        })
        .unwrap();

        kill_tree(&Started::of(root).unwrap());

        until("the tree is gone", patience, || {
            Ok(tree.iter().all(|pid| !alive(*pid)).then_some(()))
        })
        .unwrap();
    }

    #[test]
    fn a_process_is_alive_while_it_runs_and_not_once_it_is_ended_or_when_it_is_none() {
        let sleeping = Invocation::new("/bin/sleep", Path::new(".")).arg(SLEEP);
        let mut child = process::spawn(&sleeping, &Env::default(), Stdio::null(), false).unwrap();
        let pid = child.id();
        assert!(alive(pid));
        assert!(!gone(pid));
        kill(&Started::of(pid).unwrap());
        // Ended and not yet reaped is a zombie, which is not alive.
        until("the sleep is gone", Duration::from_secs(60), || {
            Ok(gone(pid).then_some(()))
        })
        .unwrap();
        child.wait().unwrap();
        assert!(!alive(pid));
        assert!(!alive(0));
        assert!(!alive(u32::MAX));
    }
}

/// The apps an update starts are the installed bundle's executable, under either name of its
/// folder: not its `cf`, nor another bundle's app.
#[test]
fn the_app_of_a_bundle_is_found_under_either_name_of_its_folder() {
    let table = parse_table(concat!(
        "  11     1 S    /var/x/Applications/ConsensFlow.app/Contents/MacOS/app\n",
        "  12     1 S    /private/var/x/Applications/ConsensFlow.app/Contents/MacOS/app --flag\n",
        "  13    11 S    /var/x/Applications/ConsensFlow.app/Contents/Resources/cli/bin/cf ui --json\n",
        "  14     1 S    /Applications/ConsensFlow.app/Contents/MacOS/app\n",
        "  15     1 S    /var/x/Applications/ConsensFlow.app/Contents/MacOSX/app\n",
    ));
    let programs = [
        "/var/x/Applications/ConsensFlow.app/Contents/MacOS/app".to_string(),
        "/private/var/x/Applications/ConsensFlow.app/Contents/MacOS/app".to_string(),
    ];
    assert_eq!(started_from(&table, &programs), [11, 12]);
}

/// A bridge release's bundle holds the Node its daemon runs on in `Contents/MacOS/`, beside the
/// app. That daemon is running when the update is installed, and is no start of the update's app
/// (T-1338: the bridge leg of the smoke read it as a second start).
#[test]
fn a_program_the_bundle_ships_beside_its_app_is_no_start_of_the_app() {
    let bundle = "/var/x/Applications/ConsensFlow.app";
    let table = parse_table(&format!(
        concat!(
            "  21     1 S    {0}/Contents/MacOS/app\n",
            "  22    21 S    {0}/Contents/MacOS/node {0}/Contents/Resources/cli/cf.mjs ui --json --no-open\n",
            "  23     1 S    {0}/Contents/MacOS/app-helper\n",
            "  24     1 S    {0}/Contents/MacOS/application\n",
        ),
        bundle
    ));
    let app = programs_of(Path::new(&format!("{bundle}/Contents/MacOS/app")));
    assert_eq!(started_from(&table, &app), [21]);
}

/// Where the bundle's folder is a link (a temporary folder's `/var`), the process table may show
/// the program under the path the system resolves it to, and the app is found under both.
#[cfg(unix)]
#[test]
fn a_program_is_known_under_the_path_the_system_resolves_it_to() {
    let folder = tempfile::tempdir().unwrap();
    let real = folder.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let binary = real.join("app");
    std::fs::write(&binary, "").unwrap();
    let link = folder.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let names = programs_of(&link.join("app"));
    let resolved = std::fs::canonicalize(&binary).unwrap();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&link.join("app").display().to_string()));
    assert!(names.contains(&resolved.display().to_string()));
    // A program that is not there is known under the one name it was given.
    assert_eq!(
        programs_of(&folder.path().join("nothing")),
        [folder.path().join("nothing").display().to_string()]
    );
}
