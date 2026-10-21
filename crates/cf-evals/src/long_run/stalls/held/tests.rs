//! What the system says of a stalled window's processes: read from what `ps` and the
//! PowerShell say, and asked of real processes, one waiting in a write to a terminal (or on
//! Windows a pipe) nobody reads, and one stopped otherwise.

use super::*;

#[test]
fn what_ps_says_is_each_process_with_its_parent_its_state_and_its_terminal() {
    let said = "  501     1 Ss   -      ttys014  /bin/zsh -l\n\
                  777   501 S+   -      ttys014  node /usr/local/bin/pi   --model x\n\
                  778   777 T    pipe_w ??       bash -c npm  test\n\
                  oops\n";
    assert_eq!(
        parse_ps(said),
        [
            Listed {
                pid: 501,
                ppid: 1,
                tty: Some("ttys014".to_owned()),
                line: "501 1 Ss - ttys014 /bin/zsh -l".to_owned(),
            },
            Listed {
                pid: 777,
                ppid: 501,
                tty: Some("ttys014".to_owned()),
                line: "777 501 S+ - ttys014 node /usr/local/bin/pi --model x".to_owned(),
            },
            Listed {
                pid: 778,
                ppid: 777,
                tty: None,
                line: "778 777 T pipe_w ?? bash -c npm test".to_owned(),
            },
        ]
    );
}

#[test]
fn a_windows_thread_is_said_by_its_state_and_what_it_waits_for_by_name_and_number() {
    assert_eq!(thread_state(5, 0), "Waiting/Executive(0)");
    assert_eq!(thread_state(5, 5), "Waiting/Suspended(5)");
    assert_eq!(thread_state(5, 6), "Waiting/UserRequest(6)");
    assert_eq!(thread_state(5, 15), "Waiting/WrQueue(15)");
    assert_eq!(thread_state(5, 99), "Waiting/#99(99)");
    assert_eq!(thread_state(2, 0), "Running");
    assert_eq!(thread_state(9, 0), "#9");
    let said = "P 4 0 System\n\
                P 900 600 node   C:\\pi\\cli.js  --model x\n\
                T 900 5 6\nT 900 5 0\nT 900 5 6\nT 900 2 0\nT 4 5 15\nT x\n";
    assert_eq!(
        parse_windows(said),
        [
            Listed {
                pid: 4,
                ppid: 0,
                tty: None,
                line: "4 0 threads: Waiting/WrQueue(15)×1 · System".to_owned(),
            },
            Listed {
                pid: 900,
                ppid: 600,
                tty: None,
                line: "900 600 threads: Waiting/UserRequest(6)×2, Running×1, Waiting/Executive(0)×1 · node C:\\pi\\cli.js --model x".to_owned(),
            },
        ]
    );
}

fn listed(pid: u32, ppid: u32) -> Listed {
    Listed {
        pid,
        ppid,
        tty: (pid == 10).then(|| "ttys001".to_owned()),
        line: format!("{pid} {ppid}"),
    }
}

#[test]
fn a_windows_tree_is_its_program_what_the_host_named_and_every_process_under_them() {
    // 10 the program, 11 and 12 its children, 13 its grandchild; 20 a job whose parent
    // ended, named by the pane host, and 21 its child; 30 no one's of the window.
    let all = [
        listed(30, 1),
        listed(13, 11),
        listed(10, 5),
        listed(11, 10),
        listed(12, 10),
        listed(20, 1),
        listed(21, 20),
    ];
    let pids = |found: Vec<&Listed>| found.iter().map(|one| one.pid).collect::<Vec<_>>();
    assert_eq!(pids(tree(&all, 10, &[])), [10, 11, 12, 13]);
    assert_eq!(pids(tree(&all, 10, &[20, 11])), [10, 20, 11, 12, 21, 13]);
    assert_eq!(pids(tree(&all, 99, &[])), Vec::<u32>::new());
    assert_eq!(terminal_of(&all, 10), Ok("ttys001"));
    assert_eq!(
        terminal_of(&all, 11),
        Err("its program has no terminal".to_owned())
    );
    assert_eq!(
        terminal_of(&all, 99),
        Err("the system lists no process 99".to_owned())
    );
}

