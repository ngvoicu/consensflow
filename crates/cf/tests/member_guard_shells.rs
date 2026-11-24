//! The guard holds in the shells a window's harness may run a command in, a login shell among
//! them (T-2213, round 2): a login shell's `/etc/zprofile` or `/etc/profile` runs `path_helper`,
//! which puts the system's folders before the PATH's own, so the guard's folder alone is not
//! enough there; zsh and bash are also given functions of the same names (the window's `ZDOTDIR`
//! and exported functions). Real shells, started as a window's are with its environment; the
//! calls name no program a machine has (`pkill -f zz-…`), so the real command could end nothing
//! if it ran, and a recorder named like it sits on the PATH where it would be found.

#![cfg(unix)]
// The tests start programs themselves.
#![allow(clippy::disallowed_methods, clippy::unwrap_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use cf_base::env::Env;
use cf_daemon::files::Log;
use cf_daemon::member_guard::guard;
use cf_daemon::seams::WindowEnv;

const WORDS: &str = "isn't run in member windows. It ends every program whose name matches, other \
     members' and the owner's among them. End a program you started by its number, after \
     checking it's yours.";
const PKILL: &str = "pkill -f zz-no-such-cmdline-t2213";
const KILLALL: &str = "killall zz-no-such-program-t2213";

struct Scene {
    root: PathBuf,
    home: PathBuf,
    recorder: PathBuf,
    path: String,
    vars: Vec<(String, String)>,
}

