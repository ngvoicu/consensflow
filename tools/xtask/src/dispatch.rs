//! The command line: which command the words name, what it is handed, and how
//! the answer reaches the caller.
//!
//! A command is a row in the table of the module that owns it
//! (`sidecar::COMMANDS`, `check::COMMANDS`, ...), and [`commands`] joins the
//! tables. A row runs in Rust ([`Run::Native`]): the scripts the commands
//! replaced handed their words and their exit status to Node, and the last of
//! them is gone. The arguments after a command's words go to it as they are,
//! `--` and words with spaces in them included; only a first one that is
//! `--help` (or `-h`) is xtask's.

use std::ffi::{OsStr, OsString};
use std::io::{self, ErrorKind, Write};
use std::path::Path;

use cf_base::env::Env;

use crate::context::{self, Context};
use crate::process;
use crate::{
    app, bench, candidate, check, clippy_windows, departures, evals, live, plants, portable,
    scrub_transcript, sidecar, smoke, specmint, suites, windows,
};

/// A command of xtask.
#[derive(Debug)]
pub struct Command {
    /// The words that name it: `["stage"]`, `["app", "test"]`.
    pub words: &'static [&'static str],
    /// What it does, in a line, for the list of commands.
    pub about: &'static str,
    /// What follows its words, for its own help.
    pub usage: &'static str,
    pub run: Run,
}

/// How a command runs.
#[derive(Debug)]
pub enum Run {
    /// In Rust: given the arguments after its words, and the two streams, it
    /// answers the exit status.
    Native(fn(&Context, &[OsString], &mut Console) -> Result<i32, Failure>),
}

/// The two streams a command speaks on: its result on `out`, how it is getting
/// on on `err`. A program it runs writes to the terminal's own.
pub struct Console<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
}

