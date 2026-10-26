//! The machines, what a run asks one it shares before it is sent, and the default. The asking
//! goes through an ssh stand-in that answers as the laptop's two sides do; on Windows the script
//! that asks the disk runs in the machine's own PowerShell.

use super::*;

#[test]
fn the_names_are_the_two_machines_and_a_host_is_reached_as_zeewin_is() {
    let zeewin = Machine::named("zeewin").unwrap();
    assert_eq!(zeewin.name, "zeewin");
    assert_eq!(zeewin.host, "zeewin");
    assert_eq!(zeewin.node, r"C:\Program Files\nodejs");
    assert_eq!(zeewin.shared, None);
    let zeewin2 = Machine::named("zeewin2").unwrap();
    assert_eq!(
        (zeewin2.name.as_str(), zeewin2.host.as_str()),
        ("zeewin2", "zeewin2")
    );
    assert_eq!(zeewin2.node, r"$env:USERPROFILE\cf-tools\node");
    assert_eq!(
        zeewin2.shared,
        Some(Shared {
            linux: "zeelin".into(),
            lock: ".compile-lock/owner".into(),
            floor_gib: 50,
        })
    );
    // How much each machine takes at once (T-585: zeewin ran out of memory under the workspace's
    // tests with a build job and a test thread for each of its 12 threads; the laptop did not).
    assert_eq!(
        Machine::named("zeewin").unwrap().limits,
        Some(Limits {
            build_jobs: 4,
            test_threads: 4
        })
    );
    assert_eq!(zeewin2.limits, None);
    assert_eq!(
        Machine::at("zeewin").limits,
        None,
        "a host named by --host is not asked for less"
    );
    assert_eq!(Machine::named("laptop"), None);
    for name in NAMES {
        assert_eq!(Machine::named(name).unwrap().name, name);
    }
    assert_eq!(Machine::at("box").shared, None);
}

#[test]
fn what_a_machine_says_of_itself_is_read_for_its_free_disk_and_a_run() {
    assert_eq!(
        State::read("free 614\r\n"),
        Some(State {
            free_gib: 614,
            running: false,
            held: false
        })
    );
    assert_eq!(
        State::read("free 30\r\nrun\r\n"),
        Some(State {
            free_gib: 30,
            running: true,
            held: false
        })
    );
    assert_eq!(
        State::read("free 30\r\nheld\r\n"),
        Some(State {
            free_gib: 30,
            running: false,
            held: true
        })
    );
    assert_eq!(State::read("run\n"), None);
    assert_eq!(State::read("free lots\n"), None);
}

#[test]
fn the_disk_is_asked_of_the_profiles_drive_and_the_run_of_the_copys_owner_file() {
    let script = state_script("consensflow-build");
    assert!(
        script.contains("(Get-Item $env:USERPROFILE).PSDrive.Free / 1GB"),
        "{script}"
    );
    assert!(
        script.contains(&format!(
            "Join-Path (Join-Path $env:USERPROFILE 'consensflow-build') '{OWNER}'"
        )),
        "{script}"
    );
}

/// A lock held by a program that is no run of this runner (a session of another tool that took
/// the file by its protocol: T-708 held zeewin2's for an hour while runs were refused) is told
/// as held, by opening the lock file for what a probe is: a look that takes nothing and holds
/// it for no time.
#[test]
fn the_lock_is_asked_of_by_opening_it_to_share_with_all_and_a_refusal_to_share_is_held() {
    let script = state_script("consensflow-build");
    assert!(
        script.contains(&format!(
            "Join-Path (Join-Path $env:USERPROFILE 'consensflow-build') '{LOCK}'"
        )),
        "{script}"
    );
    assert!(
        script.contains("[IO.File]::Open($lock, 'Open', 'ReadWrite', 'ReadWrite')"),
        "{script}"
    );
    assert!(
        script.contains("catch [IO.IOException] { $true }"),
        "{script}"
    );
    assert!(script.contains("if ($held) { 'held' }"), "{script}");
    // A copy that has no lock yet holds nothing: the file is looked for first, as a folder that
    // is not there is an IOException too.
    assert!(script.contains("Test-Path -LiteralPath $lock"), "{script}");
}

