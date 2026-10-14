use super::*;

fn key(pid: u32) -> ProcessKey {
    ProcessKey {
        pid,
        began: u64::from(pid),
    }
}

/// The process `pid`, as a parent.
fn of(pid: u32) -> Option<ProcessKey> {
    Some(key(pid))
}

/// A process of `program` started by `parent`.
fn process(pid: u32, program: &str, parent: Option<ProcessKey>) -> Member {
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
        ..process(1, program, None)
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
fn a_shell_the_harness_started_and_what_it_started_are_the_turns_and_nothing_else_is() {
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
fn a_process_with_no_parent_left_is_no_job_whatever_it_is_and_nor_is_what_it_started() {
    let started = [
        // `nohup sleep 600 &` from a shell that ended, and what the job started: no
        // chain of processes that run leads from them to a shell.
        process(70, "sleep", None),
        process(71, "python3", None),
        process(72, "ffmpeg", of(71)),
        // A shell whose parent has gone is no more: it is not proved the harness's.
        process(73, "bash", None),
        process(74, "sleep", of(73)),
        // A parent that began before the mark, which is nobody's work to tell: the
        // window's program, or a server.
        process(75, "sleep", of(10)),
    ];
    assert_eq!(turned(&started, &[]), Vec::<u32>::new());
}

#[test]
fn a_shell_that_a_server_the_harness_started_late_started_is_not_proved_the_harnesss_own() {
    let started = [
        // A server, begun late, that runs a shell, and what the shell runs.
        process(80, "node", of(10)),
        process(81, "sh", of(80)),
        process(82, "git", of(81)),
        // The agent's command beside it.
        process(83, "zsh", of(10)),
        process(84, "git", of(83)),
        // A shell the agent's shell started, and what that runs.
        process(85, "bash", of(83)),
        process(86, "sleep", of(85)),
    ];
    assert_eq!(turned(&started, &[]), [83, 84, 85, 86]);
}

#[test]
fn what_is_protected_is_never_the_turns_though_it_is_a_shell() {
    let started = [
        // A shell a server was wrapped in, and what it started.
        process(80, "sh", of(10)),
        process(81, "node", of(80)),
        // A job beside them.
        process(83, "bash", of(10)),
    ];
    assert_eq!(turned(&started, &[80, 81]), [83]);
    assert_eq!(turned(&started, &[]), [80, 81, 83]);
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
