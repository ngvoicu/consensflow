use super::testing::{
    a_window_among_strangers, identity_of, listed, the_tests_own_parent, the_windows, Machine,
    DEEP, JOB, LOST, PROGRAM, SHELL, USERS_SHELL,
};
use super::*;

/// The numbers of what `found` names, in order.
fn pids(found: &HashMap<i32, Identity>) -> Vec<i32> {
    let mut pids: Vec<i32> = found.keys().copied().collect();
    pids.sort_unstable();
    pids
}

#[test]
fn a_lineage_is_its_processes_that_run_and_what_descends_from_them_or_from_one_that_has_gone() {
    let lineage = Lineage {
        identities: vec![
            // A shell that runs, and one that has gone.
            listed(40, 400, 1).identity,
            listed(41, 410, 1).identity,
        ],
        members: Vec::new(),
    };
    let snapshot = [
        listed(40, 400, 1),
        // What the shell that runs started, and what that started.
        listed(50, 500, 400),
        listed(51, 510, 500),
        // What the shell that has gone started, still parented by its identifier.
        listed(52, 520, 410),
        // The window's own new work: a shell its program started, and what that runs.
        listed(60, 600, 1),
        listed(61, 610, 600),
        // A process that took the number of the one that has gone.
        listed(41, 411, 1),
    ];
    assert_eq!(
        pids(&lineage.found(&snapshot, &HashMap::new())),
        [40, 50, 51, 52],
        "the new work is no part of it, though a look of the tree would call it the turn's"
    );
}

#[test]
fn what_a_process_the_ending_found_started_is_found_whether_or_not_that_process_runs() {
    let lineage = Lineage {
        identities: vec![listed(40, 400, 1).identity],
        members: Vec::new(),
    };
    // A handler of the shell's started a process and went; the ending knows it by
    // the identity it found, and what that started is the lineage's too.
    let found_before: HashMap<i32, Identity> = HashMap::from([(70, listed(70, 700, 400).identity)]);
    let snapshot = [listed(71, 710, 700), listed(72, 720, 999)];
    assert_eq!(pids(&lineage.found(&snapshot, &found_before)), [71]);
}

#[test]
fn the_session_is_the_panes_while_its_number_is_nobody_elses() {
    let leader = Process {
        session: Some(500),
        ..listed(500, 5000, 1)
    };
    let in_session = |pid, unique| Process {
        session: Some(500),
        ..listed(pid, unique, 1)
    };
    let mut tree = ProcessTree::new();
    tree.root = Some(leader.identity);
    tree.session = Some(500);
    let elsewhere = Process {
        session: Some(900),
        ..listed(502, 5020, 1)
    };

    // The leader runs: what is in its session is the pane's.
    let with_leader = [leader, in_session(501, 5010), elsewhere];
    assert_eq!(
        pids(&tree.select(&with_leader, &HashMap::new())),
        [500, 501]
    );

    // The leader has gone and nothing holds its number: what is left in the session
    // is still the pane's, which a close ends.
    let gone = [in_session(501, 5010), in_session(503, 5030)];
    assert_eq!(pids(&tree.select(&gone, &HashMap::new())), [501, 503]);

    // Another process has the number, and leads a session of its own: what is in a
    // session of that number is not the pane's to tell.
    let taken = [
        Process {
            session: Some(500),
            ..listed(500, 5001, 1)
        },
        in_session(501, 5010),
    ];
    assert!(tree.select(&taken, &HashMap::new()).is_empty());
}

// What the tree signals, on a machine made up (`testing`): a table of the window among
// strangers, and a record of the signals sent to it. No process there is is signalled.

/// The tree of the window of [`a_window_among_strangers`]: its program leads its session.
fn tree_of(table: &[Process]) -> ProcessTree {
    let mut tree = ProcessTree::new();
    tree.root = Some(identity_of(table, PROGRAM));
    tree.session = Some(PROGRAM);
    tree.bound = Bound::new(tree.root);
    tree
}

/// A selection gone wrong: a lineage that names every process of the machine, as a
/// plant of the selection once did, and the ending signalled them all.
fn every_process(table: &[Process]) -> Lineage {
    Lineage {
        identities: table.iter().map(|process| process.identity).collect(),
        members: Vec::new(),
    }
}

/// The numbers of `table`'s processes the test names `of`, as the members of a lineage.
fn members_of(table: &[Process], of: &[i32]) -> Vec<Member> {
    of.iter()
        .map(|pid| Member {
            key: key_of(identity_of(table, *pid)),
            command: format!("sleep {pid}"),
            program: "sleep".to_owned(),
            exe: String::new(),
            group: None,
            parent: None,
        })
        .collect()
}