/// Why a command did not finish.
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    /// The words are not ones the command takes: the status is 2.
    #[error("{0}")]
    Usage(String),
    #[error(transparent)]
    Context(#[from] context::Error),
    /// A program it had to run could not be started.
    #[error(transparent)]
    Process(#[from] process::Failure),
    /// A step of the build, the staging or the console host could not be done.
    #[error(transparent)]
    Sidecar(#[from] sidecar::Error),
    /// The portable exe could not be packed, or a file could not be read as one.
    #[error("portable: {0}")]
    Portable(#[from] cf_portable::Error),
    /// The sources have no one version to name a build by.
    #[error(transparent)]
    Version(#[from] cf_release::version::VersionError),
    /// The plants could not be read, planted or put back.
    #[error(transparent)]
    Plants(#[from] plants::Error),
    /// The vendored Specmint skills could not be refreshed.
    #[error(transparent)]
    Specmint(#[from] specmint::Error),
    /// What it had to say could not be written.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Every command, in the order `--help` lists them.
pub fn commands() -> Vec<&'static Command> {
    [
        sidecar::COMMANDS,
        portable::COMMANDS,
        app::COMMANDS,
        clippy_windows::COMMANDS,
        suites::COMMANDS,
        live::COMMANDS,
        windows::COMMANDS,
        evals::COMMANDS,
        scrub_transcript::COMMANDS,
        specmint::COMMANDS,
        departures::COMMANDS,
        bench::COMMANDS,
        plants::COMMANDS,
        smoke::COMMANDS,
        candidate::COMMANDS,
        check::COMMANDS,
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Runs the command line `args` (without the program's name) and answers the
/// exit status: the command's own, or 1 when it could not be run, or 2 when the
/// words were not ones it takes.
pub fn run(env: &Env, args: &[OsString], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut console = Console { out, err };
    let result = Context::new(env)
        .map_err(Failure::from)
        .and_then(|context| dispatch(&context, args, &mut console));
    let failure = match result {
        Ok(status) => return status,
        Err(failure) => failure,
    };
    // `cargo xtask --help | head -0` closes the pipe: nobody is left to tell.
    if matches!(&failure, Failure::Io(cause) if cause.kind() == ErrorKind::BrokenPipe) {
        return 0;
    }
    let _ = writeln!(console.err, "xtask: {failure}");
    if matches!(failure, Failure::Usage(_)) {
        let _ = writeln!(console.err, "see `cargo xtask --help`");
        return 2;
    }
    1
}

fn dispatch(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    match parse(&commands(), args, &context.root) {
        Parsed::Help(text) => {
            write!(console.out, "{text}")?;
            Ok(0)
        }
        Parsed::Usage(message) => Err(Failure::Usage(message)),
        Parsed::Run { command, args } => {
            let Run::Native(run) = &command.run;
            run(context, args, console)
        }
    }
}

/// What a command line asks for.
#[derive(Debug)]
pub enum Parsed<'a> {
    /// A command to run, with the arguments after its words.
    Run {
        command: &'a Command,
        args: &'a [OsString],
    },
    /// Help, as text, which asks for nothing to run.
    Help(String),
    /// Words that name no command, and what is wrong with them.
    Usage(String),
}

/// Reads the command line `args` against `commands`; `root` is the checkout
/// the help says it works on.
pub fn parse<'a>(commands: &[&'a Command], args: &'a [OsString], root: &Path) -> Parsed<'a> {
    let Some(first) = args.first() else {
        return Parsed::Usage("a command is required".into());
    };
    if is_help(first) {
        return Parsed::Help(overview(commands, root));
    }
    if let Some(command) = commands
        .iter()
        .copied()
        .find(|command| named_by(command, args))
    {
        let rest = &args[command.words.len()..];
        return match rest.first() {
            Some(word) if is_help(word) => Parsed::Help(command_help(command)),
            _ => Parsed::Run {
                command,
                args: rest,
            },
        };
    }
    // Not a command, but maybe the start of some (`app`, `test`, `bench`).
    let group: Vec<_> = commands
        .iter()
        .copied()
        .filter(|command| command.words.len() > 1 && first == command.words[0])
        .collect();
    let first = first.to_string_lossy();
    if group.is_empty() {
        return Parsed::Usage(format!("unknown command: {first}"));
    }
    match args.get(1) {
        Some(word) if is_help(word) => Parsed::Help(group_help(&first, &group)),
        Some(word) => Parsed::Usage(format!(
            "unknown command: {first} {}",
            word.to_string_lossy()
        )),
        None => Parsed::Usage(format!("{first} takes a command: {}", subcommands(&group))),
    }
}

fn is_help(word: &OsStr) -> bool {
    word == "--help" || word == "-h"
}

/// Whether `args` begin with all the words of `command`.
fn named_by(command: &Command, args: &[OsString]) -> bool {
    command.words.len() <= args.len()
        && command
            .words
            .iter()
            .zip(args)
            .all(|(word, arg)| arg == word)
}

fn subcommands(group: &[&Command]) -> String {
    let rest: Vec<_> = group
        .iter()
        .map(|command| command.words[1..].join(" "))
        .collect();
    rest.join(", ")
}

fn overview(commands: &[&Command], root: &Path) -> String {
    let mut text = String::from("xtask: ConsensFlow's build, test and driver commands.\n");
    text += &format!("checkout: {}\n\n", root.display());
    text += "Usage: cargo xtask <command> [arguments]\n\nCommands:\n";
    text += &listing(commands);
    text += "\nThe arguments after a command go to what it runs, as they are. A command's \
             own help (`cargo xtask <command> --help`)\nsays what that is and what it takes.\n";
    text
}

fn group_help(name: &str, group: &[&Command]) -> String {
    format!(
        "Usage: cargo xtask {name} <command> [arguments]\n\nCommands:\n{}",
        listing(group)
    )
}

/// One line for each command: its words, and what it does.
fn listing(commands: &[&Command]) -> String {
    let width = commands
        .iter()
        .map(|command| command.words.join(" ").len())
        .max()
        .unwrap_or(0);
    commands
        .iter()
        .map(|command| format!("  {:<width$}  {}\n", command.words.join(" "), command.about))
        .collect()
}

/// What a command's own help says: how it is used, what it does, and that it
/// runs in Rust.
pub(crate) fn command_help(command: &Command) -> String {
    let words = command.words.join(" ");
    let usage = if command.usage.is_empty() {
        words
    } else {
        format!("{words} {}", command.usage)
    };
    format!(
        "Usage: cargo xtask {usage}\n\n{}\n\nIt runs in Rust.\n",
        command.about
    )
}

#[cfg(test)]
mod tests;
