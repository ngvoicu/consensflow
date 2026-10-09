//! The command line: which step a word names, what a step runs with, and how a
//! step that could not finish is told. A step is a module with a [`Command`]
//! of its own, listed in [`COMMANDS`]; the landing that builds a step edits that
//! module, and this file only when a step is added.

use std::ffi::{OsStr, OsString};
use std::io::{self, ErrorKind, Write};

use cf_base::env::Env;

use crate::{sign_mac, update, version};

/// A step of the release.
pub struct Command {
    /// The word that names it.
    pub name: &'static str,
    /// What it does, in a line, for the list of commands.
    pub about: &'static str,
    /// What follows its name, for its own help.
    pub usage: &'static str,
    /// Runs it with the words after its name.
    pub run: fn(&Env, &[OsString], &mut Console) -> Result<(), Failure>,
}

/// The two streams a step speaks on: its result on `out`, how it is getting
/// on on `err`.
pub struct Console<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
}

/// Why a step did not finish.
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    /// The words after the step's name are not what it takes: the status is 2.
    #[error("{0}")]
    Usage(String),
    /// The step ran and could not do what it was asked: the status is 1.
    #[error("{0}")]
    Failed(String),
    /// What it had to say could not be written.
    #[error("{0}")]
    Io(#[from] io::Error),
}

/// Every step, in the order the release runs them.
pub const COMMANDS: [&Command; 3] = [&version::COMMAND, &update::COMMAND, &sign_mac::COMMAND];

/// Runs the command line `args` (without the program's name) and answers the
/// exit status: 0 when the step finished, 1 when it could not, 2 when the
/// words were not ones it takes.
pub fn run(env: &Env, args: &[OsString], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let mut console = Console { out, err };
    let (name, result) = dispatch(env, args, &mut console);
    let Err(failure) = result else { return 0 };
    // `cf-release version | head -0` closes the pipe: nobody is left to tell.
    if matches!(&failure, Failure::Io(cause) if cause.kind() == ErrorKind::BrokenPipe) {
        return 0;
    }
    let who = name.map_or_else(
        || "cf-release".to_string(),
        |name| format!("cf-release {name}"),
    );
    let _ = writeln!(console.err, "{who}: {failure}");
    match failure {
        Failure::Usage(_) => {
            let _ = writeln!(console.err, "see `{who} --help`");
            2
        }
        Failure::Failed(_) | Failure::Io(_) => 1,
    }
}

/// Finds the step `args` name and runs it, or answers the help asked for: the
/// step's name for what to call it in a message, and how it came out.
fn dispatch(
    env: &Env,
    args: &[OsString],
    console: &mut Console,
) -> (Option<&'static str>, Result<(), Failure>) {
    let Some(first) = args.first() else {
        return (None, Err(Failure::Usage("a command is required".into())));
    };
    if is_help(first) {
        return (None, top_help(console.out).map_err(Failure::from));
    }
    let Some(command) = COMMANDS.iter().find(|command| first == command.name) else {
        let unknown = format!("unknown command: {}", first.to_string_lossy());
        return (None, Err(Failure::Usage(unknown)));
    };
    let rest = &args[1..];
    if rest.first().is_some_and(|word| is_help(word)) {
        let helped = command_help(command, console.out).map_err(Failure::from);
        return (Some(command.name), helped);
    }
    (Some(command.name), (command.run)(env, rest, console))
}

fn is_help(word: &OsStr) -> bool {
    word == "--help" || word == "-h"
}

fn top_help(out: &mut dyn Write) -> io::Result<()> {
    writeln!(out, "cf-release: the release's steps.\n")?;
    writeln!(out, "Usage: cf-release <command> [arguments]\n")?;
    writeln!(out, "Commands:")?;
    for command in COMMANDS {
        writeln!(out, "  {:<15} {}", command.name, command.about)?;
    }
    writeln!(
        out,
        "\n`cf-release <command> --help` says what a command takes."
    )
}

fn command_help(command: &Command, out: &mut dyn Write) -> io::Result<()> {
    writeln!(
        out,
        "Usage: cf-release {} {}\n",
        command.name, command.usage
    )?;
    writeln!(out, "{}", command.about)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<OsString> {
        line.split_whitespace().map(OsString::from).collect()
    }

    /// What `cf-release <line>` answers: its status, what it printed, what it said.
    fn answer(line: &str) -> (u8, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = run(&Env::default(), &words(line), &mut out, &mut err);
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn every_step_of_the_release_is_a_command_with_a_help_of_its_own() {
        let names: Vec<_> = COMMANDS.iter().map(|command| command.name).collect();
        assert_eq!(names, ["version", "prepare-update", "sign-mac"]);
        let (status, out, err) = answer("--help");
        assert_eq!((status, err.as_str()), (0, ""));
        for command in COMMANDS {
            assert!(
                out.contains(command.name) && out.contains(command.about),
                "{out}"
            );
            let (status, out, err) = answer(&format!("{} --help", command.name));
            assert_eq!((status, err.as_str()), (0, ""));
            assert!(
                out.starts_with(&format!("Usage: cf-release {}", command.name)),
                "{out}"
            );
        }
        assert_eq!(answer("-h").1, answer("--help").1);
    }

    #[test]
    fn a_word_that_names_no_step_is_refused_with_the_status_of_a_usage_error() {
        let (status, out, err) = answer("sign");
        assert_eq!((status, out.as_str()), (2, ""));
        assert!(
            err.starts_with("cf-release: unknown command: sign\n"),
            "{err}"
        );
        assert!(err.contains("see `cf-release --help`"), "{err}");
        let (status, _, err) = answer("");
        assert_eq!(status, 2);
        assert!(
            err.starts_with("cf-release: a command is required\n"),
            "{err}"
        );
    }

    #[test]
    fn a_step_that_is_built_refuses_arguments_it_does_not_take_as_a_usage_error() {
        let (status, out, err) = answer("prepare-update --url x");
        assert_eq!((status, out.as_str()), (2, ""));
        assert!(
            err.starts_with("cf-release prepare-update: unknown argument: --url\n"),
            "{err}"
        );
        assert!(
            err.contains("see `cf-release prepare-update --help`"),
            "{err}"
        );
    }

    #[test]
    fn a_closed_pipe_is_a_quiet_end_not_a_failure() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut err = Vec::new();
        let status = run(&Env::default(), &words("--help"), &mut Closed, &mut err);
        assert_eq!((status, err.is_empty()), (0, true));
    }
}