#[test]
fn a_selection_of_every_process_of_the_machine_asks_only_the_windows_own_to_end() {
    let machine = Machine::new(a_window_among_strangers());
    let table = machine.processes().expect("the table");
    let tree = tree_of(&table);
    let wrong = every_process(&table);

    let asked = tree
        .ask_by(&machine, Subject::Lineage(&wrong), None)
        .expect("asked");

    assert_eq!(machine.sent(libc::SIGTERM), the_windows());
    assert_eq!(machine.sent(libc::SIGHUP), the_windows());
    let waited_for: Vec<i32> = asked.identities.iter().map(|asked| asked.pid).collect();
    assert_eq!(waited_for, the_windows(), "no stranger is waited for");
    assert!(!asked.failed());
}

#[test]
fn a_selection_of_every_process_of_the_machine_stops_and_kills_only_the_windows_own() {
    let machine = Machine::new(a_window_among_strangers());
    let table = machine.processes().expect("the table");
    let tree = tree_of(&table);
    let wrong = every_process(&table);

    let forced = tree.force_by(&machine, Subject::Lineage(&wrong), &[]);

    assert!(forced.failure.is_none(), "{:?}", forced.failure);
    assert_eq!(machine.sent(libc::SIGSTOP), the_windows());
    assert_eq!(machine.sent(libc::SIGKILL), the_windows());
    let named: Vec<i32> = forced
        .members
        .iter()
        .map(|member| i32::try_from(member.key.pid).expect("a number"))
        .collect();
    assert_eq!(named, the_windows(), "the strangers are no member of it");
}

#[test]
fn a_close_signals_only_what_is_under_the_program_though_the_session_names_more() {
    let machine = Machine::new(a_window_among_strangers());
    let table = machine.processes().expect("the table");
    let tree = tree_of(&table);
    // The job in the window's session whose parent has gone is the window's by the
    // session, which the selection says: no look saw it under the program, and it is
    // left running.
    let chosen = tree.select(&table, &HashMap::new());
    assert!(chosen.contains_key(&LOST), "the selection names it");

    let forced = tree.force_by(&machine, Subject::Whole, &[]);

    assert!(forced.failure.is_none(), "{:?}", forced.failure);
    assert_eq!(machine.sent(libc::SIGSTOP), the_windows());
    assert_eq!(machine.sent(libc::SIGKILL), the_windows());
}

#[test]
fn a_job_seen_under_the_program_is_ended_by_a_close_though_its_parent_has_gone_since() {
    let machine = Machine::new(a_window_among_strangers());
    let table = machine.processes().expect("the table");
    let tree = tree_of(&table);
    // A look while the shell runs.
    tree.members_by(&machine).expect("a look");

    // The shell is gone and its job runs on, in the window's session.
    let later = Machine::new(vec![
        table[0].clone(),
        Process {
            session: Some(PROGRAM),
            ..table[2].clone()
        },
        table[4].clone(),
        table[8].clone(),
    ]);
    let forced = tree.force_by(&later, Subject::Whole, &[]);

    assert!(forced.failure.is_none(), "{:?}", forced.failure);
    assert_eq!(later.sent(libc::SIGKILL), [PROGRAM, JOB]);
}

#[test]
fn what_the_bound_refuses_is_no_part_of_the_turns_lineage_whoever_proves_it_the_turns() {
    let shell = |pid, unique, parent| Process {
        name: "sh".to_owned(),
        session: Some(PROGRAM),
        ..listed(pid, unique, parent)
    };
    // Before the turn: the window's program, and a process of its session that nothing
    // saw under the program, and whose parent has gone.
    let before = Machine::new(vec![
        Process {
            session: Some(PROGRAM),
            ..listed(PROGRAM, 1000, 700)
        },
        Process {
            session: Some(PROGRAM),
            ..listed(LOST, 2300, 1)
        },
    ]);
    let table = before.processes().expect("the table");
    let tree = tree_of(&table);
    let mut baseline = Baseline::default();
    baseline.mark(&tree.members_by(&before).expect("a look"), None, None);

    // After it: a shell the program started, and one that process started: each is a
    // shell with a parent from before the mark, which proves it the turn's.
    let after = Machine::new(vec![
        table[0].clone(),
        table[1].clone(),
        shell(SHELL, 1100, 1000),
        shell(USERS_SHELL, 2100, 2300),
    ]);
    let lineage = tree.lineage_by(&after, &baseline).expect("the lineage");

    let pids: Vec<i32> = lineage.identities.iter().map(|id| id.pid).collect();
    assert_eq!(pids, [SHELL], "only what is under the program");
    assert_eq!(lineage.members.len(), 1);
}

