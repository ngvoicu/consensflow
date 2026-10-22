use super::*;
use crate::updater_smoke::testing::{alone, is_alone, Ran};

/// `lsof -F ftdDin` of a running ConsensFlow 3.0.5 on this Mac, cut down: its streams, a unix
/// socket, a file a framework opened, a kqueue and a socket of the system's.
const SHOWN: &str = "p2232\nf0\ntCHR\nD0x3000002\ni336\nn/dev/null\nf2\ntREG\nD0x100000e\ni459196098\nn/Users/x/.consensflow/app/app.log\nf3\ntunix\nd0x4728cfc0aca56d7a\nn->0x1e1b0426067da685\nf6\ntREG\nD0x100000e\ni1152921500312139027\nn/System/Library/Extensions/AppleMetalOpenGLRenderer.bundle/Contents/Resources/default.metallib\nf11\ntKQUEUE\nncount=0, state=0xa\nf17\ntsystm\nd0xdcbcb4e125cc7e2f\nn[ctl com.apple.netsrc id 7 unit 58]\nfcwd\ntDIR\nD0x100000e\ni2\nn/\n";

fn held(fd: u32, kind: &str, object: Option<&str>, file: Option<(&str, u64)>, name: &str) -> Held {
    Held {
        fd,
        kind: kind.to_owned(),
        object: object.map(str::to_owned),
        file: file.map(|(device, node)| (device.to_owned(), node)),
        name: name.to_owned(),
    }
}

#[test]
fn what_lsof_says_is_read_as_each_descriptor_above_the_streams_by_its_object_or_its_file() {
    assert_eq!(
        parse_held(SHOWN),
        [
            held(3, "unix", Some("0x4728cfc0aca56d7a"), None, "->0x1e1b0426067da685"),
            held(
                6,
                "REG",
                None,
                Some(("0x100000e", 1_152_921_500_312_139_027)),
                "/System/Library/Extensions/AppleMetalOpenGLRenderer.bundle/Contents/Resources/default.metallib"
            ),
            held(11, "KQUEUE", None, None, "count=0, state=0xa"),
            held(17, "systm", Some("0xdcbcb4e125cc7e2f"), None, "[ctl com.apple.netsrc id 7 unit 58]"),
        ]
    );
}

/// The installed app as the case leaves it: a socket, a file a framework opened and a socket of
/// the system's, at numbers well above any this process has open, and what it was left, at the
/// numbers this process had them at.
fn old_with(left: &Left) -> Vec<Held> {
    vec![
        held(
            900,
            "unix",
            Some("0x4728cfc0aca56d7a"),
            None,
            "->0x1e1b0426067da685",
        ),
        held(
            901,
            "REG",
            None,
            Some(("0x100000e", 77)),
            "default.metallib",
        ),
        held(
            902,
            "systm",
            Some("0xdcbcb4e125cc7e2f"),
            None,
            "[ctl com.apple.netsrc]",
        ),
        held(left.pipe, "PIPE", Some("0xaaaa"), None, "->0xbbbb"),
        held(
            left.file_fd,
            "REG",
            None,
            Some(("0x100000e", left.node)),
            "/x/left-to-the-app",
        ),
    ]
}

#[test]
fn a_new_app_that_holds_none_of_the_old_ones_objects_has_no_fault_and_a_file_at_its_number_is_noted(
) {
    let folder = tempfile::tempdir().unwrap();
    let left = Left::make(folder.path()).unwrap();
    let old = old_with(&left);
    // The new app opened its own socket, and the framework's file again at the same number.
    let new = vec![
        held(900, "unix", Some("0x1111"), None, "->0x2222"),
        held(
            901,
            "REG",
            None,
            Some(("0x100000e", 77)),
            "default.metallib",
        ),
    ];
    let (faults, notes) = compare(&old, &new, &left);
    assert_eq!(faults, Vec::<String>::new());
    assert_eq!(notes.len(), 1, "{notes:?}");
}

#[test]
fn a_new_app_left_the_old_ones_descriptors_is_at_fault_for_each() {
    let folder = tempfile::tempdir().unwrap();
    let left = Left::make(folder.path()).unwrap();
    let old = old_with(&left);
    // Tauri's bare start, with nothing made close-on-exec: the new app holds them at their numbers.
    let new = old.clone();
    let (faults, notes) = compare(&old, &new, &left);
    // The file it was left is the fault; the pipe is the pipe's own check's
    // ([`Left::closed_everywhere`]).
    assert_eq!(
        faults,
        [format!(
            "the new app holds the file the installed app was left (fd {} REG /x/left-to-the-app)",
            left.file_fd
        )]
    );
    // The sockets are noted, both numbers said: lsof's address is no identity.
    let noted = notes.join("\n");
    assert!(noted.contains("0xdcbcb4e125cc7e2f"), "{noted}");
    assert!(
        noted.contains("the new app's fd 900 may be the old app's fd 900"),
        "{noted}"
    );
    assert!(!noted.contains("0xaaaa"), "{noted}");
}

