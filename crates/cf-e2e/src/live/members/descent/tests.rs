use super::*;

/// A process `pid`, begun at `started`, started by `ppid`.
fn running(pid: u32, started: u64, ppid: u32, command: &str) -> Running {
    Running {
        identity: Identity { pid, started },
        ppid,
        command: command.to_owned(),
    }
}

#[test]
fn ps_says_each_process_with_its_number_its_parent_when_it_began_and_its_command_line() {
    let listing = "\
  100     1 Fri Oct 16 04:44:46 2026     /Users/me/.local/bin/devin --config /c/config.json
  101   100 Fri Oct  6 04:44:47 2026     node -e \"setTimeout(() => {}, 300000)\"   cf-mark
 not a line
";
    assert_eq!(
        ps_lines(listing),
        [
            running(
                100,
                20_261_016_044_446,
                1,
                "/Users/me/.local/bin/devin --config /c/config.json"
            ),
            running(
                101,
                20_261_006_044_447,
                100,
                "node -e \"setTimeout(() => {}, 300000)\"   cf-mark"
            ),
        ]
    );
    // When a process began grows with the time it says.
    let when = |words: &str| started(&words.split_whitespace().collect::<Vec<_>>());
    assert!(when("Sat Oct 17 00:00:00 2026") > when("Fri Oct 16 23:59:59 2026"));
    assert!(when("Fri Jan  1 00:00:00 2027") > when("Thu Dec 31 23:59:59 2026"));
    assert_eq!(when("Fri Oct 16 04:44:46 2026"), Some(20_261_016_044_446));
    assert_eq!(when("Fri Okt 16 04:44:46 2026"), None);
}

#[test]
fn windows_process_table_is_read_as_json_and_a_command_line_of_many_lines_is_one_process() {
    // Read as lines, an argument that held `7777 4800 cf-mark` on a line of its own made up a
    // process 7777 of the window 4800, and a check ended it (Astra on the B1 fix, the
    // second look).
    let script = table_script("");
    assert!(script.ends_with("| ConvertTo-Json -Compress"), "{script}");
    assert!(
        table_script(" -Filter 'ProcessId=12'").contains("Win32_Process -Filter 'ProcessId=12' |")
    );
    let listing = r#"[{"pid":0,"ppid":0,"started":0,"command":null},{"pid":4800,"ppid":1,"started":133000000000000000,"command":"devin.exe --config c.json"},{"pid":5120,"ppid":4800,"started":133000000000000100,"command":"node.exe -e \"x\"\n7777 4800 cf-mark\nmore"}]"#;
    let read = table_of(listing).unwrap();
    let pids: Vec<u32> = read.iter().map(|one| one.identity.pid).collect();
    assert_eq!(pids, [0, 4800, 5120]);
    assert_eq!(
        read[2].command,
        "node.exe -e \"x\"\n7777 4800 cf-mark\nmore"
    );
    assert_eq!(read[0].command, "");
    // One process alone is an object, none is nothing, and a byte order mark is passed over.
    let one = format!(
        "{}{}",
        char::from_u32(0xFEFF).unwrap(),
        r#"{"pid":12,"ppid":4,"started":5,"command":"x"}"#
    );
    assert_eq!(table_of(&one).unwrap(), [running(12, 5, 4, "x")]);
    assert!(table_of("  \r\n").unwrap().is_empty());
    assert!(table_of("12 4 x").is_err(), "lines are no table");
}

#[test]
fn what_descends_from_a_process_is_what_it_started_and_began_after_it_and_not_itself() {
    let window = Identity {
        pid: 10,
        started: 100,
    };
    let all = [
        running(10, 100, 1, "window"),
        running(11, 101, 10, "agent"),
        running(12, 102, 11, "command"),
        running(13, 100, 10, "server"),
        // It names the window's number as its parent, and began before the window did:
        // its parent was another that had the number before (Windows keeps the number of a
        // parent that ended).
        running(14, 50, 10, "older"),
        running(15, 51, 14, "under the older"),
        running(20, 90, 1, "another"),
        running(21, 91, 20, "its command"),
    ];
    let under = |root: Identity| -> Vec<u32> {
        let mut pids: Vec<u32> = descending_from(&all, root)
            .iter()
            .map(|one| one.identity.pid)
            .collect();
        pids.sort_unstable();
        pids
    };
    assert_eq!(under(window), [11, 12, 13]);
    // The window's number is another's now: nothing descends from the window that was.
    let gone = Identity {
        pid: 10,
        started: 99,
    };
    assert!(under(gone).is_empty());
    assert!(under(Identity {
        pid: 12,
        started: 102
    })
    .is_empty());
}

