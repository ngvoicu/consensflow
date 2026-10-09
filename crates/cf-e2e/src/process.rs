//! The one place cf-e2e starts a program. `clippy.toml` lets only cf-process do
//! that, and a black-box suite depends on no crate of the product, cf-process
//! included: it starts programs with std, runs each to its end (or to the limit
//! it is given), and keeps what it printed, what it said and the code it exited
//! with.
//!
//! A program gets the environment a case names and no other: the case's home
//! points `cf` at its own folders, and nothing of the machine's `~/.consensflow`
//! or of a window this test may itself run in reaches it. A case that needs the
//! test's own environment under its variables asks for it
//! ([`Run::inheriting_env`]). The program's input is closed.
//!
//! On Windows a program is also given the few variables the system needs to
//! start it, where the case names none, from the test's own environment: the
//! JavaScript suites started `cf` with Node, whose libuv does that for every
//! child, and winsock does not start without `SYSTEMROOT`, for one. This module
//! reads the test's environment for that alone.
//!
//! A program that leaves a child of its own holding its output after it is
//! ended holds the run too; the verbs of `cf` start none.
// The one place a program starts and the test's environment is read; see above.
#![allow(clippy::disallowed_methods)]

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::{Error, Result};

/// How long a program may run before it is ended. A verb of `cf` answers at
/// once, and one that waited for ever would hold the whole suite with it.
pub const LIMIT: Duration = Duration::from_secs(30);

/// How often a program that has not ended is looked at again.
const POLL: Duration = Duration::from_millis(2);

/// The variables a program on Windows is given from the test's own environment
/// where the case names none (libuv's `required_vars`, as cf-process has them).
const REQUIRED_ON_WINDOWS: [&str; 11] = [
    "HOMEDRIVE",
    "HOMEPATH",
    "LOGONSERVER",
    "PATH",
    "SYSTEMDRIVE",
    "SYSTEMROOT",
    "TEMP",
    "USERDOMAIN",
    "USERNAME",
    "USERPROFILE",
    "WINDIR",
];

/// A program to run: which, with what words, in which folder, and with what
/// environment.
#[derive(Debug, Clone)]
pub struct Run {
    program: OsString,
    args: Vec<OsString>,
    vars: Vec<(OsString, OsString)>,
    inherit: bool,
    cwd: Option<PathBuf>,
    limit: Option<Duration>,
}

impl Run {
    /// `program`, with no words, none of the test's environment and the
    /// [`LIMIT`]. A path is started as it is; a bare name is left to the
    /// system to find, which the cases here do not rely on.
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().to_os_string(),
            args: Vec::new(),
            vars: Vec::new(),
            inherit: false,
            cwd: None,
            limit: Some(LIMIT),
        }
    }

    /// With one more word.
    #[must_use]
    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().to_os_string());
        self
    }

    /// With more words, each as it is: nothing splits or quotes them.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|arg| arg.as_ref().to_os_string()));
        self
    }

    /// With the variable `name` set to `value`; a later value of the same name
    /// wins.
    #[must_use]
    pub fn var(mut self, name: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.vars
            .push((name.as_ref().to_os_string(), value.as_ref().to_os_string()));
        self
    }

    /// With each of `vars` set, in order.
    #[must_use]
    pub fn vars<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        for (name, value) in vars {
            self = self.var(name, value);
        }
        self
    }

    /// With the test's own environment underneath its variables, instead of
    /// the variables alone.
    #[must_use]
    pub fn inheriting_env(mut self) -> Self {
        self.inherit = true;
        self
    }

    /// Run in the folder `dir`, instead of the test's own.
    #[must_use]
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    /// Ended after `limit` instead of the [`LIMIT`].
    #[must_use]
    pub fn limit(mut self, limit: Duration) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Never ended for running long: for a build, which takes what it takes.
    #[must_use]
    pub fn unlimited(mut self) -> Self {
        self.limit = None;
        self
    }

    /// Runs the program to its end, or to its limit, and keeps what it left.
    /// Only a program that did not start is an error: a code other than 0 is an
    /// answer, for the case to read.
    pub fn run(self) -> Result<Ran> {
        let program = self.program.to_string_lossy().into_owned();
        let mut command = Command::new(&self.program);
        if !self.inherit {
            command.env_clear().envs(required_on_windows(&self.vars));
        }
        command
            .envs(self.vars.iter().map(|(name, value)| (name, value)))
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = &self.cwd {
            command.current_dir(dir);
        }
        let mut child = command.spawn().map_err(|source| Error::Program {
            action: "start",
            program: program.clone(),
            source,
        })?;
        // Both pipes are read while the program runs: one that fills while the
        // other is waited on would stop it.
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        let ended = wait(&mut child, self.limit);
        let (stdout, stderr) = (collected(stdout), collected(stderr));
        let (code, timed_out) = ended.map_err(|source| Error::Program {
            action: "wait for",
            program,
            source,
        })?;
        Ok(Ran {
            code,
            stdout,
            stderr,
            timed_out,
        })
    }
}