/// The system's prober, its samples kept in `folder`.
fn system(folder: &std::path::Path) -> System {
    let folder = folder.to_owned();
    System::new(move |label| Ok(folder.join(format!("{label}.sample.txt"))))
}

/// The line `taken` says of the process `pid`.
fn line_of(taken: &Taken, pid: u32) -> String {
    taken
        .processes
        .as_ref()
        .unwrap()
        .iter()
        .find(|line| line.split(' ').next() == Some(pid.to_string().as_str()))
        .unwrap_or_else(|| panic!("no line of {pid}: {taken:?}"))
        .clone()
}

/// The state `ps` says in the line `line` (`pid ppid stat …`).
#[cfg(unix)]
fn state_in(line: &str) -> String {
    line.split(' ').nth(2).unwrap_or_default().to_owned()
}

/// A window's program on a terminal opened as the pane host opens one, the terminal's
/// reading end kept open as the host keeps it, and never read.
#[cfg(unix)]
struct Window {
    _master: Box<dyn portable_pty::MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

#[cfg(unix)]
fn window(script: &str) -> Window {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    let pair = native_pty_system()
        .openpty(PtySize::default())
        .expect("a terminal");
    let mut command = CommandBuilder::new("/bin/sh");
    command.args(["-c", script]);
    let child = pair.slave.spawn_command(command).expect("the program");
    drop(pair.slave);
    Window {
        _master: pair.master,
        child,
    }
}

#[cfg(unix)]
#[test]
fn a_program_waiting_in_a_write_nobody_reads_and_one_stopped_are_told_apart() {
    let folder = tempfile::tempdir().unwrap();
    // One writes to its terminal for ever, and waits in its write once the terminal's
    // queue is full. The other's job reads the terminal from the background, and the
    // terminal stops it, as it stops any.
    let mut writer =
        window("while :; do printf '%s\\n' xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx; done");
    let mut stopper = window("set -m; cat & wait");
    let (writing, stopping) = (
        writer.child.process_id().unwrap(),
        stopper.child.process_id().unwrap(),
    );
    let mut system = system(folder.path());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let (held, taken) = loop {
        let held = system.take(writing, &[], "writer-found");
        let taken = system.take(stopping, &[], "stopped-found");
        let waits = state_in(&line_of(&held, writing)).starts_with('S')
            && (!cfg!(target_os = "macos") || held.unread.as_ref().is_ok_and(|bytes| *bytes > 0));
        let stopped = taken.processes.as_ref().unwrap().len() == 2;
        if (waits && stopped) || std::time::Instant::now() > deadline {
            break (held, taken);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    };
    let _ = writer.child.kill();
    let _ = stopper.child.kill();
    // The writer asleep in its write, the terminal's queue full and unread (macOS says how
    // much, and where its program waits); the other's job stopped, under its program.
    assert!(
        state_in(&line_of(&held, writing)).starts_with('S'),
        "{held:?}"
    );
    let lines = taken.processes.as_ref().unwrap();
    assert_eq!(lines.len(), 2, "{taken:?}");
    assert!(state_in(&lines[0]).starts_with('S'), "{taken:?}");
    assert!(state_in(&lines[1]).starts_with('T'), "{taken:?}");
    assert!(lines[1].ends_with(" cat"), "{taken:?}");
    if cfg!(target_os = "macos") {
        assert!(*held.unread.as_ref().unwrap() > 0, "{held:?}");
        let sample = held.sample.clone().unwrap().unwrap();
        assert_eq!(sample, folder.path().join("writer-found.sample.txt"));
        let said = std::fs::read_to_string(&sample).unwrap();
        assert!(said.contains("write"), "{said}");
    } else {
        assert_eq!(held.unread, Err(LINUX_UNREAD.to_owned()));
        assert_eq!(held.sample, None);
    }
}

#[cfg(unix)]
#[test]
fn the_terminal_asked_is_closed_again_and_its_reader_sees_its_program_end() {
    // A program on its terminal, asked while it runs, then ends: the terminal's reader
    // (the pane host) must see the end. Held open by the asking, the terminal would keep
    // the reader from seeing a program end that leaves it open, so the asking holds none
    // of it once asked: no descriptor of this process is the terminal's.
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    let folder = tempfile::tempdir().unwrap();
    let pair = native_pty_system()
        .openpty(PtySize::default())
        .expect("a terminal");
    let device = std::fs::metadata(pair.master.tty_name().expect("the terminal's name"))
        .unwrap()
        .rdev();
    let holding = || {
        std::fs::read_dir("/dev/fd")
            .unwrap()
            .filter_map(|open| std::fs::metadata(open.ok()?.path()).ok())
            .filter(|open| open.rdev() == device)
            .count()
    };
    let mut command = CommandBuilder::new("/bin/sh");
    command.args(["-c", "sleep 3"]);
    let mut child = pair.slave.spawn_command(command).expect("the program");
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    assert_eq!(holding(), 0, "the test holds its program's terminal");
    let pid = child.process_id().unwrap();
    let taken = system(folder.path()).take(pid, &[], "ended-found");
    if cfg!(target_os = "macos") {
        assert_eq!(taken.unread, Ok(0), "{taken:?}");
    }
    assert_eq!(holding(), 0, "the asking left the terminal open");
    child.wait().unwrap();
    let (said, heard) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut rest = Vec::new();
        let _ = said.send(
            reader
                .read_to_end(&mut rest)
                .map(|_| ())
                .map_err(|failed| failed.kind()),
        );
    });
    let ended = heard.recv_timeout(std::time::Duration::from_secs(20));
    assert!(
        ended.is_ok(),
        "the reader never saw the program end: the terminal was held open"
    );
}