#[test]
fn an_ending_names_as_ended_only_what_it_signalled() {
    let machine = Machine::new(a_window_among_strangers());
    let table = machine.processes().expect("the table");
    let tree = tree_of(&table);
    // A lineage that names the window's shell and job, and the test's own parent, which
    // is alive: whatever chose it.
    let lineage = Lineage {
        identities: vec![identity_of(&table, SHELL), identity_of(&table, JOB)],
        members: members_of(&table, &[SHELL, JOB, the_tests_own_parent()]),
    };
    let asked = tree
        .ask_by(&machine, Subject::Lineage(&lineage), None)
        .expect("asked");

    let ending = tree.finish_by(&machine, &lineage, asked);

    let ended: Vec<i32> = ending
        .ended
        .processes
        .iter()
        .map(|ended| i32::try_from(ended.pid).expect("a number"))
        .collect();
    assert_eq!(
        ended,
        [SHELL, JOB, DEEP],
        "what it signalled, and what that started"
    );
    assert!(
        !ended.contains(&the_tests_own_parent()),
        "the test's own parent was never signalled and is not named: {ending:?}"
    );
    assert_eq!(ending.left.total, 0, "{ending:?}");
    assert_eq!(machine.sent(libc::SIGKILL), [SHELL, JOB, DEEP]);
}

#[test]
fn a_tree_with_no_program_signals_nothing_whatever_is_selected() {
    let machine = Machine::new(a_window_among_strangers());
    let table = machine.processes().expect("the table");
    // A pane whose program could not be read: nothing is under it, and nothing was seen.
    let tree = ProcessTree::new();
    let wrong = every_process(&table);

    let asked = tree
        .ask_by(&machine, Subject::Lineage(&wrong), None)
        .expect("asked");
    let forced = tree.force_by(&machine, Subject::Lineage(&wrong), &[]);

    assert!(machine.signalled_none());
    assert!(asked.identities.is_empty() && forced.members.is_empty());
}

/// A Python that starts a Python that sleeps, says its number, and waits to be
/// told to go.
#[cfg(target_os = "macos")]
const PARENT: &str = "import subprocess, sys; \
    child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(41)']); \
    print(child.pid, flush=True); input()";

/// The incident, replayed on this machine's own table with the signals only recorded: a
/// selection that names every process there is, and a window of two real processes
/// among the hundreds that are not its own (the build this test runs in, the shell that
/// started it, the user's programs). Only the window's two would be signalled.
#[cfg(target_os = "macos")]
#[allow(clippy::disallowed_methods)] // A test's own programs.
#[test]
fn a_selection_of_every_process_of_this_machine_would_signal_only_the_windows_own() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let mut tree = ProcessTree::new();
    let mut program = Command::new("/usr/bin/python3")
        .args(["-c", PARENT])
        .env(OWNER_ENV, &tree.marker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start the window's program");
    let program_pid = i32::try_from(program.id()).expect("a process number");
    tree.attach(program_pid);
    let mut line = String::new();
    BufReader::new(program.stdout.take().expect("its output"))
        .read_line(&mut line)
        .expect("the child's number");
    let child: i32 = line.trim().parse().expect("a process number");

    let machine = Machine::new(native::processes().expect("this machine's processes"));
    let table = machine.processes().expect("the table");
    assert!(
        table.len() > 10,
        "the machine has strangers to be told from: {}",
        table.len()
    );
    let wrong = every_process(&table);
    let ours = {
        let mut ours = vec![program_pid, child];
        ours.sort_unstable();
        ours
    };

    tree.ask_by(&machine, Subject::Lineage(&wrong), None)
        .expect("asked");
    let forced = tree.force_by(&machine, Subject::Lineage(&wrong), &[]);
    tree.force_by(&machine, Subject::Whole, &[]);

    assert!(forced.failure.is_none(), "{:?}", forced.failure);
    for signal in [libc::SIGTERM, libc::SIGHUP, libc::SIGSTOP, libc::SIGKILL] {
        assert_eq!(machine.sent(signal), ours, "signal {signal}");
    }

    // The window is this test's own, and it ends it, nothing having been sent.
    program
        .stdin
        .take()
        .expect("its input")
        .write_all(b"\n")
        .expect("tell it to go");
    program.wait().expect("the program ends");
    // SAFETY: kill takes two integers and touches no memory of ours; the child is this
    // test's own, a Python that sleeps.
    unsafe { libc::kill(child, libc::SIGKILL) };
}

