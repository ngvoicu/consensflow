use super::*;

fn key(pid: u32) -> ProcessKey {
    ProcessKey {
        pid,
        began: u64::from(pid),
    }
}

/// The process `pid`, as a parent.
fn of(pid: u32) -> Parent {
    Parent::Of(key(pid))
}

/// A process of `program` started by `parent`.
fn process(pid: u32, program: &str, parent: Parent) -> Member {
    Member {
        key: key(pid),
        command: format!("{program} {pid}"),
        program: program.to_owned(),
        exe: String::new(),
        group: None,
        parent,
    }
}

/// The numbers of what is the turn's work among `started`, less what `protected`
/// holds, in order.
fn turned(started: &[Member], protected: &[u32]) -> Vec<u32> {
    let started: Vec<&Member> = started.iter().collect();
    let protected: HashSet<ProcessKey> = protected.iter().map(|pid| key(*pid)).collect();
    let mut turned: Vec<u32> = turns(&started, &protected)
        .iter()
        .map(|key| key.pid)
        .collect();
    turned.sort_unstable();
    turned
}

#[test]
fn a_shell_is_known_by_the_name_of_its_program_with_or_without_the_extension_and_the_case_of_windows(
) {
    for shell in [
        "sh",
        "bash",
        "zsh",
        "bash.exe",
        "BASH.EXE",
        "PowerShell.exe",
        "pwsh.exe",
        "dash",
    ] {
        assert!(is_shell(shell), "{shell}");
    }
    for other in [
        "cmd.exe",
        "node.exe",
        "npx",
        "python3",
        "msedge.exe",
        "ssh",
        "bashful",
        "",
        ".exe",
    ] {
        assert!(!is_shell(other), "{other}");
    }
}

#[test]
fn a_shell_is_told_by_the_file_it_runs_and_not_by_a_name_it_had() {
    // A script a shell runs is run by the shell, and a process that has just run another
    // program may still have the name it had.
    let by = |program: &str, exe: &str| Member {
        exe: exe.to_owned(),
        ..process(1, program, Parent::Gone)
    };
    assert!(runs_a_shell(&by("deploy.sh", "/bin/bash")));
    assert!(runs_a_shell(&by(
        "run",
        r"C:\Program Files\Git\bin\bash.exe"
    )));
    assert!(!runs_a_shell(&by("bash", "/usr/bin/nohup")));
    assert!(!runs_a_shell(&by("bash", "/bin/sleep")));
    // No file said: the name is what there is.
    assert!(runs_a_shell(&by("bash", "")));
    assert!(!runs_a_shell(&by("sleep", "")));
}

#[test]
fn a_shell_the_turn_started_and_what_it_started_are_the_turns_and_nothing_else_is() {
    let started = [
        // The agent's command: a shell, the browser it runs, and what the
        // browser starts.
        process(40, "bash.exe", of(10)),
        process(41, "msedge.exe", of(40)),
        process(42, "msedge.exe", of(41)),
        // A server, begun late: no shell, and what it starts is no shell's.
        process(50, "node.exe", of(10)),
        process(51, "chrome.exe", of(50)),
        // What a `.cmd` shim starts.
        process(60, "cmd.exe", of(10)),
        process(61, "node.exe", of(60)),
    ];
    assert_eq!(turned(&started, &[]), [40, 41, 42]);
}

#[test]
fn a_process_whose_parent_is_gone_is_a_job_someone_detached_and_so_is_what_it_started() {
    let started = [
        // `nohup sleep 600 &` from a shell that ended, and what the job started.
        process(70, "sleep", Parent::Gone),
        process(71, "python3", Parent::Gone),
        process(72, "ffmpeg", of(71)),
        // A parent the platform did not say is no parent that is gone.
        process(73, "sleep", Parent::Unknown),
        // A parent that began before the mark is a process that is nobody's work to
        // tell: the window's program, or a server.
        process(74, "sleep", of(10)),
    ];
    assert_eq!(turned(&started, &[]), [70, 71, 72]);
}

#[test]
fn what_is_protected_is_never_the_turns_though_it_is_a_shell_or_has_no_parent() {
    let started = [
        // A shell a server was wrapped in, and what it started.
        process(80, "sh", of(10)),
        process(81, "node", of(80)),
        // A helper of the harness's own that lost its parent.
        process(82, "codex-helper", Parent::Gone),
        // A job beside them.
        process(83, "bash", of(10)),
    ];
    assert_eq!(turned(&started, &[80, 81, 82]), [83]);
    assert_eq!(turned(&started, &[]), [80, 81, 82, 83]);
}

#[test]
fn a_list_that_names_its_processes_as_each_others_parents_is_no_tree_and_ends() {
    let started = [process(90, "sleep", of(91)), process(91, "sleep", of(90))];
    assert_eq!(turned(&started, &[]), Vec::<u32>::new());
}

#[test]
fn nothing_started_is_none_of_the_turns() {
    assert_eq!(turned(&[], &[]), Vec::<u32>::new());
}