/// What Windows needs of the test's own environment, for each variable of
/// [`REQUIRED_ON_WINDOWS`] that `vars` do not name (the names of Windows'
/// variables are not case-sensitive) and the test has. Nothing elsewhere.
fn required_on_windows(vars: &[(OsString, OsString)]) -> Vec<(&'static str, OsString)> {
    if !cfg!(windows) {
        return Vec::new();
    }
    REQUIRED_ON_WINDOWS
        .into_iter()
        .filter(|name| {
            !vars
                .iter()
                .any(|(given, _)| given.eq_ignore_ascii_case(name))
        })
        .filter_map(|name| std::env::var_os(name).map(|value| (name, value)))
        .collect()
}

/// Reads `pipe` to its end on a thread of its own.
fn drain(pipe: Option<impl Read + Send + 'static>) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            // A read that fails leaves what was read; the program's end says the rest.
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

/// What a [`drain`] read, as text.
fn collected(reader: JoinHandle<Vec<u8>>) -> String {
    String::from_utf8_lossy(&reader.join().unwrap_or_default()).into_owned()
}

/// Waits for `child` to end: the code it exited with (none when a signal ended
/// it) and whether it was ended here, for running past `limit`.
fn wait(child: &mut Child, limit: Option<Duration>) -> io::Result<(Option<i32>, bool)> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok((status.code(), false)),
            Ok(None) => {}
            Err(cause) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(cause);
            }
        }
        if limit.is_some_and(|limit| started.elapsed() >= limit) {
            // It may have ended just now, which is no matter to say.
            let _ = child.kill();
            let _ = child.wait();
            return Ok((None, true));
        }
        thread::sleep(POLL);
    }
}

/// What a program that ran left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// The code it exited with; none when a signal ended it or it was ended for
    /// running past its limit.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Whether it was ended for running past its limit.
    pub timed_out: bool,
}

impl Ran {
    /// What it printed and what it said, one after the other: for the cases
    /// that look in either.
    pub fn output(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }

    /// What it printed, read as JSON.
    pub fn json(&self) -> Result<serde_json::Value> {
        serde_json::from_str(&self.stdout).map_err(|source| Error::Json {
            text: self.stdout.clone(),
            source,
        })
    }
}