#[test]
fn a_run_goes_on_there_only_if_the_process_the_owner_file_names_is_a_powershell_older_than_it() {
    // A file left by a reboot, a battery or a BSOD names no run (T-610), and a process id that
    // a later program has taken is none either: as the run's own lock reads its process file.
    let script = state_script("consensflow-build");
    for part in [
        "Get-Process -Id ([int]$of[1]) -ErrorAction Stop",
        "($_.Name -in @('powershell', 'pwsh'))",
        "$_.StartTime -le (Get-Item -LiteralPath $owner).LastWriteTime",
    ] {
        assert!(script.contains(part), "{part}: {script}");
    }
    // With no owner file `Get-Content` fails and leaves `$?` false: the script says it ended well.
    assert!(script.ends_with("\nexit 0\n"), "{script}");
}

/// A program started beside the test, as a run's PowerShell is: ended by the test.
#[cfg(windows)]
fn spawned(cwd: &std::path::Path, program: &str, args: &[&str]) -> std::process::Child {
    let started = Invocation::new(program, cwd).args(args.iter().copied());
    process::spawn(
        &started,
        &cf_base::env::Env::from_process(),
        std::process::Stdio::null(),
        false,
    )
    .unwrap()
}

#[cfg(windows)]
#[test]
fn the_disk_script_says_the_free_disk_and_a_run_in_the_machines_own_powershell() {
    let profile = tempfile::tempdir().unwrap();
    let ask = || {
        let asked = Invocation::new("powershell", profile.path())
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(state_script("copy"));
        let env = cf_base::env::Env::from_process();
        let vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = env
            .iter()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("USERPROFILE"))
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .chain([("USERPROFILE".into(), profile.path().into())])
            .collect();
        let answer = process::capture(&asked, &cf_base::env::Env::from_vars(vars)).unwrap();
        assert_eq!(answer.code, 0, "{}", answer.stderr);
        State::read(&answer.stdout).unwrap_or_else(|| panic!("{:?}", answer.stdout))
    };
    let state = ask();
    assert!(state.free_gib > 0, "{state:?}");
    assert!(!state.running);
    std::fs::create_dir(profile.path().join("copy")).unwrap();
    let owner = profile.path().join("copy").join(OWNER);

    // A file naming a PowerShell that is alive, started before the file was written: a run.
    let mut run = spawned(
        profile.path(),
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep 60",
        ],
    );
    std::thread::sleep(std::time::Duration::from_millis(500));
    std::fs::write(&owner, format!("id\r\n{}\r\n", run.id())).unwrap();
    assert!(ask().running, "a live PowerShell is a run");

    // The same file after that process is gone, as after a reboot or a BSOD: no run.
    run.kill().unwrap();
    run.wait().unwrap();
    assert!(!ask().running, "a file naming a dead process is no run");

    // A file that names a process that is no PowerShell (a program that took the id later), a
    // file with no process in it, and one with words in place of an id: no run either.
    let mut other = spawned(profile.path(), "cmd", &["/c", "ping -n 60 127.0.0.1 >nul"]);
    std::fs::write(&owner, format!("id\r\n{}\r\n", other.id())).unwrap();
    assert!(!ask().running, "a process that is no PowerShell is no run");
    other.kill().unwrap();
    other.wait().unwrap();
    for text in ["id\r\n", "", "id\r\nsoon\r\n"] {
        std::fs::write(&owner, text).unwrap();
        assert!(!ask().running, "{text:?}");
    }
}

