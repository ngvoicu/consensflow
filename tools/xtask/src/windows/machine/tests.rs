//! The machines, what a run asks one it shares before it is sent, and the default. The asking
//! goes through an ssh stand-in that answers as the laptop's two sides do; on Windows the script
//! that asks the disk runs in the machine's own PowerShell.

use super::*;

#[test]
fn the_names_are_the_two_machines_and_a_host_is_reached_as_zeewin_is() {
    let zeewin = Machine::named("zeewin").unwrap();
    assert_eq!(zeewin, Machine::at("zeewin"));
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
            running: false
        })
    );
    assert_eq!(
        State::read("free 30\r\nrun\r\n"),
        Some(State {
            free_gib: 30,
            running: true
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
            "(Join-Path (Join-Path $env:USERPROFILE 'consensflow-build') '{OWNER}')"
        )),
        "{script}"
    );
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
    std::fs::write(profile.path().join("copy").join(OWNER), "id\n1\n").unwrap();
    assert!(ask().running);
}

#[cfg(unix)]
mod asked {
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use cf_base::env::Env;

    use super::*;

    /// The laptop's two sides, as the stand-in ssh answers for them: zeelin says `zeelin` (`held`,
    /// `free`) of poker-lab's lock or does not answer (`none`), and zeewin2 answers the disk
    /// script with `zeewin2` (`free N`, then `run` if `running`), or does not answer (`none`).
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
case "$host" in
  zeelin) answer='{zeelin}' ;;
  *) answer='{zeewin2}' ;;
esac
case "$answer" in
  none) echo "ssh: connect to host 10.6.0.6: Operation timed out" >&2; exit 255 ;;
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

        fn ready(&self) -> Result<Ready, String> {
            Machine::named("zeewin2")
                .unwrap()
                .ready(&self.context, "consensflow-build")
        }

        fn choose(&self) -> (String, String) {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let chosen = choose(
                &self.context,
                "consensflow-build",
                &mut Console {
                    out: &mut out,
                    err: &mut err,
                },
            )
            .unwrap();
            assert!(out.is_empty());
            (chosen.name, String::from_utf8(err).unwrap())
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
    fn a_zeelin_that_does_not_answer_holds_no_lock_and_is_said() {
        let laptop = Laptop::new("none", "free 614");
        assert_eq!(
            laptop.ready(),
            Ok(Ready::Go(vec![
                "zeelin did not answer (ssh ended with status 255: ssh: connect to host 10.6.0.6: Operation timed out), so Ubuntu is not running there and no poker-lab job can hold its lock: going on".into()
            ]))
        );
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
        let laptop = Laptop::new("free", "none");
        let (chosen, said) = laptop.choose();
        assert_eq!(chosen, "zeewin");
        assert!(
            said.starts_with("windows: on zeewin, as zeewin2 is not free: zeewin2 did not say what it has free (ssh ended with status 255"),
            "{said}"
        );
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
