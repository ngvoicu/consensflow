use super::*;

/// A process of a snapshot, started by the process whose unique identifier is `parent`.
fn listed(pid: i32, unique: u64, parent: u64) -> Process {
    Process {
        identity: Identity {
            pid,
            born: (1, 0),
            unique,
        },
        parent,
        group: pid,
        session: None,
        name: format!("p{pid}"),
    }
}

#[test]
fn what_the_tracking_saw_start_is_in_the_tree_and_so_is_what_a_process_that_has_gone_started() {
    let mut tree = ProcessTree::new();
    tree.tracker = Some(Arc::new(Tracker::having_seen([
        // A shell that began and ended, and one that runs, as the tracking saw them.
        (listed(40, 400, 1).identity, Instant::now()),
        (listed(41, 410, 1).identity, Instant::now()),
    ])));
    let snapshot = [
        // The shell that runs.
        listed(41, 410, 1),
        // A job the shell that has gone started, and what the job started.
        listed(50, 500, 400),
        listed(51, 510, 500),
        // A process nobody of the pane's started.
        listed(60, 600, 1),
        // One a process that is not the tracking's started, and that is gone.
        listed(61, 610, 999),
    ];
    let mut found: Vec<i32> = tree
        .select(&snapshot, &HashMap::new())
        .keys()
        .copied()
        .collect();
    found.sort_unstable();
    assert_eq!(found, [41, 50, 51]);
}

#[test]
fn what_has_gone_and_is_the_parent_of_nothing_that_runs_is_let_go_of_when_the_tree_is_looked_at() {
    let long_ago = Instant::now()
        .checked_sub(Duration::from_secs(3600))
        .expect("a moment that long ago");
    let tracker = Arc::new(Tracker::having_seen([
        // Gone, and nobody's parent.
        (listed(80, 800, 1).identity, long_ago),
        // Gone, but what it started runs.
        (listed(81, 810, 1).identity, long_ago),
        // Running.
        (listed(82, 820, 1).identity, long_ago),
    ]));
    let mut tree = ProcessTree::new();
    tree.tracker = Some(Arc::clone(&tracker));
    let snapshot = [listed(82, 820, 1), listed(83, 830, 810)];
    tree.select(&snapshot, &HashMap::new());
    let mut kept: Vec<u64> = tracker.seen().keys().copied().collect();
    kept.sort_unstable();
    assert_eq!(kept, [810, 820]);
}

#[test]
fn a_process_the_tracking_saw_start_is_told_by_when_it_began_and_not_by_its_number() {
    let mut tree = ProcessTree::new();
    tree.tracker = Some(Arc::new(Tracker::having_seen([(
        listed(70, 700, 1).identity,
        Instant::now(),
    )])));
    // Another process has the number the one it saw had.
    let snapshot = [listed(70, 701, 1)];
    assert!(tree.select(&snapshot, &HashMap::new()).is_empty());
}

/// A Python that starts a Python that sleeps, says its number, and waits to be
/// told to go.
#[cfg(target_os = "macos")]
const PARENT: &str = "import subprocess, sys; \
    child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(41)']); \
    print(child.pid, flush=True); input()";

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
        root.map(|member| Parent::Of(member.key)),
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
        Some((Parent::Gone, Some(group))),
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
    // What the program started is told by the session alone here: the tracking is
    // what the next test is for.
    tree.tracker = None;
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
    assert_eq!(found.parent, Parent::Gone);

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

/// A session leader that says it is ready, and once told to go runs two hundred shells
/// one after the other, each in a session of its own, each of which detaches a job
/// (`nohup sleep 43 &`) and ends at once; it says the jobs' numbers and waits to be
/// told to go.
#[cfg(target_os = "macos")]
const MANY_APART: &str = "import subprocess; print('ready', flush=True); input(); \
    jobs = [subprocess.run(['/bin/sh', '-c', 'nohup sleep 43 >/dev/null 2>&1 & echo $!'], \
        capture_output=True, text=True, start_new_session=True).stdout.strip() for _ in range(200)]; \
    print(' '.join(jobs), flush=True); input()";

