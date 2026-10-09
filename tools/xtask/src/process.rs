//! The one place xtask starts a program. `clippy.toml` lets only cf-process do
//! that, and cf-process is async, with limits and process groups that a build
//! or a test run to its end has no use for.
//!
//! A program runs to its end, in the folder it is told, with the environment
//! xtask was started with (read once, in `main`, as `cf_base::env::Env`) and the
//! difference the [`Invocation`] names. It is found on that environment's PATH,
//! with the extensions Windows starts programs by, so that `npm` is the
//! `npm.cmd` it is there. Its exit code is what [`run`] answers, unchanged.
// The one place a process starts; see above.
#![allow(clippy::disallowed_methods)]

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use cf_base::env::Env;

/// A program to run: which, with what words, where, and how its environment
/// differs from xtask's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The program as a person names it (`node`, `cargo`), found on the PATH
    /// when it is started; a path is started as it is.
    pub program: OsString,
    pub args: Vec<OsString>,
    /// The folder it runs in, which has to be there.
    pub cwd: PathBuf,
    /// The variables it is given on top of xtask's own, in order; `None` takes
    /// one away.
    pub vars: Vec<(OsString, Option<OsString>)>,
}

impl Invocation {
    /// `program`, run in `cwd`, with no words and xtask's own environment.
    pub fn new(program: impl Into<OsString>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: cwd.into(),
            vars: Vec::new(),
        }
    }

    /// With one more word.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// With more words, each as it is: nothing splits or quotes them.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// With the variable `name` set to `value`.
    #[must_use]
    pub fn var(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.vars.push((name.into(), Some(value.into())));
        self
    }

    /// Without the variable `name`, which xtask's environment may have.
    #[must_use]
    pub fn without(mut self, name: impl Into<OsString>) -> Self {
        self.vars.push((name.into(), None));
        self
    }

    /// The command line as a person reads it: a word with a space or a quote
    /// in it, or none at all, quoted. For what xtask says; nothing runs from it.
    pub fn display(&self) -> String {
        let words = std::iter::once(&self.program).chain(&self.args);
        let words: Vec<_> = words.map(|word| quoted(&word.to_string_lossy())).collect();
        words.join(" ")
    }
}

fn quoted(word: &str) -> String {
    let plain =
        !word.is_empty() && !word.contains(|c: char| c.is_whitespace() || c == '"' || c == '\'');
    if plain {
        word.to_string()
    } else {
        format!("{word:?}")
    }
}

/// What a program that ran to its end left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    /// The code it exited with: 1 when a signal ended it and it left none, as
    /// the scripts this replaces read a status that was null.
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// A program that could not be started.
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    #[error("`{program}` was not found: is it installed, and on the PATH?")]
    NotFound { program: String },
    #[error("the folder `{}` to run `{program}` in is not there", folder.display())]
    NoFolder { program: String, folder: PathBuf },
    #[error("could not run `{program}`: {cause}")]
    Refused { program: String, cause: io::Error },
}

/// Runs `invocation` to its end, its input and output being xtask's own: the
/// code it exited with.
pub fn run(invocation: &Invocation, env: &Env) -> Result<i32, Failure> {
    command(invocation, env)
        .status()
        .map(|status| code(&status))
        .map_err(|cause| failure(invocation, cause))
}

