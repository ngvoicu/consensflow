//! The guard holds in the shells a window's harness may run a command in, a Mac's login shells
//! among them, however one shell starts another: a login shell runs `/usr/libexec/path_helper`,
//! which reads the guard's root (`PATH_HELPER_ROOT`) and so keeps the guard's folder ahead of
//! the system's. Real shells, started with a window's environment, in a scratch home whose
//! startup files put folders of their own first.
//!
//! No case runs a command by a name that could reach the real one: each shell is asked which
//! program the name resolves to (`whence -p`, `type -P`, `command -v`), and only once that is
//! the guard's own is the name run, for its words. Every shell runs where it may signal
//! nothing but itself (`sandbox-exec`), so nothing it runs could end a program.

#![cfg(target_os = "macos")]
// The tests start programs themselves.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use cf_base::env::Env;
use cf_daemon::files::Log;
use cf_daemon::member_guard::{folder, guard};
use cf_daemon::seams::WindowEnv;

const WORDS: &str = "isn't run in member windows. It ends every program whose name matches, other \
     members' and the owner's among them. End a program you started by its number, after \
     checking it's yours.";

/// Every form of the brief: a shell, its flags, and how it says what a name resolves to.
const FORMS: [(&str, &str, &str); 6] = [
    ("/bin/zsh", "-c", "whence -p"),
    ("/bin/zsh", "-lc", "whence -p"),
    ("/bin/zsh", "-ilc", "whence -p"),
    ("/bin/bash", "-c", "type -P"),
    ("/bin/bash", "-lc", "type -P"),
    ("/bin/sh", "-c", "command -v"),
];

/// Nothing run here can signal a program that is not itself.
const SANDBOX: &str = "(version 1)(allow default)(deny signal)(allow signal (target self))";

struct Scene {
    root: PathBuf,
    home: PathBuf,
    guard: PathBuf,
    env: Vec<(String, String)>,
}

impl Scene {
    /// A window's environment as the daemon builds it, over a home of the test's own (whose
    /// folders have a space, as a Mac's may), and startup files that put folders of the
    /// person's first.
    fn new(what: &str) -> Self {
        let root = std::env::temp_dir().join(format!("cf guard sh {what} {}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        std::fs::create_dir_all(home.join("zsh bin")).unwrap();
        std::fs::create_dir_all(home.join("bash bin")).unwrap();
        for (file, bin) in [(".zshrc", "zsh bin"), (".bash_profile", "bash bin")] {
            std::fs::write(
                home.join(file),
                format!("export PATH=\"$HOME/{bin}:$PATH\"\n"),
            )
            .unwrap();
        }
        let cf = Path::new(env!("CARGO_BIN_EXE_cf"));
        let given = Env::from_vars([("PATH", "/usr/bin:/bin")]);
        let windows = WindowEnv::new(&given, "http://127.0.0.1:1", "/bundle/bin");
        let guarded = guard(windows, &root, cf, &Log::new(&root));
        let env = vec![
            ("HOME".to_owned(), home.display().to_string()),
            ("TERM".to_owned(), "dumb".to_owned()),
            ("PATH".to_owned(), guarded.path().to_owned()),
            (
                "PATH_HELPER_ROOT".to_owned(),
                guarded
                    .guard_root()
                    .expect("a Mac's windows are given a root")
                    .to_owned(),
            ),
        ];
        Self {
            guard: folder(&root),
            env,
            root,
            home,
        }
    }

    /// `line` run by `shell` with `flags`: its exit code, standard output and error.
    fn run(&self, shell: &str, flags: &str, line: &str) -> (Option<i32>, String, String) {
        let ran = Command::new("/usr/bin/sandbox-exec")
            .args(["-p", SANDBOX, shell, flags, line])
            .env_clear()
            .envs(self.env.iter().map(|(name, value)| (name, value)))
            .current_dir(&self.home)
            .output()
            .unwrap();
        (
            ran.status.code(),
            String::from_utf8_lossy(&ran.stdout).into_owned(),
            String::from_utf8_lossy(&ran.stderr).into_owned(),
        )
    }

    /// What `shell` resolves each name to must be the guard's, as `how` asks it: every line of
    /// its output the guard's own file.
    fn resolves_to_the_guard(&self, shell: &str, flags: &str, line: &str, what: &str) {
        let (_, out, said) = self.run(shell, flags, line);
        let wanted: Vec<String> = ["pkill", "killall"]
            .iter()
            .map(|name| self.guard.join(name).display().to_string())
            .collect();
        let found: Vec<&str> = out.lines().collect();
        assert_eq!(found, wanted, "{what}: {said}");
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn resolving(how: &str) -> String {
    format!("{how} pkill; {how} killall")
}

/// `text` as one word of a POSIX shell.
fn word(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[test]
fn pkill_and_killall_resolve_to_the_guard_in_every_form_alone_and_each_inside_each() {
    let scene = Scene::new("resolve");
    for (shell, flags, how) in FORMS {
        let alone = format!("{shell} {flags}");
        scene.resolves_to_the_guard(shell, flags, &resolving(how), &alone);
        for (child, child_flags, child_how) in FORMS {
            let line = format!("{child} {child_flags} {}", word(&resolving(child_how)));
            let both = format!("{child} {child_flags} inside {alone}");
            scene.resolves_to_the_guard(shell, flags, &line, &both);
        }
    }
}

#[test]
fn each_form_is_refused_in_the_words_once_the_guard_is_what_it_resolves_to() {
    let scene = Scene::new("words");
    for (shell, flags, how) in FORMS {
        let form = format!("{shell} {flags}");
        // Asked first: nothing is run by name unless it is the guard's.
        scene.resolves_to_the_guard(shell, flags, &resolving(how), &form);
        for (line, command) in [
            ("pkill -f zz-no-such-cmdline-t2213", "pkill"),
            ("killall zz-no-such-program-t2213", "killall"),
        ] {
            let (code, _, said) = scene.run(shell, flags, line);
            assert_eq!(code, Some(1), "{form} {line}: {said}");
            assert!(
                said.contains(&format!("ConsensFlow: {command} {WORDS}")),
                "{form} {line}: {said}"
            );
        }
    }
}