/// How many of the jobs that shells detached the tracking finds when the shells end
/// as soon as they have started them, and every core of the machine is busy: the
/// measure of how fast it follows, which depends on how loaded the machine is, and so
/// no test that passes or fails.
#[cfg(target_os = "macos")]
#[allow(clippy::disallowed_methods)] // A test's own programs.
#[test]
#[ignore = "measures how fast the tracking follows, with every core busy"]
fn how_many_of_the_jobs_detached_by_shells_that_end_at_once_the_tracking_finds() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let busy = Arc::new(AtomicBool::new(true));
    let spinning: Vec<_> = (0..std::thread::available_parallelism().map_or(8, usize::from))
        .map(|_| {
            let busy = Arc::clone(&busy);
            std::thread::spawn(move || {
                while busy.load(Ordering::Relaxed) {
                    std::hint::spin_loop();
                }
            })
        })
        .collect();
    let mut tree = ProcessTree::new();
    let mut command = Command::new("/usr/bin/python3");
    command
        .args(["-c", MANY_APART])
        .env(OWNER_ENV, &tree.marker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    // SAFETY: setsid is async-signal-safe and touches no memory of ours.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut leader = command.spawn().expect("start the session leader");
    tree.attach(i32::try_from(leader.id()).expect("a process number"));
    let mut output = BufReader::new(leader.stdout.take().expect("its output"));
    let mut input = leader.stdin.take().expect("its input");
    let mut line = String::new();
    output.read_line(&mut line).expect("it is ready");
    input.write_all(b"\n").expect("tell it to go");
    line.clear();
    output.read_line(&mut line).expect("the jobs' numbers");
    let jobs: Vec<u32> = line
        .split_whitespace()
        .map(|number| number.parse().expect("a process number"))
        .collect();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let members = tree.members().expect("look at the tree");
    busy.store(false, Ordering::Relaxed);
    for thread in spinning {
        thread.join().expect("a busy thread ends");
    }
    let found = jobs
        .iter()
        .filter(|job| members.iter().any(|member| member.key.pid == **job))
        .count();
    eprintln!("the tracking found {found} of {} jobs", jobs.len());
    input.write_all(b"\n").expect("tell it to end");
    leader.wait().expect("the leader ends");
    for job in jobs {
        // SAFETY: kill takes two integers and touches no memory of ours.
        unsafe { libc::kill(i32::try_from(job).expect("a process number"), libc::SIGKILL) };
    }
}

/// A session leader that says it is ready, and once told to go runs a shell in a
/// session of its own (as Pi's Bash tool starts each command), which detaches a job
/// (`nohup sleep 43 &`) and ends a moment after; it says the job's number and waits to
/// be told to go.
#[cfg(target_os = "macos")]
const APART: &str = "import subprocess; print('ready', flush=True); input(); \
    job = subprocess.run(['/bin/sh', '-c', 'nohup sleep 43 >/dev/null 2>&1 & echo $!; sleep 0.2'], \
        capture_output=True, text=True, start_new_session=True).stdout.strip(); \
    print(job, flush=True); input()";

#[cfg(target_os = "macos")]
#[allow(clippy::disallowed_methods)] // A test's own programs.
#[test]
fn a_job_that_left_the_session_with_its_shell_gone_is_in_the_tree_by_what_the_program_started() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let mut tree = ProcessTree::new();
    let mut command = Command::new("/usr/bin/python3");
    command
        .args(["-c", APART])
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
    let root = i32::try_from(leader.id()).expect("a process number");
    tree.attach(root);
    let mut output = BufReader::new(leader.stdout.take().expect("its output"));
    let mut input = leader.stdin.take().expect("its input");
    let mut line = String::new();
    output.read_line(&mut line).expect("it is ready");
    // The tracking is in place: what the program starts now is told as it starts.
    input.write_all(b"\n").expect("tell it to go");
    line.clear();
    output.read_line(&mut line).expect("the job's number");
    let job: u32 = line.trim().parse().expect("a process number");

    // `sleep` is a system binary, which shows no environment: no marker finds it. It
    // is in the session of a shell that has gone and not in the program's, and the
    // system names no parent of it that the tree has. What the program started is
    // what finds it, once it has become `sleep`.
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
    assert_eq!(found.parent, Parent::Gone);
    // SAFETY: getsid takes a process number and touches no memory of ours.
    let (program, apart) = unsafe { (libc::getsid(root), libc::getsid(job as i32)) };
    assert_ne!(apart, program, "the job is in a session of its own");

    input.write_all(b"\n").expect("tell it to end");
    leader.wait().expect("the leader ends");
    // SAFETY: kill takes two integers and touches no memory of ours.
    unsafe { libc::kill(i32::try_from(job).expect("a process number"), libc::SIGKILL) };
}
