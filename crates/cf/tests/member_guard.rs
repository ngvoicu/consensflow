//! A member's window runs `pkill -f x`, `killall x` or `taskkill /IM x.exe` and the stand-in is
//! what it finds, whatever else the PATH holds: the folder the daemon makes
//! (`cf_daemon::member_guard`) holds `cf` under those names, first on the window's PATH
//! (`WindowEnv`, as `start` builds it). A recorder named like the real command sits later on
//! the PATH; no test here starts the real command by its name.

// The tests start programs themselves.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use cf_base::env::Env;
use cf_daemon::files::Log;
use cf_daemon::member_guard::guard;
use cf_daemon::seams::WindowEnv;

mod common;

/// What the guard answers a refused call with, whatever the command.
const OWNERS_WORDS: &str = "isn't run in member windows. It ends every program whose name \
     matches, other members' and the owner's among them. End a program you started";

struct Window {
    home: PathBuf,
    recorder: PathBuf,
    path: String,
}

impl Window {
    /// A window's PATH as the daemon builds it, with a recorder of each command's name later
    /// on it, which writes `reached` beside itself when it runs.
    fn new(what: &str, commands: &[&str]) -> Self {
        let home = std::env::temp_dir().join(format!("cf-guard-{what}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let recorder = home.join("recorder");
        std::fs::create_dir_all(&recorder).unwrap();
        for command in commands {
            record(&recorder, command);
        }
        let cf = Path::new(env!("CARGO_BIN_EXE_cf"));
        let system = if cfg!(windows) {
            std::env::var("SystemRoot").map_or_else(
                |_| r"C:\Windows\System32".into(),
                |root| format!(r"{root}\System32"),
            )
        } else {
            "/usr/bin:/bin".into()
        };
        let delimiter = if cfg!(windows) { ';' } else { ':' };
        let given =
            Env::from_vars([("PATH", format!("{}{delimiter}{system}", recorder.display()))]);
        let windows = WindowEnv::new(
            &given,
            "http://127.0.0.1:1",
            &cf.parent().unwrap().to_string_lossy(),
        );
        let guarded = guard(windows, &home, cf, &Log::new(&home));
        let path = guarded.path().to_owned();
        Self {
            home,
            recorder,
            path,
        }
    }

    /// Whether a recorder ran.
    fn reached(&self, command: &str) -> bool {
        self.recorder.join(format!("{command}.reached")).exists()
    }

    /// `line` run by the window's shell on its PATH: its exit code and standard error.
    fn run(&self, line: &str) -> (Option<i32>, String) {
        let mut shell = if cfg!(windows) {
            let mut shell = Command::new("cmd");
            shell.args(["/c", line]);
            shell
        } else {
            let mut shell = Command::new("/bin/sh");
            shell.args(["-c", line]);
            shell
        };
        let ran = shell
            .env("PATH", &self.path)
            .env("CONSENSFLOW_TOOL_FOLDERS", "")
            .output()
            .unwrap();
        (
            ran.status.code(),
            String::from_utf8_lossy(&ran.stderr).into_owned(),
        )
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

/// Git for Windows's `bash.exe`, where it installs.
#[cfg(windows)]
fn git_bash() -> PathBuf {
    let found = ["ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(|name| std::env::var_os(name))
        .map(|root| Path::new(&root).join(r"Git\bin\bash.exe"))
        .find(|bash| bash.is_file());
    found.expect("Git Bash is installed with Git for Windows")
}

/// A recorder named `command` in `folder`: it writes `<command>.reached` beside itself.
#[cfg(windows)]
fn record(folder: &Path, command: &str) {
    let marker = folder.join(format!("{command}.reached"));
    std::fs::write(
        folder.join(format!("{command}.cmd")),
        format!("@echo off\r\necho x> \"{}\"\r\n", marker.display()),
    )
    .unwrap();
}

/// A recorder named `command` in `folder`: it writes `<command>.reached` beside itself.
#[cfg(unix)]
fn record(folder: &Path, command: &str) {
    use std::os::unix::fs::PermissionsExt;
    let file = folder.join(command);
    std::fs::write(
        &file,
        format!(
            "#!/bin/sh\n: > \"{}/{command}.reached\"\n",
            folder.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn pkill_and_killall_in_a_window_are_refused_and_the_real_command_is_never_reached() {
    let window = Window::new("mac", &["pkill", "killall"]);
    for (line, command) in [
        ("pkill -f x", "pkill"),
        ("pkill", "pkill"),
        ("killall x", "killall"),
        ("killall -9 node", "killall"),
    ] {
        let (code, said) = window.run(line);
        assert_eq!(code, Some(1), "{line}: {said}");
        assert!(
            said.contains(&format!(
                "ConsensFlow: {command} {OWNERS_WORDS} by its number"
            )),
            "{line}: {said}"
        );
        assert!(!window.reached(command), "{line} reached the real command");
    }
}

#[cfg(windows)]
#[test]
fn taskkill_by_name_in_a_window_is_refused_and_by_number_goes_to_the_real_one() {
    let window = Window::new("win", &["taskkill", "tskill"]);
    for line in [
        "taskkill /IM x.exe",
        "taskkill /F /im x.exe",
        "taskkill /FI \"IMAGENAME eq x.exe\"",
        "tskill notepad",
    ] {
        let (code, said) = window.run(line);
        assert_eq!(code, Some(1), "{line}: {said}");
        assert!(said.contains(OWNERS_WORDS), "{line}: {said}");
        assert!(
            !window.reached("taskkill") && !window.reached("tskill"),
            "{line}"
        );
    }
    // Both PowerShells, as a harness runs a command in them: Windows PowerShell 5.1, and
    // PowerShell 7 where the machine has it. (`Stop-Process -Name` is a cmdlet: out of reach.)
    for shell in ["powershell.exe", "pwsh.exe"] {
        for line in ["taskkill /IM x.exe", "tskill notepad"] {
            let ran = Command::new(shell)
                .args(["-NoProfile", "-NonInteractive", "-Command", line])
                .env("PATH", &window.path)
                .output();
            let ran = match ran {
                Ok(ran) => ran,
                Err(failed) if shell == "pwsh.exe" => {
                    eprintln!("SHELL {shell}: not on this machine ({failed})");
                    continue;
                }
                Err(failed) => panic!("{shell}: {failed}"),
            };
            let said = String::from_utf8_lossy(&ran.stderr);
            assert_ne!(ran.status.code(), Some(0), "{shell} {line}: {said}");
            assert!(said.contains("ConsensFlow:"), "{shell} {line}: {said}");
            eprintln!("SHELL {shell} {line}: refused");
        }
        assert!(
            !window.reached("taskkill") && !window.reached("tskill"),
            "{shell}"
        );
    }
    // Git Bash, which Claude Code, Pi and Devin run commands in: it turns `/PID` into a
    // path before a program reads it, so `//PID` is its spelling, and every other call is
    // refused.
    let bash = git_bash();
    // The test's own executable stands for the real taskkill (below), so a call that goes
    // through ends nothing.
    let fake_root = window.home.join("bash-root");
    std::fs::create_dir_all(fake_root.join("System32")).unwrap();
    std::fs::copy(
        std::env::current_exe().unwrap(),
        fake_root.join(r"System32\taskkill.exe"),
    )
    .unwrap();
    for (line, goes_through) in [
        ("taskkill //PID 123", true),
        ("taskkill -PID 123 //F", true),
        ("taskkill /PID 123", false),
        ("taskkill /IM x.exe", false),
        ("taskkill //IM x.exe", false),
        ("taskkill /FI 'IMAGENAME eq x.exe'", false),
        ("tskill notepad", false),
    ] {
        let ran = Command::new(&bash)
            .args(["--noprofile", "--norc", "-c", line])
            .env("PATH", &window.path)
            .env("SystemRoot", &fake_root)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&ran.stderr);
        assert_eq!(
            !said.contains("ConsensFlow:"),
            goes_through,
            "Git Bash {line}: {said}"
        );
        assert_eq!(
            ran.status.code() == Some(0),
            goes_through,
            "Git Bash {line}: {said}"
        );
        eprintln!(
            "SHELL git-bash {line}: {}",
            if goes_through { "through" } else { "refused" }
        );
    }
    assert!(
        !window.reached("taskkill") && !window.reached("tskill"),
        "Git Bash"
    );
    // By number it is the real program, with the arguments as they were: a SystemRoot of the
    // test's holds a harmless program as the real taskkill: the test's own executable.
    let root = window.home.join("root");
    std::fs::create_dir_all(root.join("System32")).unwrap();
    std::fs::copy(
        std::env::current_exe().unwrap(),
        root.join(r"System32\taskkill.exe"),
    )
    .unwrap();
    let taskkill = window.home.join("member-guard").join("taskkill.exe");
    let through = |args: &[&str]| {
        let ran = Command::new(&taskkill)
            .args(args)
            .env("SystemRoot", &root)
            .output()
            .unwrap();
        (
            ran.status.code(),
            String::from_utf8_lossy(&ran.stdout).to_string()
                + &String::from_utf8_lossy(&ran.stderr),
        )
    };
    // The real program here is this test's own executable, which lists its tests for `--list`
    // and takes `/PID 123` for a filter of them: it ran, with the arguments as they were given.
    let (code, said) = through(&["--list"]);
    assert_eq!(code, Some(0), "{said}");
    assert!(said.contains("taskkill_by_name_in_a_window"), "{said}");
    let (code, said) = through(&["/PID", "123"]);
    assert_eq!(code, Some(0), "{said}");
    assert!(!said.contains("ConsensFlow:"), "{said}");
    assert!(!window.reached("taskkill"));
}