#[test]
fn this_machine_says_which_process_this_one_is_and_lists_it_as_the_same() {
    let own = std::process::id();
    let identity = ThisMachine
        .identity(own)
        .unwrap()
        .expect("this process is there");
    assert_eq!(identity.pid, own);
    let all = ThisMachine.processes().unwrap();
    assert!(
        all.iter().any(|process| process.identity == identity),
        "{identity:?}"
    );
    // A number no process has is none.
    assert_eq!(ThisMachine.identity(u32::MAX - 7).unwrap(), None);
}

/// The Rust files at `path`: the file, or every one in the folder and the folders in it.
fn rust_files(path: &std::path::Path) -> Vec<std::path::PathBuf> {
    if path.is_file() {
        return if path.extension().is_some_and(|kind| kind == "rs") {
            vec![path.to_owned()]
        } else {
            Vec::new()
        };
    }
    let mut found: Vec<std::path::PathBuf> = std::fs::read_dir(path)
        .unwrap()
        .flat_map(|entry| rust_files(&entry.unwrap().path()))
        .collect();
    found.sort();
    found
}

/// Whether a line of Rust ends a process: sends a signal (`signal`, a system's `kill`), or
/// runs a program that ends processes. A window's own end (the pane host's `pane.kill` of
/// it) and a test's of a program it started (`.kill()`) go by no command line. A comment
/// says.
fn ends_a_process(line: &str) -> bool {
    let code = line.trim_start();
    if code.starts_with("//") {
        return false;
    }
    [
        "signal(",
        "Signal::",
        "::kill(",
        "taskkill",
        "pkill",
        "killall",
        "Stop-Process",
        "kill -",
    ]
    .iter()
    .any(|word| code.contains(word))
}

#[test]
fn no_live_check_ends_a_process_but_through_the_descent() {
    // A check that ended every process whose command line held its mark ended the worker
    // that ran it, whose brief quoted the mark (2026-10-16). A live check ends a process only
    // through `end_chosen` (and `LongCommand::end_in`, which chooses from a window's own
    // tree), each process by which it is, read again before its end: nothing else of the
    // live checks, their support or their suites, sends a signal or runs a program that
    // ends processes.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = rust_files(&root.join("src").join("live"));
    for entry in std::fs::read_dir(root.join("tests")).unwrap() {
        let path = entry.unwrap().path();
        let live = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("live_"));
        if live {
            files.extend(rust_files(&path));
        }
    }
    for suite in [
        "live_resume_cut/case.rs",
        "live_restart_cut.rs",
        "live_wake_ups/run.rs",
        "live_finish/leaving.rs",
        "live_stops/stuck.rs",
        "live_devin_resume/drive.rs",
    ] {
        assert!(
            files.contains(&root.join("tests").join(suite)),
            "{suite} is read"
        );
    }
    // The descent's own end, the one way, and its tests, which name the words.
    let descent = root
        .join("src")
        .join("live")
        .join("members")
        .join("descent");
    let ending: Vec<String> = files
        .iter()
        .filter(|file| !file.starts_with(&descent) && **file != descent.with_extension("rs"))
        .flat_map(|file| {
            let text = std::fs::read_to_string(file).unwrap();
            let shown = file
                .strip_prefix(root)
                .unwrap_or(file)
                .display()
                .to_string();
            text.lines()
                .enumerate()
                .filter(|(_, line)| ends_a_process(line))
                .map(|(at, line)| format!("{shown}:{}: {}", at + 1, line.trim()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        ending.is_empty(),
        "a live check ends a process other than through the descent: {ending:#?}"
    );
    // What the guard takes for an end, and what it does not.
    assert!(ends_a_process("let _ = signal(found.pid, Signal::Kill);"));
    assert!(ends_a_process("    libc::kill(pid, 9);"));
    assert!(ends_a_process(
        r#"Run::new("taskkill").args(["/F", "/PID", &pid])"#
    ));
    assert!(!ends_a_process("        cut.kill();"));
    assert!(!ends_a_process("        kill();"));
    assert!(!ends_a_process("    rig.kill_daemon();"));
    assert!(!ends_a_process(
        "    // signal(pid, Signal::Kill) ended the worker."
    ));
}
