//! A member's window runs `pkill -f x`, `killall x` or `taskkill /IM x.exe` and the stand-in is
//! what it finds, whatever else the PATH holds: the folder the daemon makes
//! (`cf_daemon::member_guard`) holds `cf` under those names, first on the window's PATH
//! (`WindowEnv`, as `start` builds it). A recorder named like the real command sits later on
//! the PATH; no test here can start a real utility: the system's folder of a window is a scratch
//! one (empty on a Mac; on Windows it holds this test's own executable as `taskkill.exe` and
//! `tskill.exe`, and `SystemRoot` and `windir` name it, in every shell from the first case),
//! so a call that goes through reaches a harmless program, and the test says which program
//! received each call.

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
    /// The window's system folder: scratch, never the machine's.
    system: PathBuf,
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
        let system = home.join("system");
        std::fs::create_dir_all(&system).unwrap();
        #[cfg(windows)]
        for program in ["taskkill.exe", "tskill.exe"] {
            std::fs::create_dir_all(system.join("System32")).unwrap();
            std::fs::copy(
                std::env::current_exe().unwrap(),
                system.join("System32").join(program),
            )
            .unwrap();
        }
        let system_folder = if cfg!(windows) {
            system.join("System32")
        } else {
            system.clone()
        };
        let delimiter = if cfg!(windows) { ';' } else { ':' };
        let given = Env::from_vars([(
            "PATH",
            format!(
                "{}{delimiter}{}",
                recorder.display(),
                system_folder.display()
            ),
        )]);
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
            system,
            path,
        }
    }

    /// `command` given the window's PATH and, on Windows, the scratch system folder as its
    /// `SystemRoot` and `windir`: where the real `taskkill` is looked for.
    fn given<'a>(&self, command: &'a mut Command) -> &'a mut Command {
        command.env("PATH", &self.path);
        if cfg!(windows) {
            command
                .env("SystemRoot", &self.system)
                .env("windir", &self.system);
        }
        command
    }

    /// Whether a recorder ran.
    fn reached(&self, command: &str) -> bool {
        self.recorder.join(format!("{command}.reached")).exists()
    }

    /// `line` run by the window's shell on its PATH: its exit code, standard output and error.
    fn run(&self, line: &str) -> (Option<i32>, String, String) {
        let mut shell = if cfg!(windows) {
            let mut shell = Command::new("cmd");
            shell.args(["/c", line]);
            shell
        } else {
            let mut shell = Command::new("/bin/sh");
            shell.args(["-c", line]);
            shell
        };
        let ran = self
            .given(&mut shell)
            .env("CONSENSFLOW_TOOL_FOLDERS", "")
            .output()
            .unwrap();
        (
            ran.status.code(),
            String::from_utf8_lossy(&ran.stdout).into_owned(),
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
        .filter_map(std::env::var_os)
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
    let guard = folder_of(&window);
    for (line, command) in [
        ("pkill -f x", "pkill"),
        ("pkill", "pkill"),
        ("killall x", "killall"),
        ("killall -9 node", "killall"),
    ] {
        // The name is asked first, and what it resolves to must be the guard's own file; the
        // words are then asked of that exact path, never of a bare name.
        let (_, found, said) = window.run(&format!("command -v {command}"));
        let wanted = guard.join(command);
        assert_eq!(found.trim_end(), wanted.display().to_string(), "{said}");
        let exact = line.replacen(command, &format!("'{}'", wanted.display()), 1);
        let (code, _, said) = window.run(&exact);
        assert_eq!(code, Some(1), "{exact}: {said}");
        assert!(
            said.contains(&format!(
                "ConsensFlow: {command} {OWNERS_WORDS} by its number"
            )),
            "{exact}: {said}"
        );
        assert!(!window.reached(command), "{line} reached the real command");
    }
}

/// The guard's folder of `window`, the first on its PATH.
#[cfg(unix)]
fn folder_of(window: &Window) -> PathBuf {
    PathBuf::from(window.path.split(':').next().unwrap())
}

/// Whether a call reached the program standing for the real `taskkill` or `tskill`: this
/// test's own executable, which takes the call's words for filters of its tests (`running 0
/// tests`) or refuses them as options of its own, and says so either way. A refused call is
/// answered by ConsensFlow's words, and neither.
#[cfg(windows)]
fn received(out: &str, said: &str) -> bool {
    out.contains("running 0 tests") || said.contains("Unrecognized option")
}

#[cfg(windows)]
#[test]
fn taskkill_by_name_in_a_window_is_refused_and_by_number_goes_to_the_one_the_system_has() {
    // The window's system folder is the scratch one from its first case on: every shell below
    // is given it as `SystemRoot`, so what a call reaches is the test's own executable.
    let window = Window::new("win", &["taskkill", "tskill"]);
    for line in [
        "taskkill /IM x.exe",
        "taskkill /F /im x.exe",
        "taskkill /FI \"IMAGENAME eq x.exe\"",
        "tskill notepad",
    ] {
        let (code, out, said) = window.run(line);
        assert_eq!(code, Some(1), "{line}: {said}");
        assert!(said.contains(OWNERS_WORDS), "{line}: {said}");
        assert!(!received(&out, &said), "{line}: {out} {said}");
        assert!(
            !window.reached("taskkill") && !window.reached("tskill"),
            "{line}"
        );
    }
    // By number, the program the system has receives it.
    for line in ["taskkill /PID 123", "taskkill /F /T /PID 123", "tskill 123"] {
        let (_, out, said) = window.run(line);
        assert!(received(&out, &said), "{line}: {out} {said}");
        assert!(!said.contains("ConsensFlow:"), "{line}: {said}");
    }
    // Both PowerShells, as a harness runs a command in them: Windows PowerShell 5.1, and
    // PowerShell 7 where the machine has it. (`Stop-Process -Name` is a cmdlet: out of reach.)
    for shell in ["powershell.exe", "pwsh.exe"] {
        for line in ["taskkill /IM x.exe", "tskill notepad"] {
            let ran = window
                .given(Command::new(shell).args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    line,
                ]))
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
            let out = String::from_utf8_lossy(&ran.stdout);
            assert_ne!(ran.status.code(), Some(0), "{shell} {line}: {said}");
            assert!(said.contains("ConsensFlow:"), "{shell} {line}: {said}");
            assert!(!received(&out, &said), "{shell} {line}: {out} {said}");
            eprintln!("SHELL {shell} {line}: refused");
        }
        assert!(
            !window.reached("taskkill") && !window.reached("tskill"),
            "{shell}"
        );
    }
    // Git Bash, which Claude Code, Pi and Devin run commands in: it turns `/PID` into a
    // path before a program reads it, so `//PID` is its spelling, and every other call is
    // refused. A call counts as passed only when the program the system has received it.
    let bash = git_bash();
    for (line, goes_through) in [
        ("taskkill //PID 123", true),
        ("taskkill -PID 123 //F", true),
        ("taskkill /PID 123", false),
        ("taskkill /IM x.exe", false),
        ("taskkill //IM x.exe", false),
        ("taskkill /FI 'IMAGENAME eq x.exe'", false),
        ("tskill notepad", false),
    ] {
        let ran = window
            .given(Command::new(&bash).args(["--noprofile", "--norc", "-c", line]))
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&ran.stderr);
        let out = String::from_utf8_lossy(&ran.stdout);
        assert_eq!(
            received(&out, &said),
            goes_through,
            "Git Bash {line}: {out} {said}"
        );
        if goes_through {
            assert!(!said.contains("ConsensFlow:"), "Git Bash {line}: {said}");
        } else {
            assert!(said.contains(OWNERS_WORDS), "Git Bash {line}: {said}");
            assert_eq!(ran.status.code(), Some(1), "Git Bash {line}: {said}");
        }
        eprintln!(
            "SHELL git-bash {line}: {}",
            if goes_through { "through" } else { "refused" }
        );
    }
    assert!(
        !window.reached("taskkill") && !window.reached("tskill"),
        "Git Bash"
    );
    // By number the stand-in itself starts the program the system has, with the arguments as
    // they were: this test's own executable, which takes `/PID` and `123` for filters of its
    // tests and runs none.
    let taskkill = window.home.join("member-guard").join("taskkill.exe");
    let ran = window
        .given(Command::new(&taskkill).args(["/PID", "123"]))
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&ran.stdout);
    let said = String::from_utf8_lossy(&ran.stderr);
    assert_eq!(ran.status.code(), Some(0), "{out} {said}");
    assert!(received(&out, &said), "{out} {said}");
    assert!(!said.contains("ConsensFlow:"), "{said}");
    assert!(!window.reached("taskkill"));
}
