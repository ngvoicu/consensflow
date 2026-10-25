//! The machines, their lines and the default. The lines of a machine reached through WSL are
//! run here by `/bin/sh`, as WSL's shell runs them, with a stand-in for Windows' PowerShell; the
//! default is asked through an ssh that runs what it is given here.

use super::*;

#[test]
fn the_names_are_the_two_machines_and_a_host_is_reached_as_zeewin_is() {
    let zeewin = Machine::named("zeewin").unwrap();
    assert_eq!(
        (zeewin.name.as_str(), zeewin.host.as_str()),
        ("zeewin", "zeewin")
    );
    assert_eq!(zeewin.reach, Reach::OpenSsh);
    assert_eq!(zeewin, Machine::at("zeewin"));
    let zeewin2 = Machine::named("zeewin2").unwrap();
    assert_eq!(
        (zeewin2.name.as_str(), zeewin2.host.as_str()),
        ("zeewin2", "zeewin2")
    );
    let Reach::Wsl(wsl) = &zeewin2.reach else {
        panic!("zeewin2 is reached through WSL: {zeewin2:?}");
    };
    assert_eq!(wsl.profile, "/mnt/c/Users/AGV");
    assert_eq!(wsl.busy, ".compile-lock");
    assert_eq!(Machine::named("laptop"), None);
    for name in NAMES {
        assert_eq!(Machine::named(name).unwrap().name, name);
    }
    assert_eq!(Machine::at("box").reach, Reach::OpenSsh);
}

#[test]
fn zeewins_lines_and_paths_are_what_they_were() {
    let zeewin = Machine::at("zeewin");
    let line = "powershell -NoProfile -NonInteractive -EncodedCommand QUJD";
    assert_eq!(zeewin.line("QUJD"), line);
    assert_eq!(zeewin.run_line("QUJD", "consensflow-build.1.tgz"), line);
    assert_eq!(
        zeewin.in_profile("consensflow-build.1.tgz"),
        "zeewin:consensflow-build.1.tgz"
    );
    assert_eq!(zeewin.node(), r"C:\Program Files\nodejs");
}

#[test]
fn zeewin2_is_sent_to_its_windows_profile_and_runs_ci_s_node() {
    let zeewin2 = Machine::named("zeewin2").unwrap();
    assert_eq!(
        zeewin2.in_profile("consensflow-build.1.tgz"),
        "zeewin2:/mnt/c/Users/AGV/consensflow-build.1.tgz"
    );
    assert_eq!(zeewin2.node(), r"$env:USERPROFILE\cf-tools\node");
    assert_eq!(
        zeewin2.line("QUJD"),
        "cd '/mnt/c/Users/AGV' && exec '/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe' -NoProfile -NonInteractive -EncodedCommand QUJD"
    );
}

#[test]
fn a_shell_word_is_quoted_whatever_it_holds() {
    assert_eq!(sh_quoted("a b"), "'a b'");
    assert_eq!(sh_quoted("it's"), r"'it'\''s'");
}

#[cfg(unix)]
mod on_a_shell {
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use cf_base::env::Env;

    use super::*;

    /// A machine reached through WSL whose Windows is folders of the test's: the profile, a home
    /// for WSL's user, and a PowerShell that writes down where it ran and what it was given, and
    /// ends with 7.
    struct Wsl {
        root: tempfile::TempDir,
        machine: Machine,
    }

    impl Wsl {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            for folder in ["profile", "home", "bin"] {
                fs::create_dir(root.path().join(folder)).unwrap();
            }
            let powershell = root.path().join("bin/powershell.exe");
            let said = root.path().join("said");
            fs::write(
                &powershell,
                format!(
                    "#!/bin/sh\npwd > '{0}'\nprintf '%s\\n' \"$@\" >> '{0}'\necho ran\nexit 7\n",
                    said.display()
                ),
            )
            .unwrap();
            fs::set_permissions(&powershell, fs::Permissions::from_mode(0o755)).unwrap();
            let machine = Machine {
                name: "zeewin2".into(),
                host: "zeewin2".into(),
                reach: Reach::Wsl(super::Wsl {
                    profile: root.path().join("profile").display().to_string(),
                    powershell: powershell.display().to_string(),
                    busy: ".compile-lock".into(),
                    node: "node".into(),
                    floor_gib: 0,
                }),
            };
            Self { root, machine }
        }