/// The system gave the new app's socket an address one of the old app's had (it reuses the
/// address of an object that is gone): with another peer it is the new app's own (seen in the
/// smoke's plant run, T-271).
#[test]
fn a_socket_at_an_address_the_old_app_had_with_another_peer_is_the_new_apps_own() {
    let folder = tempfile::tempdir().unwrap();
    let left = Left::make(folder.path()).unwrap();
    let old = old_with(&left);
    let new = vec![
        held(
            18,
            "unix",
            Some("0x4728cfc0aca56d7a"),
            None,
            "->0xa65099d04f5df15f",
        ),
        held(
            20,
            "unix",
            Some("0x4728cfc0aca56d7a"),
            None,
            "->0xa65099d04f5df15f",
        ),
    ];
    let (faults, notes) = compare(&old, &new, &left);
    assert_eq!(faults, Vec::<String>::new());
    assert_eq!(notes, Vec::<String>::new());
}

/// The same address and the same peer may be the old app's socket, at whatever number the new
/// app holds it, or the new app's own to the same service at an address given again: a note,
/// which says both numbers.
#[test]
fn a_socket_at_the_old_apps_address_with_its_peer_is_noted_with_both_numbers() {
    let folder = tempfile::tempdir().unwrap();
    let left = Left::make(folder.path()).unwrap();
    let old = old_with(&left);
    let new = vec![held(
        18,
        "unix",
        Some("0x4728cfc0aca56d7a"),
        None,
        "->0x1e1b0426067da685",
    )];
    let (faults, notes) = compare(&old, &new, &left);
    assert_eq!(faults, Vec::<String>::new());
    assert_eq!(
        notes,
        ["the new app's fd 18 may be the old app's fd 900, given again or left: unix 0x4728cfc0aca56d7a ->0x1e1b0426067da685"]
    );
}

/// A socket left whose peer has since closed shows no peer: at the old one's address and number,
/// it is noted all the same (T-277).
#[test]
fn a_socket_at_the_old_apps_address_and_number_whose_peer_closed_is_noted() {
    let folder = tempfile::tempdir().unwrap();
    let left = Left::make(folder.path()).unwrap();
    let old = old_with(&left);
    let new = vec![held(
        900,
        "unix",
        Some("0x4728cfc0aca56d7a"),
        None,
        "->(none)",
    )];
    let (faults, notes) = compare(&old, &new, &left);
    assert_eq!(faults, Vec::<String>::new());
    assert_eq!(
        notes,
        ["the new app's fd 900 may be the old app's fd 900, given again or left: unix 0x4728cfc0aca56d7a ->(none)"]
    );
}

/// A pipe of the old app's in the new one is the left pipe's check's: it does not reach its end.
#[test]
fn a_pipe_is_left_to_the_pipe_the_case_left() {
    let folder = tempfile::tempdir().unwrap();
    let left = Left::make(folder.path()).unwrap();
    let old = old_with(&left);
    let new = vec![held(left.pipe, "PIPE", Some("0xaaaa"), None, "->0xbbbb")];
    let (faults, _) = compare(&old, &new, &left);
    assert_eq!(faults, Vec::<String>::new());
}

#[test]
fn an_installed_app_that_was_not_left_the_two_makes_the_check_prove_nothing() {
    let folder = tempfile::tempdir().unwrap();
    let left = Left::make(folder.path()).unwrap();
    let (faults, _) = compare(&parse_held(SHOWN), &[], &left);
    assert_eq!(faults.len(), 2, "{faults:?}");
    assert!(faults
        .iter()
        .all(|fault| fault.ends_with("the check proves nothing")));
}

/// Only this process holds the writing end, and then none. Run in a process of its own: the
/// tests beside it start programs bare, and one started as the pipe is made (std makes it
/// close-on-exec in a second step) holds the writing end for as long as it runs
/// (`the_pipe_left_under_starts_as_it_is_left_now` shows it: a `sleep` held it).
#[test]
fn the_pipe_left_reaches_its_end_once_no_process_holds_it_and_not_before() {
    alone("updater_smoke::descriptors::tests::the_pipe_left_alone");
}

#[test]
#[ignore = "run alone, by the test above it"]
fn the_pipe_left_alone() {
    if !is_alone() {
        return;
    }
    let _ran = Ran;
    let folder = tempfile::tempdir().unwrap();
    let mut left = Left::make(folder.path()).unwrap();
    let mut held = Left::make(folder.path()).unwrap();
    // A wait that must not end is held to a moment; the one that must, given seconds, ends at
    // once when it is right.
    assert!(held
        .closed_everywhere(&Waits::new(std::time::Duration::from_millis(300)))
        .is_err());
    left.given();
    left.closed_everywhere(&Waits::new(std::time::Duration::from_secs(10)))
        .unwrap();
}

