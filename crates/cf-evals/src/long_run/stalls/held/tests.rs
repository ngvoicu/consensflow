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

/// The first thread's block of a `sample`'s call graph (the main thread's): its header,
/// how many samples it holds, and each frame of it, its depth, count and function.
#[cfg(target_os = "macos")]
fn main_thread(said: &str) -> (String, u64, Vec<(usize, u64, String)>) {
    let mut lines = said
        .lines()
        .skip_while(|line| !line.starts_with("Call graph:"))
        .skip(1);
    let header = lines.next().unwrap_or_default().trim().to_owned();
    let total = header
        .split(' ')
        .next()
        .and_then(|count| count.parse().ok())
        .unwrap_or_default();
    let frames = lines
        .take_while(|line| line.starts_with("    +") || line.starts_with("    !"))
        .filter_map(|line| {
            let rest = line.trim_start_matches([' ', '+', '!', ':', '|']);
            let depth = line.len() - rest.len();
            let (count, function) = rest.split_once(' ')?;
            let function = function.split("  (in").next().unwrap_or(function).trim();
            Some((depth, count.parse().ok()?, function.to_owned()))
        })
        .collect();
    (header, total, frames)
}

/// The function of the deepest frame of the main thread at (nearly) every sample, and the
/// one above it: where the program waited the whole second. Nothing where the sample's first
/// thread is not the main thread, or there is no sample.
#[cfg(target_os = "macos")]
fn waits_in(said: &str) -> (String, String) {
    let (header, total, frames) = main_thread(said);
    if !header.contains("main-thread") {
        return (String::new(), String::new());
    }
    let full: Vec<&(usize, u64, String)> = frames
        .iter()
        .filter(|(_, count, _)| count * 10 >= total * 9)
        .collect();
    let leaf = full.iter().max_by_key(|(depth, ..)| *depth);
    let parent = leaf.and_then(|(depth, ..)| {
        full.iter()
            .filter(|(above, ..)| above < depth)
            .max_by_key(|(above, ..)| *above)
    });
    let name = |frame: Option<&&(usize, u64, String)>| {
        frame
            .map(|(.., function)| function.clone())
            .unwrap_or_default()
    };
    (name(leaf), name(parent))
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
fn window(program: &str, args: &[&str]) -> Window {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    let pair = native_pty_system()
        .openpty(PtySize::default())
        .expect("a terminal");
    let mut command = CommandBuilder::new(program);
    command.args(args);
    let child = pair.slave.spawn_command(command).expect("the program");
    drop(pair.slave);
    Window {
        _master: pair.master,
        child,
    }
}

/// Whether `taken` shows a program that waits for its job and the job stopped under it: two
/// lines, the program's in a sleep and the job's `T`. A job that is listed has not read the
/// terminal yet when it runs (`R`) or sleeps before its read (`S`, `U`), and is stopped by the
/// terminal only once it has: a look taken before then is of a job that is no stopped one
/// (T-643: the program's `Rs+` and the job's `R`, `S`, `U` and `RN` in 4 of 192 runs at a load
/// of 130 to 190, each taken by a wait that went on once two processes were listed).
#[cfg(unix)]
fn stopped_under_its_program(taken: &Taken) -> bool {
    matches!(
        taken.processes.as_deref(),
        Ok([program, job]) if state_in(program).starts_with('S') && state_in(job).starts_with('T')
    )
}

#[cfg(unix)]
fn taken_with(states: [&str; 2]) -> Taken {
    Taken {
        processes: Ok(vec![
            format!(
                "5513 5389 {} - ttys048 /bin/sh -c set -m; cat & wait",
                states[0]
            ),
            format!("5535 5513 {} - ttys048 cat", states[1]),
        ]),
        unread: Ok(0),
        sample: None,
    }
}

#[cfg(unix)]
#[test]
fn a_job_is_stopped_under_its_program_only_when_the_states_say_so_not_when_it_is_listed() {
    // Listed, and not stopped: the job has not read the terminal yet.
    for job in ["R", "RN", "S", "U", "Ss", "R+"] {
        assert!(
            !stopped_under_its_program(&taken_with(["SNs+", job])),
            "a job in {job} is not stopped"
        );
    }
    // The program not yet waiting for it.
    for program in ["Rs+", "R", "U"] {
        assert!(
            !stopped_under_its_program(&taken_with([program, "T"])),
            "a program in {program} is not yet waiting"
        );
    }
    for (program, job) in [("SNs+", "T"), ("Ss+", "TN"), ("S+", "T+")] {
        assert!(
            stopped_under_its_program(&taken_with([program, job])),
            "{program} over {job}"
        );
    }
    // One line, three, or none asked.
    let mut one = taken_with(["SNs+", "T"]);
    one.processes = one.processes.map(|lines| lines[..1].to_vec());
    assert!(!stopped_under_its_program(&one));
    assert!(!stopped_under_its_program(&Taken::none("no ps")));
}

#[cfg(unix)]
#[test]
#[allow(clippy::too_many_lines)]
fn a_program_waiting_in_a_write_nobody_reads_and_one_stopped_are_told_apart() {
    let folder = tempfile::tempdir().unwrap();
    // A Node program writes to its terminal for ever, as Pi does, and waits in its write
    // once the terminal's queue is full; another sits idle in its event loop. A third's job
    // reads the terminal from the background, and the terminal stops it, as it stops any.
    let mut writer = window(
        "node",
        &[
            "-e",
            "const s = 'x'.repeat(4096); for (;;) process.stdout.write(s)",
        ],
    );
    let mut idle = window("node", &["-e", "setInterval(() => {}, 1000)"]);
    let mut stopper = window("/bin/sh", &["-c", "set -m; cat & wait"]);
    // The pane host's stand-in, sampled as the evidence samples the pane host.
    let mut host = cf_e2e::process::Run::new("sleep")
        .arg("120")
        .inheriting_env()
        .spawn()
        .unwrap();
    let (writing, resting, stopping) = (
        writer.child.process_id().unwrap(),
        idle.child.process_id().unwrap(),
        stopper.child.process_id().unwrap(),
    );
    let mut system = system(folder.path());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let (held, rested, taken) = loop {
        let listed = system.list();
        let held = system.take(&listed, writing, &[], "writer-1-found");
        let rested = system.take(&listed, resting, &[], "idle-1-found");
        let taken = system.take(&listed, stopping, &[], "stopped-1-found");
        // Waited for on all that is asserted after: the writer asleep with bytes unread, the
        // job stopped under its program, and each Node's main thread where the assertions
        // want it.
        let waits = state_in(&line_of(&held, writing)).starts_with('S')
            && (!cfg!(target_os = "macos") || held.unread.as_ref().is_ok_and(|bytes| *bytes > 0));
        let stopped = stopped_under_its_program(&taken);
        if (waits && stopped && settled(&held, &rested)) || std::time::Instant::now() > deadline {
            break (held, rested, taken);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    };
    let _ = writer.child.kill();
    let _ = idle.child.kill();
    let _ = stopper.child.kill();
    // The writer asleep in its write, the terminal's queue full and unread (macOS says how
    // much, and where its program and the pane host wait); the other's job stopped, under
    // its program.
    assert!(
        state_in(&line_of(&held, writing)).starts_with('S'),
        "{held:?}"
    );
    let lines = taken.processes.as_ref().unwrap();
    assert_eq!(lines.len(), 2, "{taken:?}");
    assert!(state_in(&lines[0]).starts_with('S'), "{taken:?}");
    assert!(state_in(&lines[1]).starts_with('T'), "{taken:?}");
    assert!(lines[1].ends_with(" cat"), "{taken:?}");
    #[cfg(target_os = "macos")]
    {
        assert!(*held.unread.as_ref().unwrap() > 0, "{held:?}");
        let sample = held.sample.clone().unwrap().unwrap();
        assert_eq!(sample, folder.path().join("writer-1-found.sample.txt"));
        // Its main thread in `write` the whole second, under libuv's write; an idle Node's,
        // in `kevent` under `uv__io_poll`, and in no write.
        let (leaf, above) = waits_in(&sampled(&held));
        assert_eq!((leaf.as_str(), above.as_str()), ("write", "uv__try_write"));
        let (leaf, above) = waits_in(&sampled(&rested));
        assert_eq!((leaf.as_str(), above.as_str()), ("kevent", "uv__io_poll"));
        assert_eq!(rested.unread, Ok(0));
    }
    #[cfg(not(target_os = "macos"))]
    {
        assert_eq!(held.unread, Err(LINUX_UNREAD.to_owned()));
        assert_eq!(held.sample, None);
        let _ = rested;
    }
    // The pane host's stand-in sampled as the pane host is: on macOS, beside the program's.
    let sampled_host = system.sample_host(host.id(), "writer-1-found-host");
    host.kill();
    if cfg!(target_os = "macos") {
        let file = sampled_host.unwrap().unwrap();
        assert_eq!(file, folder.path().join("writer-1-found-host.sample.txt"));
        assert!(std::fs::metadata(&file).unwrap().len() > 0);
    } else {
        assert_eq!(sampled_host, None);
    }
}

/// Whether each Node's main thread is where the assertions want it: the writer in `write`
/// under `uv__try_write`, the idle one in `kevent` under `uv__io_poll`, the whole second.
#[cfg(target_os = "macos")]
fn settled(held: &Taken, rested: &Taken) -> bool {
    let wants = |taken: &Taken, leaf: &str, above: &str| {
        let (found, parent) = waits_in(&sampled(taken));
        found == leaf && parent == above
    };
    wants(held, "write", "uv__try_write") && wants(rested, "kevent", "uv__io_poll")
}

/// No sample to read off this system.
#[cfg(all(unix, not(target_os = "macos")))]
fn settled(_: &Taken, _: &Taken) -> bool {
    true
}

/// What the sample `taken` kept says; nothing where none was kept.
#[cfg(unix)]
fn sampled(taken: &Taken) -> String {
    taken
        .sample
        .clone()
        .and_then(Result::ok)
        .and_then(|file| std::fs::read_to_string(file).ok())
        .unwrap_or_default()
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
    let mut system = system(folder.path());
    let listed = system.list();
    let taken = system.take(&listed, pid, &[], "ended-1-found");
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
        let listed = system.list();
        let taken = system.take(&listed, writer.id(), &[], "writer-1-found");
        if line_of(&taken, writer.id()).contains("Waiting/Executive(0)")
            || std::time::Instant::now() > deadline
        {
            break taken;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    };
    let listed = system.list();
    let taken = system.take(&listed, stopped.id(), &[], "stopped-1-found");
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
