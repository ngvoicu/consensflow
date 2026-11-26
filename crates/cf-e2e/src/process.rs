//! The one place cf-e2e starts a program. `clippy.toml` lets only cf-process do
//! that, and a black-box suite depends on no crate of the product, cf-process
//! included: it starts programs with std, runs each to its end (or to the limit
//! it is given) or leaves it running for the case to drive ([`Spawned`]), and
//! keeps what it printed, what it said and the code it exited with.
//!
//! A program gets the environment a case names and no other: the case's home
//! points `cf` at its own folders, and nothing of the machine's `~/.consensflow`
//! or of a window this test may itself run in reaches it. What a program is given of
//! the test's own environment is what the machine is ([`is_machine_variable`]: where programs
//! are, what the system is called, the temporary folders, the language), and nothing of the
//! account: every home variable is a folder of the run's, and the door's seal says so before
//! anything starts. A stand-in program in a window gives its children the window's own
//! environment ([`Run::as_this_window`]); no program gets the test's whole. Board variables are
//! never handed on implicitly. The program's input is closed, unless the case
//! gives it some ([`Run::input`]) or drives it ([`Run::spawn`]).
//!
//! On Windows the machine's variables are what the system needs to start a program: the
//! JavaScript suites started `cf` with Node, whose libuv does that for every child, and winsock
//! does not start without `SYSTEMROOT`, for one. This module reads the test's environment for
//! that, and for the few variables a stand-in program or a suite is told its settings by
//! ([`own_var`]).
//!
//! A program that leaves a child of its own holding its output after it has ended
//! (a wrapper's helper started in the background) does not hold the run: what it
//! wrote is read for a few seconds more ([`HELD`]), and the run says it was not read
//! to its end ([`Ran::held`]).
// The one place a program starts and the test's environment is read; see above.
#![allow(clippy::disallowed_methods)]

mod pid;
mod spawned;
#[cfg(test)]
mod tests;

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::{Error, Result};

#[cfg(unix)]
pub use pid::stall;
#[cfg(unix)]
pub use pid::{age, command, described, group_of};
pub use pid::{alive_among, end_all, is_alive, signal, started, Signal};
pub use spawned::{Sink, Spawned};

/// How long a program may run before it is ended. A verb of `cf` answers at
/// once, and one that waited for ever would hold the whole suite with it.
pub const LIMIT: Duration = Duration::from_secs(30);

/// How long a PowerShell the rig, its checks or its stand-ins start on Windows is given:
/// well past what one takes to start on a loaded machine. That was 11 to 17 s on GitHub's
/// Windows runner (2026-10-15), and past [`LIMIT`] under the load of a whole test program
/// (CI's run of the live suites' offline cases, 2026-11-03, T-1161): a PowerShell ended at
/// [`LIMIT`] said nothing of processes that ran, and a stand-in that waited no longer for
/// its own command's PowerShell took the command for failed and left its window.
pub const POWERSHELL_LIMIT: Duration = Duration::from_secs(120);

/// The case `case` of a test binary (`binary`: the test's own, `std::env::current_exe()`) run
/// again alone as a program by a test of it, held to `limit`: the most what the case runs may
/// take, which its caller says. A nested case is held to no verb of `cf`'s [`LIMIT`] by default:
/// a case that starts a rig may take longer than that, and one was ended at it on main's CI
/// (T-848); a rig case's own is [`crate::rig::nested::case_of`]. The words after are the
/// caller's, and so is the environment (none of the test's unless it asks).
pub fn case_of(binary: impl AsRef<OsStr>, case: &str, limit: Duration) -> Run {
    let run = Run::new(binary).args(["--exact", case]).limit(limit);
    if own_var("CF_E2E_STAND_IN").as_deref() == Some("1") {
        let temp = std::env::temp_dir();
        let home = own_var("HOME").unwrap_or_else(|| panic!("stand-in case has no HOME"));
        run.var("CF_E2E_STAND_IN", "1")
            .var("HOME", &home)
            .var("USERPROFILE", &home)
            .var("TMPDIR", &temp)
            .var("TMP", &temp)
            .var("TEMP", &temp)
    } else {
        run
    }
}

/// How often a program that has not ended is looked at again.
const POLL: Duration = Duration::from_millis(2);

/// How long a program's output is read for once it has ended: what it wrote is in
/// its pipes by then, and a process it left that holds them open (a helper a
/// wrapper started in the background) would hold the run for as long as it lives.
pub const HELD: Duration = Duration::from_secs(5);