#[test]
fn what_is_left_is_close_on_exec_until_it_is_left_to_the_app() {
    let folder = tempfile::tempdir().unwrap();
    let left = Left::make(folder.path()).unwrap();
    let flags = |fd: u32| fcntl(i32::try_from(fd).unwrap(), FcntlArg::F_GETFD).unwrap();
    assert_ne!(flags(left.pipe) & FdFlag::FD_CLOEXEC.bits(), 0);
    assert_ne!(flags(left.file_fd) & FdFlag::FD_CLOEXEC.bits(), 0);
}

/// What holds the writing end of the pipe whose reading end is `reader`, by `lsof` of the whole
/// machine: the processes that hold the object the reading end is connected to.
fn holders_of_the_writer(reader: i32) -> String {
    let own = held_by(std::process::id()).unwrap();
    let Some(peer) = own
        .iter()
        .find(|held| held.fd == reader.unsigned_abs())
        .and_then(|held| held.name.strip_prefix("->"))
        .map(str::to_owned)
    else {
        return "the reading end is not listed".to_owned();
    };
    let lsof = Invocation::new(LSOF, Path::new(".")).args(["-n", "-P"]);
    let shown = process::capture(&lsof, &Env::default()).unwrap();
    shown
        .stdout
        .lines()
        .filter(|line| line.contains(&peer) && !line.contains(&format!("->{peer}")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// How many times a reproduction runs the pipe's positive half.
const ROUNDS: usize = 500;

/// Sixteen threads that start `sleep 1` bare, as the tests beside these start programs, until
/// dropped.
struct BareStarts {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl BareStarts {
    #[allow(clippy::disallowed_methods)] // The starts are std's own, as a sibling test's are.
    fn begin() -> Self {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let stop = Arc::new(AtomicBool::new(false));
        let threads = (0..16)
            .map(|_| {
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || {
                    let mut live = std::collections::VecDeque::new();
                    while !stop.load(Ordering::Relaxed) {
                        let mut sleep = std::process::Command::new("/bin/sleep");
                        sleep
                            .arg("1")
                            .stdin(std::process::Stdio::null())
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null());
                        live.push_back(sleep.spawn().unwrap());
                        if live.len() >= 32 {
                            live.pop_front().unwrap().wait().unwrap();
                        }
                    }
                    for mut sleep in live {
                        sleep.wait().unwrap();
                    }
                })
            })
            .collect();
        Self { stop, threads }
    }
}

impl Drop for BareStarts {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// The pipe's positive half [`ROUNDS`] times beside [`BareStarts`], in this process;
/// `left_at_once` leaves the pipe to any program from the moment it is made, as round 5's first
/// `Left::make` did. Says each run that did not reach the end, and who held the writing end then.
fn pipe_under_starts(left_at_once: bool) -> usize {
    let _starts = BareStarts::begin();
    let folder = tempfile::tempdir().unwrap();
    let mut missed = 0;
    for run in 0..ROUNDS {
        let mut left = Left::make(folder.path()).unwrap();
        if left_at_once {
            left.leave().unwrap();
        }
        let reader = left.reader.as_ref().unwrap().as_raw_fd();
        left.given();
        let waits = Waits::new(std::time::Duration::from_millis(300));
        // A wait that misses leaves its reader reading: the reading end is still open to ask about.
        if let Err(missed_end) = left.closed_everywhere(&waits) {
            missed += 1;
            eprintln!("run {run}: {missed_end}");
            eprintln!("  holders: {}", holders_of_the_writer(reader));
        }
    }
    eprintln!("{missed} of {ROUNDS} runs did not reach the end");
    missed
}

#[test]
#[ignore = "a reproduction, by hand: `cargo test -p xtask --lib -- --ignored --nocapture pipe_left_under_starts`"]
fn the_pipe_left_under_starts_as_round_5_first_left_it() {
    pipe_under_starts(true);
}

#[test]
#[ignore = "a reproduction, by hand: `cargo test -p xtask --lib -- --ignored --nocapture pipe_left_under_starts`"]
fn the_pipe_left_under_starts_as_it_is_left_now() {
    pipe_under_starts(false);
}

/// The test as the gate runs it, in a process of its own, a hundred times beside
/// [`BareStarts`] in this one: no start of this process can hold what the other made.
#[test]
#[ignore = "a stress, by hand: `cargo test -p xtask --lib -- --ignored --nocapture pipe_left_alone_under_starts`"]
fn the_pipe_left_alone_under_starts() {
    let _starts = BareStarts::begin();
    for _ in 0..100 {
        the_pipe_left_reaches_its_end_once_no_process_holds_it_and_not_before();
    }
}