        /// The machine, with a floor of free disk no disk has.
        fn short_of_disk(mut self) -> Self {
            if let Reach::Wsl(wsl) = &mut self.machine.reach {
                wsl.floor_gib = 1 << 30;
            }
            self
        }

        fn path(&self, name: &str) -> std::path::PathBuf {
            self.root.path().join(name)
        }

        /// `line` run as ssh runs it there: by the shell, in WSL's user's home.
        fn run(&self, line: &str) -> process::Captured {
            let asked = Invocation::new("/bin/sh", self.path("home"))
                .arg("-c")
                .arg(line);
            let vars: Vec<(OsString, OsString)> =
                vec![("HOME".into(), self.path("home").into_os_string())];
            process::capture(&asked, &Env::from_vars(vars)).unwrap()
        }

        /// A context whose `ssh` runs the line it is given here, as `run` does, or ends with
        /// `status` without running it when one is given.
        fn context(&self, status: Option<i32>) -> Context {
            let ssh = self.path("bin/ssh");
            let body = match status {
                Some(status) => format!("exit {status}"),
                None => "cd \"$HOME\" && exec /bin/sh -c \"$2\"".into(),
            };
            fs::write(&ssh, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&ssh, fs::Permissions::from_mode(0o755)).unwrap();
            let path = std::env::join_paths(std::iter::once(self.path("bin")).chain(
                std::env::split_paths(Env::from_process().os("PATH").unwrap()),
            ))
            .unwrap();
            let vars: Vec<(OsString, OsString)> = vec![
                ("PATH".into(), path),
                ("HOME".into(), self.path("home").into_os_string()),
            ];
            Context {
                root: self.root.path().to_path_buf(),
                env: Env::from_vars(vars),
            }
        }