/// The lock as the machine's own PowerShell and file system tell it: held by a program that opened
/// it to share with no one (a run's script, or T-708's session), not held when nobody has it, and
/// the look takes nothing: the file can be taken by a run straight after.
#[cfg(windows)]
#[test]
fn the_state_script_says_held_for_a_lock_nobody_else_can_open_and_takes_nothing() {
    let profile = tempfile::tempdir().unwrap();
    let ask = || {
        let asked = Invocation::new("powershell", profile.path())
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(state_script("copy"));
        let env = cf_base::env::Env::from_process();
        let vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = env
            .iter()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("USERPROFILE"))
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .chain([("USERPROFILE".into(), profile.path().into())])
            .collect();
        let answer = process::capture(&asked, &cf_base::env::Env::from_vars(vars)).unwrap();
        assert_eq!(answer.code, 0, "{}", answer.stderr);
        State::read(&answer.stdout).unwrap_or_else(|| panic!("{:?}", answer.stdout))
    };
    // No copy yet, then a copy with no lock: nothing holds it.
    assert!(!ask().held);
    let copy = profile.path().join("copy");
    std::fs::create_dir(&copy).unwrap();
    assert!(!ask().held);
    let lock = copy.join(LOCK);
    std::fs::write(&lock, "").unwrap();
    assert!(!ask().held, "a lock file nobody has open is not held");
    // A PowerShell that holds it as a run does.
    let mut holder = spawned(
        profile.path(),
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "$l = [IO.File]::Open('{}', 'OpenOrCreate', 'ReadWrite', 'None'); Start-Sleep 60",
                lock.display()
            ),
        ],
    );
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !ask().held {
        assert!(std::time::Instant::now() < until, "never said held");
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    holder.kill().unwrap();
    holder.wait().unwrap();
    assert!(
        !ask().held,
        "the lock of a program that is gone is held by none"
    );
    // And the look left it free to take.
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock)
        .unwrap();
}

#[cfg(unix)]
mod asked {
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use cf_base::env::Env;
    use cf_base::time::{Clock, SystemClock};

    use super::*;
    use crate::windows::queue::{Queues, Who};

    /// The laptop's two sides, as the stand-in ssh answers for them: zeelin says `zeelin` (`held`,
    /// `free`) of poker-lab's lock, or is not asked at all (`down`: the connection times out, as
    /// the laptop's firewall makes of a port nothing listens on, measured 2026-10-26; `refused`),
    /// or connects and cannot say (`stalled`: no banner; `key`: a host key that changed; `denied`;
    /// `nonsense`: a login banner in place of the answer; `killed`: ssh ended by a signal, which
    /// leaves status 1 and no words). zeewin2 answers the disk script with `zeewin2` (`free N`,
    /// then `run` if `running`), or does not answer (`none`).
    struct Laptop {
        bin: tempfile::TempDir,
        context: Context,
    }

    impl Laptop {
        fn new(zeelin: &str, zeewin2: &str) -> Self {
            let bin = tempfile::tempdir().unwrap();
            let ssh = bin.path().join("ssh");
            fs::write(
                &ssh,
                format!(
                    r#"#!/bin/sh
for word; do host=$last; last=$word; done
printf '%s\t%s\n' "$host" "$last" >> "{log}"
printf '%s\n' "$*" >> "{log}.words"
case "$host" in
  zeelin) answer='{zeelin}' ;;
  *) answer='{zeewin2}' ;;
esac
case "$answer" in
  none|down) echo "ssh: connect to host 10.6.0.6 port 22: Operation timed out" >&2; exit 255 ;;
  refused) echo "ssh: connect to host 10.6.0.6 port 22: Connection refused" >&2; exit 255 ;;
  stalled) printf 'Connection timed out during banner exchange\nConnection to 10.6.0.6 port 22 timed out\n' >&2; exit 255 ;;
  key) echo "Host key verification failed." >&2; exit 255 ;;
  denied) echo "zee@10.6.0.6: Permission denied (publickey)." >&2; exit 255 ;;
  nonsense) echo "Welcome to Ubuntu 26.04" ;;
  killed) kill -INT $$ ;;
  *) printf '%b\n' "$answer" ;;