/// Runs `invocation` to its end and keeps what it wrote. Its input is closed,
/// and a nonzero code is an answer, for the caller to read: only a program that
/// did not start is an error.
pub fn capture(invocation: &Invocation, env: &Env) -> Result<Captured, Failure> {
    let output = command(invocation, env)
        .stdin(Stdio::null())
        .output()
        .map_err(|cause| failure(invocation, cause))?;
    Ok(Captured {
        code: code(&output.status),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// The program to start for `program`: a bare name found on `env`'s PATH (with
/// Windows's extensions, if it is Windows's environment), else as it is.
pub fn resolved(program: &OsStr, env: &Env) -> OsString {
    let bare = Path::new(program).components().count() == 1;
    program
        .to_str()
        .filter(|_| bare)
        .and_then(|name| cf_process::on_path(name, env))
        .map_or_else(|| program.to_os_string(), PathBuf::into_os_string)
}

fn command(invocation: &Invocation, env: &Env) -> Command {
    let mut command = Command::new(resolved(&invocation.program, env));
    command
        .args(&invocation.args)
        .current_dir(&invocation.cwd)
        .env_clear()
        .envs(env.iter());
    for (name, value) in &invocation.vars {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    command
}

fn code(status: &ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

/// Why `invocation` did not start. A folder that is not there is the first
/// thing to say: the system reports it as a missing file on Unix, as a
/// program that is not there, and as another error on Windows.
fn failure(invocation: &Invocation, cause: io::Error) -> Failure {
    let program = invocation.program.to_string_lossy().into_owned();
    if !invocation.cwd.is_dir() {
        return Failure::NoFolder {
            program,
            folder: invocation.cwd.clone(),
        };
    }
    match cause.kind() {
        io::ErrorKind::NotFound => Failure::NotFound { program },
        _ => Failure::Refused { program, cause },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_line_is_shown_with_what_has_a_space_in_it_quoted() {
        let invocation = Invocation::new("cargo", ".")
            .args(["test", "--", "a b", "", "it's", "plain-1.rs"])
            .arg("say \"x\"");
        assert_eq!(
            invocation.display(),
            r#"cargo test -- "a b" "" "it's" plain-1.rs "say \"x\"""#
        );
    }

    #[test]
    fn the_environment_is_changed_in_the_order_it_is_told() {
        let invocation = Invocation::new("node", ".")
            .var("A", "1")
            .without("B")
            .var("A", "2");
        assert_eq!(
            invocation.vars,
            [
                (OsString::from("A"), Some(OsString::from("1"))),
                (OsString::from("B"), None),
                (OsString::from("A"), Some(OsString::from("2"))),
            ]
        );
    }

    /// A folder with a file of each name, as the files `PATH` entries hold.
    fn folder_with(names: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for name in names {
            let file = dir.path().join(name);
            std::fs::write(&file, "").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        dir
    }

    #[test]
    fn a_bare_name_is_found_on_the_path_and_a_path_is_left_as_it_is() {
        // A program is a file with the extension its system starts programs by.
        let file = format!("tool{}", std::env::consts::EXE_SUFFIX);
        let dir = folder_with(&[&file]);
        let path = dir.path().as_os_str();
        let env = Env::from_vars([("PATH", path)]);
        assert_eq!(
            resolved(OsStr::new("tool"), &env),
            dir.path().join(&file).into_os_string()
        );
        // Not on the PATH: left for the system to say so when it is started.
        assert_eq!(resolved(OsStr::new("other"), &env), OsString::from("other"));
        // A path names its file; the PATH is not asked.
        let named = dir.path().join(&file);
        assert_eq!(
            resolved(named.as_os_str(), &Env::default()),
            named.into_os_string()
        );
        assert_eq!(
            resolved(OsStr::new("./tool"), &env),
            OsString::from("./tool")
        );
    }

    #[test]
    fn on_windows_npm_is_the_cmd_file_and_node_the_exe_by_the_extensions_the_system_names() {
        let dir = folder_with(&["npm.cmd", "node.exe", "npm"]);
        let windows = |pathext: Option<&str>| {
            let mut vars = vec![
                ("OS", "Windows_NT".to_string()),
                ("PATH", dir.path().display().to_string()),
            ];
            vars.extend(pathext.map(|value| ("PATHEXT", value.to_string())));
            Env::from_vars(vars)
        };
        let found = |name: &str, env: &Env| resolved(OsStr::new(name), env);
        let file = |name: &str| dir.path().join(name).into_os_string();

        // PATHEXT unset: the system's own list, `.COM;.EXE;.BAT;.CMD`.
        assert_eq!(found("npm", &windows(None)), file("npm.cmd"));
        assert_eq!(found("node", &windows(None)), file("node.exe"));
        // The order PATHEXT names decides, and an extension nothing can start does not count.
        assert_eq!(found("npm", &windows(Some(".EXE;.CMD"))), file("npm.cmd"));
        assert_eq!(
            found("npm", &windows(Some(".JS;.PS1"))),
            OsString::from("npm")
        );
    }
}