        fn choose(&self, context: &Context) -> (String, String) {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let chosen = choose_between(
                context,
                "consensflow-build",
                self.machine.clone(),
                Machine::at("zeewin"),
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

    fn said(path: &Path) -> Vec<String> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn a_free_machines_powershell_runs_in_the_profile_with_the_script_and_its_status_comes_back() {
        let wsl = Wsl::new();
        fs::write(wsl.path("profile/b.tgz"), "tree").unwrap();
        let ran = wsl.run(&wsl.machine.run_line("QUJD", "b.tgz"));
        assert_eq!(ran.code, 7);
        assert_eq!(ran.stdout, "ran\n");
        let profile = fs::canonicalize(wsl.path("profile")).unwrap();
        let said = said(&wsl.path("said"));
        assert_eq!(
            fs::canonicalize(&said[0]).unwrap(),
            profile,
            "PowerShell starts in the profile"
        );
        assert_eq!(
            said[1..],
            ["-NoProfile", "-NonInteractive", "-EncodedCommand", "QUJD"]
        );
        assert!(
            wsl.path("profile/b.tgz").exists(),
            "it is left for the run's script, which removes it"
        );
    }

    #[test]
    fn a_run_on_a_machine_poker_lab_holds_is_refused_with_75_and_starts_nothing_and_leaves_the_lock_as_it_was(
    ) {
        let wsl = Wsl::new();
        let lock = wsl.path("home/.compile-lock");
        fs::create_dir(&lock).unwrap();
        fs::write(lock.join("owner"), "poker-lab job 4242\n").unwrap();
        fs::write(wsl.path("profile/b.tgz"), "tree").unwrap();
        fs::write(wsl.path("profile/other.tgz"), "another run's").unwrap();
        let ran = wsl.run(&wsl.machine.run_line("QUJD", "b.tgz"));
        assert_eq!(ran.code, 75);
        assert_eq!(
            ran.stdout,
            "zeewin2 is busy: poker-lab's job holds it (~/.compile-lock), and no run goes while it does.\n\n== exit 75\n"
        );
        assert!(!wsl.path("said").exists(), "no PowerShell was started");
        assert!(
            !wsl.path("profile/b.tgz").exists(),
            "the run's own tarball is removed"
        );
        assert!(wsl.path("profile/other.tgz").exists(), "and nothing else");
        assert_eq!(
            fs::read_to_string(lock.join("owner")).unwrap(),
            "poker-lab job 4242\n"
        );
    }

    #[test]
    fn a_run_on_a_machine_short_of_disk_is_refused_with_75_and_starts_nothing() {
        let wsl = Wsl::new().short_of_disk();
        fs::write(wsl.path("profile/b.tgz"), "tree").unwrap();
        let ran = wsl.run(&wsl.machine.run_line("QUJD", "b.tgz"));
        assert_eq!(ran.code, 75);
        let said: Vec<&str> = ran.stdout.lines().collect();
        assert_eq!(
            said[0],
            "zeewin2 is short of disk: a run needs 1073741824 GiB free there, and no run goes until it has them."
        );
        assert!(
            said[1].starts_with('(') && said[1].ends_with(" GiB free)"),
            "{said:?}"
        );
        assert_eq!(said[2..], ["", "== exit 75"]);
        assert!(!wsl.path("said").exists(), "no PowerShell was started");
        assert!(
            !wsl.path("profile/b.tgz").exists(),
            "the run's own tarball is removed"
        );
    }

    #[test]
    fn zeewin2s_floor_is_fifty_gib_of_its_c_drive() {
        let Reach::Wsl(wsl) = Machine::named("zeewin2").unwrap().reach else {
            panic!("zeewin2 is reached through WSL");
        };
        assert_eq!(wsl.floor_gib, 50);
        let line = Machine::named("zeewin2").unwrap().run_line("QUJD", "b.tgz");
        assert!(
            line.contains("set -- $(df -P -k '/mnt/c/Users/AGV' | tail -n 1); if [ \"${4:-0}\" -lt 52428800 ]; then"),
            "{line}"
        );
    }

    #[test]
    fn the_stop_and_the_listing_go_whoever_holds_the_machine() {
        let wsl = Wsl::new();
        fs::create_dir(wsl.path("home/.compile-lock")).unwrap();
        let ran = wsl.run(&wsl.machine.line("QUJD"));
        assert_eq!(ran.code, 7);
        assert!(wsl.path("said").exists());
    }

    #[test]
    fn the_default_is_zeewin2_while_it_is_free_and_says_so() {
        let wsl = Wsl::new();
        let (chosen, said) = wsl.choose(&wsl.context(None));
        assert_eq!(chosen, "zeewin2");
        assert_eq!(said, "windows: on zeewin2\n");
    }

    #[test]
    fn the_default_is_zeewin_while_poker_lab_holds_zeewin2() {
        let wsl = Wsl::new();
        fs::create_dir(wsl.path("home/.compile-lock")).unwrap();
        let (chosen, said) = wsl.choose(&wsl.context(None));
        assert_eq!(chosen, "zeewin");
        assert_eq!(
            said,
            "windows: on zeewin, as zeewin2 is not free: poker-lab's job holds it (~/.compile-lock)\n"
        );
        assert!(
            wsl.path("home/.compile-lock").is_dir(),
            "the lock is left as it was"
        );
    }

    #[test]
    fn the_default_is_zeewin_while_a_run_holds_zeewin2s_copy() {
        let wsl = Wsl::new();
        let copy = wsl.path("profile/consensflow-build");
        fs::create_dir(&copy).unwrap();
        fs::write(copy.join(OWNER), "0000beef-00000001\n4242\n").unwrap();
        let (chosen, said) = wsl.choose(&wsl.context(None));
        assert_eq!(chosen, "zeewin");
        assert_eq!(
            said,
            "windows: on zeewin, as zeewin2 is not free: a run goes on there\n"
        );
    }

    #[test]
    fn the_default_is_zeewin_while_zeewin2_is_short_of_disk() {
        let wsl = Wsl::new().short_of_disk();
        let (chosen, said) = wsl.choose(&wsl.context(None));
        assert_eq!(chosen, "zeewin");
        let why = said
            .strip_prefix("windows: on zeewin, as zeewin2 is not free: it has ")
            .unwrap_or_else(|| panic!("{said}"));
        assert!(
            why.ends_with(" GiB free, and a run needs 1073741824\n"),
            "{said}"
        );
    }

    #[test]
    fn the_default_is_zeewin_when_zeewin2_does_not_answer() {
        let wsl = Wsl::new();
        let (chosen, said) = wsl.choose(&wsl.context(Some(255)));
        assert_eq!(chosen, "zeewin");
        assert_eq!(
            said,
            "windows: on zeewin, as zeewin2 is not free: it did not answer (ssh ended with status 255)\n"
        );
    }
}