esac
"#,
                    log = bin.path().join("log").display()
                ),
            )
            .unwrap();
            fs::set_permissions(&ssh, fs::Permissions::from_mode(0o755)).unwrap();
            let path = std::env::join_paths(std::iter::once(bin.path().to_path_buf()).chain(
                std::env::split_paths(Env::from_process().os("PATH").unwrap()),
            ))
            .unwrap();
            let vars: Vec<(OsString, OsString)> = vec![("PATH".into(), path)];
            let context = Context {
                root: bin.path().to_path_buf(),
                env: Env::from_vars(vars),
            };
            Self { bin, context }
        }

        /// The hosts asked, in order.
        fn asked(&self) -> Vec<String> {
            fs::read_to_string(self.bin.path().join("log"))
                .unwrap_or_default()
                .lines()
                .map(|line| line.split('\t').next().unwrap().to_owned())
                .collect()
        }

        /// The words ssh was given before the host, for each call to `host`.
        fn options(&self, host: &str) -> Vec<String> {
            fs::read_to_string(self.bin.path().join("log.words"))
                .unwrap_or_default()
                .lines()
                .filter_map(|line| {
                    format!(" {line}")
                        .split_once(&format!(" {host} "))
                        .map(|(options, _)| options.trim().to_owned())
                })
                .collect()
        }

        fn ready(&self) -> Result<Ready, String> {
            Machine::named("zeewin2")
                .unwrap()
                .ready(&self.context, "consensflow-build")
        }

        fn choose(&self) -> (String, String) {
            let (chosen, said) = self.choose_with(&Interrupt::default());
            (chosen.expect("a machine was chosen").name, said)
        }

        /// The machine chosen, or none if the stop was heard, and what was said.
        fn choose_with(&self, interrupt: &Interrupt) -> (Option<Machine>, String) {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let chosen = choose(
                &self.context,
                "consensflow-build",
                &mut Console {
                    out: &mut out,
                    err: &mut err,
                },
                interrupt,
            )
            .unwrap();
            assert!(out.is_empty());
            (chosen, String::from_utf8(err).unwrap())
        }
    }

    #[test]
    fn a_machine_the_runner_does_not_share_is_asked_nothing() {
        let laptop = Laptop::new("held", "free 1");
        let zeewin = Machine::named("zeewin").unwrap();
        assert_eq!(
            zeewin.ready(&laptop.context, "consensflow-build"),
            Ok(Ready::Go(Vec::new()))
        );
        assert!(laptop.asked().is_empty());
    }

    #[test]
    fn zeewin2_is_ready_when_poker_lab_does_not_hold_it_and_its_disk_has_the_floor() {
        let laptop = Laptop::new("free", "free 614");
        assert_eq!(laptop.ready(), Ok(Ready::Go(Vec::new())));
        assert_eq!(laptop.asked(), ["zeelin", "zeewin2"]);
    }

    #[test]
    fn zeewin2_is_refused_while_poker_lab_holds_it_and_its_disk_is_not_asked() {
        let laptop = Laptop::new("held", "free 614");
        assert_eq!(
            laptop.ready(),
            Ok(Ready::Refused(
                "zeewin2 is busy: poker-lab's job holds it (~/.compile-lock/owner on zeelin), and no run goes while it does.".into()
            ))
        );
        assert_eq!(laptop.asked(), ["zeelin"]);
    }

    #[test]
    fn a_zeelin_nobody_connects_to_holds_no_lock_and_is_said() {
        // WSL stopped: the laptop's firewall drops port 22 (its sshd rule is off, nothing listens),
        // so the connection times out (measured 2026-10-26); a refused one is the same Ubuntu.
        for (zeelin, ssh) in [
            ("down", "Operation timed out"),
            ("refused", "Connection refused"),
        ] {
            let laptop = Laptop::new(zeelin, "free 614");
            assert_eq!(
                laptop.ready(),
                Ok(Ready::Go(vec![format!(
                    "zeelin did not answer (ssh ended with status 255: ssh: connect to host 10.6.0.6 port 22: {ssh}), so Ubuntu is not running there and no poker-lab job can hold its lock: going on"
                )])),
                "{zeelin}"
            );
            assert_eq!(laptop.asked(), ["zeelin", "zeewin2"], "{zeelin}");
        }
    }

    #[test]
    fn a_zeelin_that_connected_and_cannot_say_leaves_the_laptop_unsent_a_run() {
        // sshd is there (Ubuntu runs, perhaps under poker-lab's load) but cannot be asked: not
        // knowing whether the lock is held, no run goes, and the disk is not asked.
        for (zeelin, why) in [
            (
                "stalled",
                "ssh ended with status 255: Connection timed out during banner exchange; Connection to 10.6.0.6 port 22 timed out",
            ),
            (
                "key",
                "ssh ended with status 255: Host key verification failed.",
            ),
            (
                "denied",
                "ssh ended with status 255: zee@10.6.0.6: Permission denied (publickey).",
            ),
            ("nonsense", "it said \"Welcome to Ubuntu 26.04\""),
            ("killed", "ssh ended with status 1"),
        ] {
            let laptop = Laptop::new(zeelin, "free 614");
            assert_eq!(
                laptop.ready(),
                Ok(Ready::Refused(format!(
                    "zeewin2 may be busy: zeelin answered, but not with whether poker-lab's job holds it ({why}), and no run goes while that is not known."
                ))),
                "{zeelin}"
            );
            assert_eq!(laptop.asked(), ["zeelin"], "{zeelin}: the disk is not asked");
        }
    }

    #[test]
    fn a_laptop_whose_ssh_cannot_be_started_cannot_be_sent_a_run() {
        // No ssh to ask with: nothing is known of the lock, and the run is not sent.
        let mut laptop = Laptop::new("free", "free 614");
        let empty = tempfile::tempdir().unwrap();
        let vars: Vec<(OsString, OsString)> =
            vec![("PATH".into(), empty.path().as_os_str().to_owned())];
        laptop.context.env = Env::from_vars(vars);
        match laptop.ready() {
            Ok(Ready::Refused(why)) => assert!(
                why.starts_with("zeewin2 may be busy: zeelin answered, but not with whether poker-lab's job holds it (`ssh` was not found"),
                "{why}"
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn both_probes_of_the_laptop_are_bounded_alike() {
        // The disk probe once had none of these: a connection that hung held the run, and
        // through the lead tool the queue, for ever.
        let laptop = Laptop::new("free", "free 614");
        laptop.ready().unwrap();
        let bounds = "-o ConnectTimeout=5 -o BatchMode=yes -o ServerAliveInterval=5 -o ServerAliveCountMax=1";
        assert_eq!(laptop.options("zeelin"), [bounds]);
        assert_eq!(laptop.options("zeewin2"), [bounds]);
    }

    #[test]
    fn zeewin2_is_refused_below_the_floor_and_says_what_it_has() {
        let laptop = Laptop::new("free", "free 49");
        assert_eq!(
            laptop.ready(),
            Ok(Ready::Refused(
                "zeewin2 is short of disk: a run needs 50 GiB free there, and it has 49.".into()
            ))
        );
        let laptop = Laptop::new("free", "free 50");
        assert_eq!(laptop.ready(), Ok(Ready::Go(Vec::new())));
    }

    #[test]
    fn a_zeewin2_that_does_not_say_its_disk_cannot_be_sent_a_run() {
        let laptop = Laptop::new("free", "none");
        let refused = laptop.ready().unwrap_err();
        assert!(
            refused.starts_with("zeewin2 did not say what it has free (ssh ended with status 255"),
            "{refused}"
        );
        let laptop = Laptop::new("free", "it is Sunday");
        assert_eq!(
            laptop.ready(),
            Err("zeewin2 did not say what it has free: \"it is Sunday\\n\"".into())
        );
    }

    #[test]
    fn zeewin2_is_refused_while_a_run_goes_on_there_or_a_program_holds_its_run_lock() {
        // A run that is only refused by the machine's own lock after its tree is sent wastes the
        // upload, and a runner that waits for a turn must know the machine is not free.
        for (zeewin2, why) in [
            (
                "free 614\\nrun",
                "zeewin2 is busy: a run goes on there, and no run goes while it does.",
            ),
            (
                "free 614\\nheld",
                "zeewin2 is busy: its run lock is held by a program that is not a run of this runner (another tool's session), and no run goes while it does.",
            ),
        ] {
            let laptop = Laptop::new("free", zeewin2);
            assert_eq!(laptop.ready(), Ok(Ready::Refused(why.into())), "{zeewin2}");
        }
    }

    #[test]
    fn the_default_is_zeewin_while_runs_are_queued_for_zeewin2_and_says_how_many() {
        let queues = tempfile::tempdir().unwrap();
        let mut laptop = Laptop::new("free", "free 614");
        laptop.context.env = Env::from_vars(
            laptop
                .context
                .env
                .iter()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .chain([("CF_WINDOWS_QUEUES".into(), queues.path().into())]),
        );
        let now = SystemClock.now_ms();
        let who = Who {
            tree: "a".into(),
            command: "cargo test".into(),
        };
        let kept = Queues::of(&laptop.context.env)
            .unwrap()
            .join("zeewin2", &who, now)
            .unwrap();
        assert_eq!(
            laptop.choose(),
            (
                "zeewin".into(),
                "windows: on zeewin, as zeewin2 is not free: 1 run is queued for it\n".into()
            )
        );
        drop(kept);
        assert_eq!(laptop.choose().0, "zeewin2");
    }

    #[test]
    fn the_default_is_zeewin2_while_it_is_free_and_says_so() {
        let laptop = Laptop::new("free", "free 614");
        assert_eq!(
            laptop.choose(),
            ("zeewin2".into(), "windows: on zeewin2\n".into())
        );
        let laptop = Laptop::new("none", "free 614");
        assert_eq!(
            laptop.choose().0,
            "zeewin2",
            "an Ubuntu that is down holds nothing"
        );
    }

    #[test]
    fn the_default_is_zeewin_while_zeewin2_is_not_free_and_says_why() {
        for (zeelin, zeewin2, why) in [
            (
                "held",
                "free 614",
                "zeewin2 is busy: poker-lab's job holds it (~/.compile-lock/owner on zeelin), and no run goes while it does.",
            ),
            (
                "free",
                "free 30",
                "zeewin2 is short of disk: a run needs 50 GiB free there, and it has 30.",
            ),
            ("free", "free 614\\nrun", "a run goes on there"),
            (
                "free",
                "free 614\\nheld",
                "its run lock is held by a program that is not a run of this runner (another tool's session)",
            ),
        ] {
            let laptop = Laptop::new(zeelin, zeewin2);
            assert_eq!(
                laptop.choose(),
                (
                    "zeewin".into(),
                    format!("windows: on zeewin, as zeewin2 is not free: {why}\n")
                ),
                "{zeelin} {zeewin2}"
            );
        }
        let laptop = Laptop::new("stalled", "free 614");
        let (chosen, said) = laptop.choose();
        assert_eq!(chosen, "zeewin");
        assert!(
            said.starts_with("windows: on zeewin, as zeewin2 is not free: zeewin2 may be busy: zeelin answered, but not with whether poker-lab's job holds it (ssh ended with status 255: Connection timed out during banner exchange"),
            "{said}"
        );
        let laptop = Laptop::new("free", "none");
        let (chosen, said) = laptop.choose();
        assert_eq!(chosen, "zeewin");
        assert!(
            said.starts_with("windows: on zeewin, as zeewin2 is not free: zeewin2 did not say what it has free (ssh ended with status 255"),
            "{said}"
        );
    }

    #[test]
    fn a_stop_heard_while_the_default_is_chosen_chooses_nothing_and_says_nothing() {
        // The Ctrl-C ended the probes with this program: zeewin2 is not "not free" for it (T-610).
        for zeelin in ["killed", "down"] {
            let laptop = Laptop::new(zeelin, "free 614");
            let interrupt = Interrupt::default();
            interrupt.raise();
            let (chosen, said) = laptop.choose_with(&interrupt);
            // A zeelin that is not running still leaves zeewin2 free, and the run goes on to
            // look for the stop itself; one ended by the signal leaves it not free, and that
            // is the stop.
            if zeelin == "killed" {
                assert_eq!((chosen, said.as_str()), (None, ""), "{zeelin}");
            } else {
                assert_eq!(chosen.map(|machine| machine.name), Some("zeewin2".into()));
            }
        }
    }

    #[test]
    fn zeelin_is_asked_of_the_owner_file_and_nothing_else() {
        let laptop = Laptop::new("free", "free 614");
        laptop.ready().unwrap();
        let log = fs::read_to_string(laptop.bin.path().join("log")).unwrap();
        assert_eq!(
            log.lines().next().unwrap(),
            "zeelin\tif [ -e ~/.compile-lock/owner ]; then echo held; else echo free; fi"
        );
    }
}