impl Scene {
    /// A window's environment as the daemon builds it, over a home of the test's own, given the
    /// person's `ZDOTDIR` of the daemon's.
    fn new(what: &str, zdotdir: Option<&Path>) -> Self {
        let root = std::env::temp_dir().join(format!("cf-guard-sh-{what}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let recorder = root.join("recorder");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&recorder).unwrap();
        for command in ["pkill", "killall"] {
            let file = recorder.join(command);
            std::fs::write(
                &file,
                format!(
                    "#!/bin/sh\n: > \"{}/{command}.reached\"\n",
                    recorder.display()
                ),
            )
            .unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut given = vec![(
            "PATH".to_owned(),
            format!("{}:/usr/bin:/bin", recorder.display()),
        )];
        if let Some(zdotdir) = zdotdir {
            given.push(("ZDOTDIR".to_owned(), zdotdir.display().to_string()));
        }
        let cf = Path::new(env!("CARGO_BIN_EXE_cf"));
        let windows = WindowEnv::new(&Env::from_vars(given), "http://127.0.0.1:1", "/bundle/bin");
        let guarded = guard(windows, &root, cf, &Log::new(&root));
        Self {
            path: guarded.path().to_owned(),
            vars: guarded.guard_vars().to_vec(),
            root,
            home,
            recorder,
        }
    }

    /// `shell` with `flags` and `line`, in the window's environment: the PATH and the shell
    /// layer (`with_functions`), the home of the test's, nothing else.
    fn run(
        &self,
        shell: &str,
        flags: &str,
        line: &str,
        with_functions: bool,
    ) -> (Option<i32>, String, String) {
        let mut command = Command::new(shell);
        command
            .arg(flags)
            .arg(line)
            .env_clear()
            .env("HOME", &self.home)
            .env("TERM", "dumb")
            .env(
                "PATH",
                if with_functions {
                    &self.path
                } else {
                    self.path.rsplit_once(':').map_or("", |_| &self.path)
                },
            );
        if with_functions {
            command.envs(self.vars.iter().map(|(n, v)| (n, v)));
        }
        let ran = command.output().unwrap();
        (
            ran.status.code(),
            String::from_utf8_lossy(&ran.stdout).into_owned(),
            String::from_utf8_lossy(&ran.stderr).into_owned(),
        )
    }

    fn reached(&self, command: &str) -> bool {
        self.recorder.join(format!("{command}.reached")).exists()
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Every shell form of the brief that this machine has.
fn forms() -> Vec<(&'static str, &'static str)> {
    let mut forms = vec![
        ("/bin/zsh", "-c"),
        ("/bin/zsh", "-lc"),
        ("/bin/zsh", "-ilc"),
        ("/bin/bash", "-c"),
        ("/bin/bash", "-lc"),
        ("/bin/sh", "-c"),
    ];
    for brew in ["/opt/homebrew/bin/bash", "/usr/local/bin/bash"] {
        if Path::new(brew).exists() {
            forms.push((brew, "-c"));
            forms.push((brew, "-lc"));
        }
    }
    forms
}

fn refused(said: &str, command: &str) -> bool {
    said.contains(&format!("ConsensFlow: {command} {WORDS}"))
}

#[test]
fn pkill_and_killall_are_refused_in_every_shell_form_and_the_layer_that_caught_each_is_named() {
    let scene = Scene::new("forms", None);
    for (shell, flags) in forms() {
        if !Path::new(shell).exists() {
            continue;
        }
        for (line, command) in [(PKILL, "pkill"), (KILLALL, "killall")] {
            let (code, _, said) = scene.run(shell, flags, line, true);
            assert_eq!(code, Some(1), "{shell} {flags} {line}: {said}");
            assert!(refused(&said, command), "{shell} {flags} {line}: {said}");
            // With the PATH alone: which layer held it.
            let (_, _, bare) = scene.run(shell, flags, line, false);
            let layer = if refused(&bare, command) {
                "PATH"
            } else {
                "functions"
            };
            eprintln!("LAYER {shell} {flags} {command}: {layer}");
        }
    }
    assert!(!scene.reached("pkill") && !scene.reached("killall"));
}

/// Startup files of the person's in `dir`, each writing its name to `marks`.
fn person_files(dir: &Path, marks: &Path, names: &[&str], extra: &str) {
    std::fs::create_dir_all(dir).unwrap();
    for name in names {
        let mut text = format!("echo {name}-read >> '{}'\n", marks.display());
        if *name == ".zshrc" {
            text.push_str(extra);
        }
        std::fs::write(dir.join(name), text).unwrap();
    }
}

fn marks_of(marks: &Path) -> String {
    std::fs::read_to_string(marks).unwrap_or_default()
}

const ALL: [&str; 3] = [".zshenv", ".zprofile", ".zshrc"];

#[test]
fn a_window_zsh_reads_the_persons_zshenv_zprofile_and_zshrc_with_no_zdotdir_of_theirs() {
    let scene = Scene::new("zsh-none", None);
    let marks = scene.root.join("marks");
    person_files(&scene.home, &marks, &ALL, "");
    let (code, out, said) = scene.run("/bin/zsh", "-ilc", "echo ZD=${ZDOTDIR-unset}", true);
    assert_eq!(code, Some(0), "{said}");
    assert_eq!(
        marks_of(&marks),
        ".zshenv-read\n.zprofile-read\n.zshrc-read\n",
        "{said}"
    );
    assert!(
        out.contains("ZD=unset"),
        "the person's own shells see no ZDOTDIR of ours: {out}"
    );
}

#[test]
fn a_window_zsh_reads_the_files_of_a_zdotdir_the_person_had_before_consensflow_started() {
    let own = std::env::temp_dir().join(format!("cf-guard-sh-own-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&own);
    let scene = Scene::new("zsh-before", Some(&own));
    let marks = scene.root.join("marks");
    person_files(&own, &marks, &ALL, "");
    // Files in the home are no part of it.
    person_files(&scene.home, &scene.root.join("wrong"), &ALL, "");
    let (code, out, said) = scene.run("/bin/zsh", "-ilc", "echo ZD=$ZDOTDIR", true);
    let _ = std::fs::remove_dir_all(&own);
    assert_eq!(code, Some(0), "{said}");
    assert_eq!(
        marks_of(&marks),
        ".zshenv-read\n.zprofile-read\n.zshrc-read\n",
        "{said}"
    );
    assert!(!scene.root.join("wrong").exists());
    assert!(out.contains(&format!("ZD={}", own.display())), "{out}");
}

#[test]
fn a_window_zsh_reads_the_files_of_a_zdotdir_the_persons_own_zshenv_sets() {
    let scene = Scene::new("zsh-inside", None);
    let marks = scene.root.join("marks");
    let moved = scene.root.join("moved");
    person_files(&moved, &marks, &[".zprofile", ".zshrc"], "");
    std::fs::write(
        scene.home.join(".zshenv"),
        format!(
            "echo .zshenv-read >> '{}'\nexport ZDOTDIR='{}'\n",
            marks.display(),
            moved.display()
        ),
    )
    .unwrap();
    let (code, _, said) = scene.run("/bin/zsh", "-ilc", "true", true);
    assert_eq!(code, Some(0), "{said}");
    assert_eq!(
        marks_of(&marks),
        ".zshenv-read\n.zprofile-read\n.zshrc-read\n",
        "{said}"
    );
    // And the guard stands after all of it.
    let (_, _, said) = scene.run("/bin/zsh", "-ilc", PKILL, true);
    assert!(refused(&said, "pkill"), "{said}");
}

#[test]
fn a_pkill_alias_or_function_in_the_persons_zshrc_wins_in_an_interactive_zsh() {
    // A limit, said as one: the person's own definition is theirs.
    let scene = Scene::new("zsh-alias", None);
    let marks = scene.root.join("marks");
    person_files(
        &scene.home,
        &marks,
        &[".zshrc"],
        "alias pkill='echo mine'\nkillall() { echo mine-fn \"$@\"; }\n",
    );
    let (code, out, said) = scene.run("/bin/zsh", "-ic", PKILL, true);
    assert_eq!(code, Some(0), "{said}");
    assert!(out.contains("mine -f"), "{out}");
    let (_, out, _) = scene.run("/bin/zsh", "-ic", KILLALL, true);
    assert!(out.contains("mine-fn"), "{out}");
    assert!(!scene.reached("pkill") && !scene.reached("killall"));
}