#[cfg(windows)]
#[test]
#[allow(clippy::disallowed_methods)] // The test starts what it asks the system of.
fn a_program_waiting_in_a_write_nobody_reads_and_one_stopped_are_told_apart() {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    /// `CREATE_SUSPENDED`: the process is made with its thread stopped.
    const SUSPENDED: u32 = 0x4;
    let folder = tempfile::tempdir().unwrap();
    let mut writer = Command::new("cmd")
        .args([
            "/d",
            "/c",
            "for /l %i in (0,0,1) do @echo xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stopped = Command::new("cmd")
        .args(["/d", "/c", "exit"])
        .creation_flags(SUSPENDED)
        .spawn()
        .unwrap();
    let mut system = system(folder.path());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let held = loop {
        let taken = system.take(writer.id(), &[], "writer-found");
        if line_of(&taken, writer.id()).contains("Waiting/Executive(0)")
            || std::time::Instant::now() > deadline
        {
            break taken;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    };
    let taken = system.take(stopped.id(), &[], "stopped-found");
    let _ = writer.kill();
    let _ = stopped.kill();
    let _ = writer.wait();
    let _ = stopped.wait();
    // A thread of the writer waits in its write; the stopped one's thread waits suspended.
    let writes = line_of(&held, writer.id());
    assert!(writes.contains("Waiting/Executive(0)"), "{writes}");
    assert!(!writes.contains("Suspended"), "{writes}");
    let pauses = line_of(&taken, stopped.id());
    assert!(pauses.contains("Waiting/Suspended(5)"), "{pauses}");
    assert_eq!(held.unread, Err(WINDOWS_UNREAD.to_owned()));
    assert_eq!(held.sample, None);
}
