//! What the tree is of the processes there are: the session of a pane, told on a table
//! written out, and the tree's look at real processes the tests start. What the tree signals, on a machine made up, is in `machine_tests`.

use super::machine_tests::every_process;
use super::testing::{listed, Machine};
use super::*;

/// The numbers of what `found` names, in order.
fn pids(found: &HashMap<i32, Identity>) -> Vec<i32> {
    let mut pids: Vec<i32> = found.keys().copied().collect();
    pids.sort_unstable();
    pids
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

    tree.ask_by(&machine, None).expect("asked");
    let failure = tree.force_by(&machine, &wrong);
    tree.force_by(&machine, &[]);

    assert!(failure.is_none(), "{failure:?}");
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