#[cfg(target_os = "macos")]
#[allow(clippy::disallowed_methods)] // A test's own programs.
#[test]
fn a_process_has_the_process_that_started_it_as_its_parent_until_that_has_gone() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let mut tree = ProcessTree::new();
    let mut parent = Command::new("/usr/bin/python3")
        .args(["-c", PARENT])
        .env(OWNER_ENV, &tree.marker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start the parent");
    tree.attach(i32::try_from(parent.id()).expect("a process number"));
    let mut line = String::new();
    BufReader::new(parent.stdout.take().expect("its output"))
        .read_line(&mut line)
        .expect("the child's number");
    let child: u32 = line.trim().parse().expect("a process number");

    let members = tree.members().expect("look at the tree");
    let root = members.iter().find(|m| m.key.pid == parent.id());
    let started = members.iter().find(|m| m.key.pid == child);
    assert_eq!(
        started.map(|member| member.parent),
        root.map(|member| Some(member.key)),
        "its parent is the process that started it: {members:?}"
    );

    // The parent goes. The child is still the pane's, found by the marker its
    // environment carries; the parent it names is no process of the tree, so
    // it names none, whether the system names it still or names launchd.
    parent
        .stdin
        .take()
        .expect("its input")
        .write_all(b"\n")
        .expect("tell it to go");
    parent.wait().expect("the parent ends");
    let members = tree.members().expect("look at the tree");
    let orphan = members.iter().find(|m| m.key.pid == child);
    // SAFETY: getpgid takes a process number and touches no memory of ours.
    let group = unsafe { libc::getpgid(i32::try_from(child).expect("a process number")) };
    assert_eq!(
        orphan.map(|member| (member.parent, member.group)),
        Some((None, Some(group))),
        "{members:?}"
    );
    // SAFETY: kill takes two integers and touches no memory of ours.
    unsafe {
        libc::kill(
            i32::try_from(child).expect("a process number"),
            libc::SIGKILL,
        )
    };
}

/// A session leader (as a pane's program is) that runs a shell, which detaches a job
/// (`nohup sleep 41 &`) and ends, says the job's number, and waits to be told to go.
#[cfg(target_os = "macos")]
const LEADER: &str = "import subprocess; \
    job = subprocess.run(['/bin/sh', '-c', 'nohup sleep 41 >/dev/null 2>&1 & echo $!'], \
        capture_output=True, text=True).stdout.strip(); \
    print(job, flush=True); input()";

#[cfg(target_os = "macos")]
#[allow(clippy::disallowed_methods)] // A test's own programs.
#[test]
fn a_job_whose_shell_has_gone_is_in_the_tree_by_the_session_it_kept_and_has_no_parent() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let mut tree = ProcessTree::new();
    let mut command = Command::new("/usr/bin/python3");
    command
        .args(["-c", LEADER])
        .env(OWNER_ENV, &tree.marker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    // SAFETY: setsid is async-signal-safe and touches no memory of ours; the pty makes
    // a pane's program a session leader as this does.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut leader = command.spawn().expect("start the session leader");
    tree.attach(i32::try_from(leader.id()).expect("a process number"));
    let mut line = String::new();
    BufReader::new(leader.stdout.take().expect("its output"))
        .read_line(&mut line)
        .expect("the job's number");
    let job: u32 = line.trim().parse().expect("a process number");

    // `sleep` is a system binary, which shows no environment: no marker finds it. Its
    // shell has gone, and the system names no parent of it that the tree has. The
    // session is what finds it, once it has become `sleep` (it is a copy of the
    // shell until `nohup` has run it).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let found = loop {
        let members = tree.members().expect("look at the tree");
        if let Some(member) = members
            .iter()
            .find(|member| member.key.pid == job && member.exe.ends_with("/sleep"))
        {
            break member.clone();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the job was never found as `sleep`: {members:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(found.parent, None);

    leader
        .stdin
        .take()
        .expect("its input")
        .write_all(b"\n")
        .expect("tell it to go");
    leader.wait().expect("the leader ends");
    // SAFETY: kill takes two integers and touches no memory of ours.
    unsafe { libc::kill(i32::try_from(job).expect("a process number"), libc::SIGKILL) };
}