/// The whole run, for the message of an assertion that did not hold.
impl fmt::Display for Ran {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.code, self.timed_out) {
            (_, true) => write!(f, "ended for running past its limit")?,
            (Some(code), false) => write!(f, "exit code {code}")?,
            (None, false) => write!(f, "ended by a signal")?,
        }
        write!(f, "\nstdout:\n{}\nstderr:\n{}", self.stdout, self.stderr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_that_cannot_be_started_is_an_error_that_names_it() {
        let failed = Run::new("cf-e2e-no-such-program").run().unwrap_err();
        assert!(matches!(
            &failed,
            Error::Program { action: "start", program, .. } if program == "cf-e2e-no-such-program"
        ));
        assert!(
            failed
                .to_string()
                .starts_with("could not start `cf-e2e-no-such-program`: "),
            "{failed}"
        );
    }

    #[test]
    fn windows_is_given_what_it_needs_of_the_tests_where_the_case_names_none_and_others_nothing() {
        let names = |vars: &[(OsString, OsString)]| -> Vec<&'static str> {
            required_on_windows(vars)
                .into_iter()
                .map(|(name, _)| name)
                .collect()
        };
        if cfg!(windows) {
            // Every Windows process has a system folder.
            assert!(names(&[]).contains(&"SYSTEMROOT"));
            // What a case names is its own, in whichever case it writes the name.
            let named = [(OsString::from("SystemRoot"), OsString::from("C:\\x"))];
            assert!(!names(&named).contains(&"SYSTEMROOT"));
        } else {
            assert_eq!(names(&[]), Vec::<&str>::new());
        }
    }

    #[test]
    fn a_run_says_how_it_ended_and_what_it_printed_and_said() {
        let ran = |code, timed_out| Ran {
            code,
            stdout: "out".into(),
            stderr: "err".into(),
            timed_out,
        };
        assert_eq!(
            ran(Some(3), false).to_string(),
            "exit code 3\nstdout:\nout\nstderr:\nerr"
        );
        assert!(ran(None, false)
            .to_string()
            .starts_with("ended by a signal\n"));
        assert!(ran(None, true)
            .to_string()
            .starts_with("ended for running past its limit\n"));
        assert_eq!(ran(Some(0), false).output(), "outerr");
    }

    #[test]
    fn what_a_program_printed_as_json_is_read_and_what_is_not_is_an_error() {
        let said = |stdout: &str| Ran {
            code: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
            timed_out: false,
        };
        assert_eq!(said(r#"{"a":[1]}"#).json().unwrap()["a"][0], 1);
        let failed = said("not json").json().unwrap_err();
        assert!(matches!(failed, Error::Json { .. }), "{failed}");
        assert!(failed.to_string().ends_with("\nnot json"), "{failed}");
    }
}

/// The runs that need a shell to be told what to do.
#[cfg(test)]
#[cfg(unix)]
mod shell_tests {
    use super::*;

    fn sh(script: &str) -> Run {
        Run::new("/bin/sh").args(["-c", script])
    }

    #[test]
    fn what_a_program_printed_said_and_exited_with_is_kept() {
        let ran = sh("printf out; printf err >&2; exit 3").run().unwrap();
        assert_eq!(
            ran,
            Ran {
                code: Some(3),
                stdout: "out".into(),
                stderr: "err".into(),
                timed_out: false,
            }
        );
    }

    #[test]
    fn a_program_is_given_the_variables_it_is_told_and_none_of_the_tests() {
        let probe = r#"printf '%s|%s' "${CF_E2E_A-unset}" "${CARGO_MANIFEST_DIR-unset}""#;
        let alone = sh(probe).var("CF_E2E_A", "1").run().unwrap();
        assert_eq!(alone.stdout, "1|unset");
        // Cargo sets the manifest's folder for the tests it runs.
        let with_the_tests = sh(probe)
            .var("CF_E2E_A", "1")
            .inheriting_env()
            .run()
            .unwrap();
        assert_eq!(
            with_the_tests.stdout,
            format!("1|{}", env!("CARGO_MANIFEST_DIR"))
        );
    }

    #[test]
    fn a_variable_told_twice_has_the_later_value_and_a_told_one_beats_the_tests() {
        let ran = sh(r#"printf '%s|%s' "$A" "$CARGO_MANIFEST_DIR""#)
            .vars([("A", "1"), ("A", "2")])
            .var("CARGO_MANIFEST_DIR", "told")
            .inheriting_env()
            .run()
            .unwrap();
        assert_eq!(ran.stdout, "2|told");
    }

    #[test]
    fn a_program_runs_in_the_folder_it_is_given() {
        let folder = tempfile::tempdir().unwrap();
        let ran = sh("pwd").cwd(folder.path()).run().unwrap();
        let there = std::fs::canonicalize(folder.path()).unwrap();
        assert_eq!(ran.stdout.trim_end(), there.to_string_lossy());
    }

    #[test]
    fn a_program_that_reads_its_input_finds_it_closed() {
        let ran = sh("cat; printf done").run().unwrap();
        assert_eq!((ran.code, ran.stdout.as_str()), (Some(0), "done"));
    }

    #[test]
    fn output_larger_than_a_pipe_holds_does_not_stop_the_program() {
        // Both pipes fill many times over, a line at a time and alternately.
        let script = "i=0; while [ $i -lt 20000 ]; do printf 'xxxxxxxxxxxxxxx\\n'; \
                      printf 'yyyyyyyyyyyyyyy\\n' >&2; i=$((i+1)); done";
        let ran = sh(script).run().unwrap();
        assert_eq!(ran.code, Some(0));
        assert_eq!((ran.stdout.len(), ran.stderr.len()), (320_000, 320_000));
    }

    #[test]
    fn a_program_that_runs_past_its_limit_is_ended_and_says_so() {
        let started = Instant::now();
        let ran = sh("printf begun; exec sleep 30")
            .limit(Duration::from_millis(200))
            .run()
            .unwrap();
        assert_eq!((ran.code, ran.timed_out), (None, true));
        assert_eq!(ran.stdout, "begun");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_program_ended_by_a_signal_has_no_code() {
        let ran = sh("kill -9 $$").run().unwrap();
        assert_eq!((ran.code, ran.timed_out), (None, false));
    }
}