/// The environment variable `name` of this process, or none when it is not set
/// (or is not text). A stand-in program reads what the window it stands in for
/// gave it, a suite its size, and a run the programs built for it before it
/// (`cf::BUILT`) or the id nextest gave it (`cf::RUN`), here and nowhere else.
pub fn own_var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// The first `name` in the folders of `path`, as a shell finds a command: on Windows with each
/// of the extensions `extensions` names (`PATHEXT`). A program started by its bare name on
/// Windows is looked for first in the folder of the program that starts it, which no shell
/// does: a stand-in's is cargo's `target\debug`, where another tree's build of `cf` may be
/// (zeewin2, T-1186: a3-1's `cf`, with T-1110's fix, ran for a tree without it).
pub fn first_on_path(name: &str, path: &str, extensions: &str) -> Option<std::path::PathBuf> {
    let extensions: Vec<&str> = if cfg!(windows) {
        extensions
            .split(';')
            .filter(|extension| !extension.is_empty())
            .collect()
    } else {
        vec![""]
    };
    std::env::split_paths(path).find_map(|folder| {
        extensions
            .iter()
            .map(|extension| folder.join(format!("{name}{extension}")))
            .find(|file| file.is_file())
    })
}

/// The program `name` as a shell started from this process's environment finds it: the first on
/// its `PATH` ([`first_on_path`]), or the bare name where none is. The stand-in agents run `cf`
/// so, as an agent's shell does (T-1186, T-1207).
pub fn as_a_shell_finds(name: &str) -> std::path::PathBuf {
    let path = own_var("PATH").unwrap_or_default();
    let extensions = own_var("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_owned());
    first_on_path(name, &path, &extensions).unwrap_or_else(|| name.into())
}

/// Git Bash, where Claude Code looks for it: the file `CLAUDE_CODE_GIT_BASH_PATH` names,
/// then the `bin\bash.exe` of the Git whose `git.exe` is on the PATH (`…\Git\cmd` or
/// `…\Git\bin`), then Git's own install folder. The shell Claude Code runs an agent's commands
/// in on Windows, and Devin too (its commands answer `/usr/bin/bash: line 1: …` on zeewin,
/// `devin-shell-on-windows-does-not-find-cf`): a stand-in that runs a command for its agent
/// runs it so, for the host proves the work of a turn by a shell that started it, and `cmd.exe`
/// is none (`cf-panes`' `background::work`).
#[cfg(windows)]
pub fn git_bash() -> Option<PathBuf> {
    git_bash_in(
        own_var("CLAUDE_CODE_GIT_BASH_PATH").as_deref(),
        &own_var("PATH").unwrap_or_default(),
    )
}

/// [`git_bash`], with the file `named` and the `path` it looks in.
#[cfg(windows)]
fn git_bash_in(named: Option<&str>, path: &str) -> Option<PathBuf> {
    if let Some(named) = named.map(PathBuf::from) {
        if named.is_file() {
            return Some(named);
        }
    }
    std::env::split_paths(path)
        .filter(|folder| folder.join("git.exe").is_file())
        .filter_map(|folder| folder.parent().map(|git| git.join("bin").join("bash.exe")))
        .chain([PathBuf::from(r"C:\Program Files\Git\bin\bash.exe")])
        .find(|bash| bash.is_file())
}

/// What a program is given to read.
#[derive(Debug, Clone)]
enum Input {
    /// What the way it is started does: nothing, for [`Run::run`]; a pipe the
    /// case writes to, for [`Run::spawn`].
    Default,
    /// Nothing: the program finds the end of its input at once.
    Closed,
    /// These bytes first. [`Run::run`] then ends the input; [`Run::spawn`]
    /// leaves it open.
    Bytes(Vec<u8>),
}

/// One word of a program's command line.
#[derive(Debug, Clone)]
enum Arg {
    /// A word, quoted for the program as the platform quotes one.
    Plain(OsString),
    /// A word written as it is, which `cmd.exe` reads by rules of its own.
    #[cfg(windows)]
    Raw(OsString),
}

/// A program to run: which, with what words, in which folder, and with what
/// environment.
#[derive(Debug, Clone)]
pub struct Run {
    program: OsString,
    args: Vec<Arg>,
    vars: Vec<(OsString, OsString)>,
    removed: Vec<OsString>,
    /// The verdict of a stand-in's window on its own environment, where the program is a child
    /// of the stand-in that gives it that environment ([`Run::as_this_window`]).
    window: Option<std::result::Result<(), String>>,
    cwd: Option<PathBuf>,
    limit: Option<Duration>,
    input: Input,
    own_group: bool,
    /// The bundle the release ships, where the run has leave to start it ([`Run::shipped`]).
    shipped: Option<PathBuf>,
    /// Which door it is started by: a plain start, a harness's, or one of the door's own
    /// operations, whose program and words are fixed where it is made.
    kind: Kind,
}

/// Which door a program is started by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    /// [`Run::new`]: any program but a harness's and one that needs a door of its own.
    Plain,
    /// [`Run::harness`]: a harness's program, on the run's homes, with its update switches.
    Harness(&'static str),
    /// A door operation (cargo, the Keychain's utility): its environment is the operation's own,
    /// named where it is made, and the seal's homes are not asked of it.
    Operation(&'static str),
    /// [`Run::fixture_script`]: an offline fixture's own script, run by the shell.
    Script,
}

/// Whether the variable `name` says what the machine is, and nothing of whose account runs on
/// it: where programs are, what the system is called, the temporary folders, the language.
/// (Names are not case-sensitive on Windows.)
pub fn is_machine_variable(name: &str) -> bool {
    const MACHINE: [&str; 34] = [
        "PATH",
        "SYSTEMROOT",
        "SYSTEMDRIVE",
        "WINDIR",
        "COMSPEC",
        "PATHEXT",
        "TEMP",
        "TMP",
        "TMPDIR",
        "PROGRAMDATA",
        "PROGRAMFILES",
        "PROGRAMFILES(X86)",
        "PROGRAMW6432",
        "COMMONPROGRAMFILES",
        "COMMONPROGRAMFILES(X86)",
        "COMMONPROGRAMW6432",
        "ALLUSERSPROFILE",
        "PUBLIC",
        "COMPUTERNAME",
        "NUMBER_OF_PROCESSORS",
        "PROCESSOR_ARCHITECTURE",
        "OS",
        "USERNAME",
        "USER",
        "LOGNAME",
        "LANG",
        "TERM",
        "SHELL",
        "LOGONSERVER",
        "USERDOMAIN",
        // What a real harness on Windows reads of the machine, and a window no longer has from
        // the account's registry (T-2119 B1-c): the processor's own words, and where PowerShell
        // finds its modules.
        "PROCESSOR_IDENTIFIER",
        "PROCESSOR_LEVEL",
        "PROCESSOR_REVISION",
        "PSMODULEPATH",
    ];
    let upper = name.to_ascii_uppercase();
    MACHINE.contains(&upper.as_str()) || upper == "LC_ALL" || upper == "LC_CTYPE"
}

/// The machine's variables among `all` ([`is_machine_variable`]).
pub fn machine_vars(
    all: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    all.into_iter()
        .filter(|(name, _)| name.to_str().is_some_and(is_machine_variable))
        .collect()
}

fn board_variable(name: &OsStr) -> bool {
    name.to_string_lossy()
        .to_ascii_uppercase()
        .starts_with("CONSENSFLOW_")
}

impl Run {
    /// With leave to start the bundle the release ships, which no case starts otherwise: a
    /// [`Shipped`](crate::shipped::Shipped), made only on GitHub's runner where its workflow
    /// asks and no live app runs.
    #[must_use]
    pub fn shipped(mut self, leave: &crate::shipped::Shipped) -> Self {
        self.shipped = Some(leave.app().to_path_buf());
        self
    }

    /// `program`, with no words, none of the test's environment and the
    /// [`LIMIT`]. A path is started as it is; a bare name is left to the
    /// system to find, which the cases here do not rely on.
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().to_os_string(),
            args: Vec::new(),
            vars: Vec::new(),
            removed: Vec::new(),
            window: None,
            cwd: None,
            limit: Some(LIMIT),
            input: Input::Default,
            own_group: false,
            shipped: None,
            kind: Kind::Plain,
        }
    }

    /// `harness`'s program `program`, started as a harness is: on the run's homes, with the
    /// switches that keep it from updating or installing anything itself
    /// ([`crate::door::spawn::update_switches`]). The program must be the harness's, by the file
    /// it resolves to ([`crate::door::spawn::program_of`]).
    pub fn harness(harness: &'static str, program: impl AsRef<OsStr>) -> Self {
        let mut run = Self::new(program);
        for (name, value) in crate::door::spawn::update_switches(harness) {
            run = run.var(name, value);
        }
        run.kind = Kind::Harness(harness);
        run
    }

    /// A script of the kit's own or a fixture's, run by `shell` with `flags` before it
    /// (`/bin/sh -c`, `powershell -NoProfile -Command`, `cmd /c`): the one start through a shell
    /// whose command the door does not read. A script with a word that names a harness's program,
    /// cargo or the Keychain's utility is refused here; the audit holds this function to the files
    /// that call it ([`crate::door::rule`]).
    pub fn script_in(shell: impl AsRef<OsStr>, flags: &[&str], script: impl AsRef<str>) -> Self {
        let mut run = Self::new(shell);
        for flag in flags {
            run.args.push(Arg::Plain(OsString::from(flag)));
        }
        run.args.push(Arg::Plain(OsString::from(script.as_ref())));
        run.kind = Kind::Script;
        run
    }

    /// A command line a stand-in runs by `/bin/sh -c` as its harness would (a hook's): where it
    /// can be read as one program with words it is judged by that program, as every start is
    /// (the product's own `cf hook claude` is a start of `cf`), and where it cannot, it is a
    /// fixture's script ([`Run::fixture_script`]). The audit holds this to its callers too.
    pub fn shell_line(command: impl AsRef<str>) -> Self {
        let words = ["-c".to_owned(), command.as_ref().to_owned()];
        match crate::door::shells::told(crate::door::shells::Shell::Posix, &words) {
            Ok(_) => Self::new("/bin/sh").args(words),
            Err(_) => Self::fixture_script(command),
        }
    }

    /// An offline fixture's own script, run by `/bin/sh -c`: the one start through a shell whose
    /// command the door does not read (the door refuses any other, where it cannot read the
    /// command, T-2219). A script with a word that names a harness's program, cargo or the
    /// Keychain's utility is refused here too. The audit holds this function to the files that
    /// call it ([`crate::door::rule`]): a new caller fails it.
    pub fn fixture_script(script: impl AsRef<str>) -> Self {
        Self::script_in("/bin/sh", &["-c"], script)
    }

    /// One of the door's own operations (`name`), on `program` with the environment it names: a
    /// door module makes it, and nothing else does.
    pub(crate) fn operation(name: &'static str, program: impl AsRef<OsStr>) -> Self {
        let mut run = Self::new(program);
        run.kind = Kind::Operation(name);
        run
    }

    /// With one more word.
    #[must_use]
    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(Arg::Plain(arg.as_ref().to_os_string()));
        self
    }

    /// With more words, each as it is: nothing splits or quotes them.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args.extend(
            args.into_iter()
                .map(|arg| Arg::Plain(arg.as_ref().to_os_string())),
        );
        self
    }

    /// With one more word, written into the command line as it is. For
    /// `cmd.exe /c`, which reads a line by rules of its own and takes the
    /// quotes Windows' usual quoting of a word would put in a command as its
    /// own.
    #[cfg(windows)]
    #[must_use]
    pub fn raw_arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(Arg::Raw(arg.as_ref().to_os_string()));
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

    /// Without the variable `name`, whichever way the program got it: told, or
    /// the window's own under [`Run::as_this_window`].
    #[must_use]
    pub fn without(mut self, name: impl AsRef<OsStr>) -> Self {
        self.removed.push(name.as_ref().to_os_string());
        self
    }

    /// A child of a stand-in program, on the environment the window it runs in gave the
    /// stand-in: the window's own board connection and homes, as a real harness's children have
    /// them, and no other. `window` is the stand-in's own verdict on that environment
    /// ([`crate::door::spawn::Window::this`]): where it breaks the seal, nothing starts. For the
    /// stand-in programs in `src/bin` alone (the audit finds it elsewhere).
    #[must_use]
    pub fn as_this_window(mut self, window: &crate::door::spawn::Window) -> Self {
        self.window = Some(window.verdict());
        self.vars
            .extend(std::env::vars_os().filter(|(name, _)| board_variable(name)));
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

    /// Given `bytes` to read: all of them, and then (for [`Run::run`]) the end
    /// of its input. A [`Run::spawn`]ed program is left its input open after.
    #[must_use]
    pub fn input(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.input = Input::Bytes(bytes.into());
        self
    }

    /// Given no input at all, not even a pipe to write to: what a [`Run::run`]
    /// is given anyway, and a [`Run::spawn`]ed program is not unless told.
    #[must_use]
    pub fn closed_input(mut self) -> Self {
        self.input = Input::Closed;
        self
    }

    /// In a process group of its own, as Claude Code runs a command it is asked to
    /// (Unix's; Windows has no groups, and starts it as ever).
    #[must_use]
    pub fn own_group(mut self) -> Self {
        self.own_group = true;
        self
    }

    /// Refuses a program that is an app a test may not start, or reaches one by its words,
    /// before anything starts.
    fn startable(&self) -> Result<()> {
        let words: Vec<&OsStr> = self
            .args
            .iter()
            .map(|arg| match arg {
                Arg::Plain(word) => word.as_os_str(),
                #[cfg(windows)]
                Arg::Raw(word) => word.as_os_str(),
            })
            .collect();
        let program = std::path::Path::new(&self.program);
        let shipped = self.shipped.as_deref().is_some_and(|app| {
            cf_base::bundle::bundle_of(program).is_some_and(|bundle| bundle == app)
        });
        if shipped {
            return cf_base::bundle::words_startable_by_a_test(program, &words)
                .map_err(Error::LiveApp);
        }
        cf_base::bundle::command_startable_by_a_test(program, &words).map_err(Error::LiveApp)
    }

    /// The whole environment the program will have, sealed ([`crate::door::spawn::seal`]): what
    /// its case names over what it is given of the test's (nothing, the machine's, or all of it
    /// but the board's), the door's own variables last, and every home variable it does not name
    /// below the home it names, or below a scratch home of the run's where it names none. An
    /// error where the seal refuses it: nothing starts.
    pub fn environment(&self) -> Result<Vec<(OsString, OsString)>> {
        let windows = cfg!(windows);
        if let Some(Err(why)) = &self.window {
            return Err(Error::LiveApp(format!(
                "{} is not started: the window it runs in broke the seal: {why}",
                self.program.to_string_lossy()
            )));
        }
        let mut env: Vec<(OsString, OsString)> = if self.window.is_some() {
            // The window's own environment, but what of it is the account's: a home that is no
            // folder of the run's, and any other variable but the search path that names a
            // store, is not handed on (the door gives the run's homes below). A case that names
            // such a variable itself is refused by the seal instead.
            let roots = crate::door::Roots::now();
            std::env::vars_os()
                .filter(|(name, _)| !board_variable(name))
                .filter(|(name, value)| inheritable(name, value, &roots))
                .collect()
        } else {
            machine_vars(std::env::vars_os())
        };
        for (name, value) in &self.vars {
            set(&mut env, name, value, windows);
        }
        for name in &self.removed {
            env.retain(|(held, _)| !same_name(held, name, windows));
        }
        // The profile's two halves are said by the door from the profile the program is given,
        // unless its case names them: the machine's own would say the caller's.
        without_unnamed_profile_halves(&mut env, &self.vars);
        // A program of a say-only run says only, into the same file, whatever its environment;
        // every program is told the folders the run made so far, the only ones it writes below,
        // and the roots it never writes in. None of them is a caller's to give.
        let kit = crate::door::Door::child_vars();
        for owned in [
            crate::door::MADE_ROOTS,
            crate::door::account::PROTECTED_FILE,
            crate::door::account::PROTECTED_DIGEST,
        ] {
            env.retain(|(held, _)| !same_name(held, OsStr::new(owned), windows));
        }
        for (name, value) in &kit {
            set(&mut env, OsStr::new(name), OsStr::new(value), windows);
        }
        if let Kind::Operation(_) = self.kind {
            return Ok(env);
        }
        let named_home = ["HOME", "USERPROFILE"].into_iter().find_map(|wanted| {
            env.iter()
                .find(|(name, _)| same_name(name, OsStr::new(wanted), windows))
                .map(|(_, value)| PathBuf::from(value))
                .filter(|home| home.is_absolute() && crate::door::is_made_or_below(home))
        });
        let base = match named_home {
            Some(home) => home,
            None => scratch_home()?,
        };
        let missing = crate::door::spawn::missing_homes(&env, &base);
        env.extend(missing);
        crate::door::sealed(env, &kit, windows).map_err(|why| {
            Error::LiveApp(format!(
                "{} is not started: {why}",
                self.program.to_string_lossy()
            ))
        })
    }

    /// What the program is, by the file it resolves to on the search path `env` gives it, and
    /// whether its door may start it: a plain start refuses a harness's program and one that needs
    /// a door of its own; a harness's start refuses any other.
    fn admitted(&self, env: &[(OsString, OsString)]) -> Result<()> {
        if let Kind::Operation(_) = self.kind {
            return Ok(());
        }
        if let Kind::Script = self.kind {
            // A script is run by a shell: any other program is a start with no judgement of its
            // program, which is what a script entry is not.
            if crate::door::shells::shell_named(&self.program.to_string_lossy()).is_none() {
                return Err(Error::LiveApp(format!(
                    "{} is not started: a script is run by a shell, and this is none the door knows",
                    self.program.to_string_lossy()
                )));
            }
            let words: Vec<String> = self
                .args
                .iter()
                .map(|arg| match arg {
                    Arg::Plain(word) => word.to_string_lossy().into_owned(),
                    #[cfg(windows)]
                    Arg::Raw(word) => word.to_string_lossy().into_owned(),
                })
                .collect();
            return match crate::door::shells::names_a_door_program(&words) {
                Some(name) => Err(Error::LiveApp(format!(
                    "a fixture's script names {name}, which has a door of its own"
                ))),
                None => Ok(()),
            };
        }
        let args: Vec<String> = self
            .args
            .iter()
            .map(|arg| match arg {
                Arg::Plain(word) => word.to_string_lossy().into_owned(),
                #[cfg(windows)]
                Arg::Raw(word) => word.to_string_lossy().into_owned(),
            })
            .collect();
        let program =
            crate::door::program_started(Path::new(&self.program), &args, env).map_err(|why| {
                Error::LiveApp(format!(
                    "{} is not started: {why}",
                    self.program.to_string_lossy()
                ))
            })?;
        match (&self.kind, &program) {
            (Kind::Plain, crate::door::spawn::Program::Plain) => Ok(()),
            (Kind::Harness(_), crate::door::spawn::Program::Harness(_)) => {
                crate::door::spawn::has_update_switches(&program, env).map_err(Error::LiveApp)
            }
            // A file of the run's own that starts the harness (a rig's wrapper, a stand-in's copy
            // of it): started as the harness is, with its switches.
            (Kind::Harness(harness), crate::door::spawn::Program::Plain)
                if crate::door::is_made_or_below(Path::new(&self.program)) =>
            {
                crate::door::spawn::has_update_switches(
                    &crate::door::spawn::Program::Harness(harness),
                    env,
                )
                .map_err(Error::LiveApp)
            }
            (Kind::Harness(harness), _) => Err(Error::LiveApp(format!(
                "{} is no program of {harness}'s, and is not started as one",
                self.program.to_string_lossy()
            ))),
            (_, other) => Err(Error::LiveApp(format!(
                "{} is {}, which a plain start does not start: it has a door of its own",
                self.program.to_string_lossy(),
                match other {
                    crate::door::spawn::Program::Harness(harness) => format!("{harness}'s program"),
                    crate::door::spawn::Program::Keychain => "the Keychain's utility".to_owned(),
                    crate::door::spawn::Program::Cargo => "cargo".to_owned(),
                    crate::door::spawn::Program::Plain => String::new(),
                }
            ))),
        }
    }

    /// The program's command, with everything but its streams, on its sealed environment.
    fn command(&self) -> Result<Command> {
        self.command_on().map(|(command, _)| command)
    }

    /// [`Run::command`], and the sealed environment it was given.
    fn command_on(&self) -> Result<(Command, Vec<(OsString, OsString)>)> {
        let env = self.environment()?;
        self.admitted(&env)?;
        let mut command = Command::new(&self.program);
        // Daemons, hosts and stand-ins run for a whole case while other cases open sockets and
        // pipes: one left another's would hold it to its end.
        cf_process::hold_only_the_streams(&mut command);
        command
            .env_clear()
            .envs(env.iter().map(|(name, value)| (name, value)));
        for arg in &self.args {
            arg.add_to(&mut command);
        }
        if let Some(dir) = &self.cwd {
            command.current_dir(dir);
        }
        if self.own_group {
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                command.process_group(0);
            }
        }
        Ok((command, env))
    }

    /// Runs the program to its end, or to its limit, and keeps what it left.
    /// Only a program that did not start is an error: a code other than 0 is an
    /// answer, for the case to read.
    pub fn run(self) -> Result<Ran> {
        self.startable()?;
        let program = self.program.to_string_lossy().into_owned();
        let mut command = self.command()?;
        command
            .stdin(match self.input {
                Input::Bytes(_) => Stdio::piped(),
                Input::Default | Input::Closed => Stdio::null(),
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|source| Error::Program {
            action: "start",
            program: program.clone(),
            source,
        })?;
        // The input goes in on a thread of its own, and the pipes are read
        // while the program runs: one that fills while the other is waited on
        // would stop it.
        let feed = match self.input {
            Input::Bytes(bytes) => Some(feed(child.stdin.take(), bytes)),
            Input::Default | Input::Closed => None,
        };
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        let ended = wait(&mut child, self.limit);
        let by = Instant::now() + HELD;
        let ((stdout, out_held), (stderr, err_held)) =
            (collected(stdout, by), collected(stderr, by));
        if let Some(feed) = feed {
            // A program that ended without reading it all has nothing to say of it.
            let _ = feed.join();
        }
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
            held: out_held || err_held,
        })
    }

    /// Starts the program and leaves it running, for the case to drive: its
    /// streams are pipes ([`Spawned`]), its input one too unless it was told to
    /// be closed. Ending it is the case's, or its owner's drop.
    pub fn spawn(self) -> Result<Spawned> {
        self.spawn_on().map(|(spawned, _)| spawned)
    }

    /// Starts the program on a pty of the kit's own, of `rows` and `cols`, sealed as every start is
    /// ([`Run::environment`]) and admitted by the file it resolves to ([`Run::admitted`]): Muse's
    /// direct window, and no other program of the kit's runs on a pty of its own
    /// ([`crate::door::terminal`]).
    pub fn terminal(self, size: (u16, u16)) -> Result<crate::door::terminal::Terminal> {
        self.startable()?;
        let env = self.environment()?;
        self.admitted(&env)?;
        let args: Vec<OsString> = self
            .args
            .iter()
            .map(|arg| match arg {
                Arg::Plain(word) => word.clone(),
                #[cfg(windows)]
                Arg::Raw(word) => word.clone(),
            })
            .collect();
        crate::door::terminal::open(&self.program, &args, &env, self.cwd.as_deref(), size)
    }

    /// Starts the program as this program's one child on this program's own console and
    /// standard handles, sealed as every start is, and hands it over: for a stand-in that runs the
    /// harness it stands for as its child (Windows' Muse stand-in, which has no `exec`). Waiting
    /// for it is the caller's.
    pub fn relay(self) -> Result<Child> {
        self.startable()?;
        let program = self.program.to_string_lossy().into_owned();
        let (mut command, _) = self.command_on()?;
        command.spawn().map_err(|source| Error::Program {
            action: "start",
            program,
            source,
        })
    }

    /// [`Run::spawn`], and the whole environment the program was given, sealed: what a pane
    /// host's windows are opened over ([`crate::door::window_admitted`]).
    pub fn spawn_on(self) -> Result<(Spawned, Vec<(OsString, OsString)>)> {
        self.startable()?;
        let program = self.program.to_string_lossy().into_owned();
        let (mut command, env) = self.command_on()?;
        command
            .stdin(match self.input {
                Input::Closed => Stdio::null(),
                Input::Default | Input::Bytes(_) => Stdio::piped(),
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = command.spawn().map_err(|source| Error::Program {
            action: "start",
            program: program.clone(),
            source,
        })?;
        let first = match self.input {
            Input::Bytes(bytes) => Some(bytes),
            Input::Default | Input::Closed => None,
        };
        Ok((Spawned::new(child, program, first), env))
    }
}

impl Arg {
    fn add_to(&self, command: &mut Command) {
        match self {
            Self::Plain(word) => {
                command.arg(word);
            }
            #[cfg(windows)]
            Self::Raw(word) => {
                use std::os::windows::process::CommandExt;
                command.raw_arg(word);
            }
        }
    }
}

/// Whether the window's own variable `name` is handed on to a stand-in's child
/// ([`Run::as_this_window`]): not a home of the account's (one that is no folder this
/// run made), and not another variable but the search path that names a store.
fn inheritable(name: &OsStr, value: &OsStr, roots: &crate::door::Roots) -> bool {
    let text = name.to_string_lossy().to_ascii_uppercase();
    if text == "PATH" {
        return true;
    }
    let home = crate::door::spawn::HOME_VARS
        .iter()
        .chain(crate::door::spawn::OPTIONAL_HOME_VARS.iter())
        .any(|home| *home == text);
    if home {
        let path = Path::new(value);
        return path.is_absolute() && crate::door::is_made_or_below(path) && !roots.holds(path);
    }
    if crate::door::spawn::PROFILE_HALVES.contains(&text.as_str()) {
        // Said again by the door from the profile the program is given.
        return false;
    }
    let whole = std::iter::once(PathBuf::from(value));
    !whole
        .chain(std::env::split_paths(value))
        .filter(|path| path.is_absolute())
        .any(|path| roots.holds(&path))
}

/// The profile's two halves (`HOMEDRIVE`, `HOMEPATH`) taken out of `env` unless the program's case
/// (`named`) gives them: the door says them again from the profile the program is given, and the
/// machine's own would say the caller's.
fn without_unnamed_profile_halves(
    env: &mut Vec<(OsString, OsString)>,
    named: &[(OsString, OsString)],
) {
    let halves_named = named.iter().any(|(name, _)| {
        crate::door::spawn::PROFILE_HALVES
            .iter()
            .any(|half| same_name(name, OsStr::new(half), true))
    });
    if !halves_named {
        env.retain(|(held, _)| {
            !crate::door::spawn::PROFILE_HALVES
                .iter()
                .any(|half| same_name(held, OsStr::new(half), true))
        });
    }
}

/// Whether two names of variables are one: in any case on Windows.
fn same_name(left: &OsStr, right: &OsStr, windows: bool) -> bool {
    if windows {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

/// Sets `name` to `value` in `env`, over what it held of that name (in any case on Windows).
fn set(env: &mut Vec<(OsString, OsString)>, name: &OsStr, value: &OsStr, windows: bool) {
    env.retain(|(held, _)| !same_name(held, name, windows));
    env.push((name.to_os_string(), value.to_os_string()));
}

/// A home of the run's own, for a program whose case names none: made once for this process,
/// below a folder of the run's, and taken with the run's other folders of the door's at its end.
pub(crate) fn scratch_home() -> Result<PathBuf> {
    static HOME: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    if let Some(home) = HOME.get() {
        return Ok(home.clone());
    }
    let root = crate::door::made("cf-home-")
        .map_err(Error::file("make a folder in", &std::env::temp_dir()))?
        .hand_on();
    let home = root.join("home");
    crate::door::make_dir(&home)?;
    crate::door::account::remove_at_exit(&root);
    Ok(HOME.get_or_init(|| home).clone())
}

/// Writes `bytes` to `input` on a thread of its own, and closes it.
fn feed(input: Option<impl Write + Send + 'static>, bytes: Vec<u8>) -> JoinHandle<()> {
    thread::spawn(move || {
        if let Some(mut input) = input {
            // A program that stops reading ends the write; its end says the rest.
            let _ = input.write_all(&bytes);
        }
    })
}

/// A pipe read to its end on a thread of its own, what it read so far kept where
/// the run can take it whether the thread has ended or not.
struct Drain {
    read: Arc<Mutex<Vec<u8>>>,
    thread: JoinHandle<()>,
}

/// Reads `pipe` to its end on a thread of its own.
fn drain(pipe: Option<impl Read + Send + 'static>) -> Drain {
    let read = Arc::new(Mutex::new(Vec::new()));
    let into = Arc::clone(&read);
    let thread = thread::spawn(move || {
        let Some(mut pipe) = pipe else {
            return;
        };
        let mut chunk = [0_u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) => return,
                Ok(count) => into
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                // A read that fails leaves what was read; the program's end says the rest.
                Err(_) => return,
            }
        }
    });
    Drain { read, thread }
}

/// Waits for `thread` to end, until `by` at most: whether it did. One that ended
/// is joined; one that did not is left to end of itself.
fn ended_by(thread: JoinHandle<()>, by: Instant) -> bool {
    while !thread.is_finished() && Instant::now() < by {
        thread::sleep(POLL);
    }
    let ended = thread.is_finished();
    if ended {
        let _ = thread.join();
    }
    ended
}

/// What a [`drain`] read, as text, waited for until `by` at most; and whether its
/// pipe was still held open then (by a process the program left), so that what came
/// after is not read. The thread is left to end when the pipe does.
fn collected(drain: Drain, by: Instant) -> (String, bool) {
    let held = !ended_by(drain.thread, by);
    let bytes = drain
        .read
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    (String::from_utf8_lossy(&bytes).into_owned(), held)
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
                if crate::door::Door::of_run()
                    .end_child("a program that could not be waited for", child)
                {
                    let _ = child.wait();
                }
                return Err(cause);
            }
        }
        if limit.is_some_and(|limit| started.elapsed() >= limit) {
            // It may have ended just now, which is no matter to say.
            if crate::door::Door::of_run().end_child("a program past its limit", child) {
                let _ = child.wait();
            }
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
    /// Whether a process it left still held its output open [`HELD`] after it had
    /// ended, so that what was written after is not read.
    pub held: bool,
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
        if self.held {
            write!(
                f,
                "; a process it left held its output open, which was not read to its end"
            )?;
        }
        write!(f, "\nstdout:\n{}\nstderr:\n{}", self.stdout, self.stderr)
    }
}
