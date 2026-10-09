//! The command line: which command the words name, what it is handed, and how
//! the answer reaches the caller.
//!
//! A command is a row in the table of the module that owns it
//! (`sidecar::COMMANDS`, `check::COMMANDS`, ...), and [`commands`] joins the
//! tables. A row either runs in Rust ([`Run::Native`]) or, until the landing
//! that ports it, hands its arguments to the Node script it replaces
//! ([`Run::Node`]): `node <script> <arguments>`, from the folder the script
//! was run from, with the script's exit status as its own. The arguments after
//! a command's words go to it as they are, `--` and words with spaces in them
//! included; only a first one that is `--help` (or `-h`) is xtask's.

use std::ffi::{OsStr, OsString};
use std::io::{self, ErrorKind, Write};
use std::path::Path;

use cf_base::env::Env;

use crate::context::{self, Context};
use crate::process::{self, Invocation};
use crate::{
    app, bench, candidate, check, clippy_windows, departures, portable, sidecar, smoke, suites,
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
    /// By the Node script it replaces, until the landing that ports it.
    Node(Script),
    /// In Rust: given the arguments after its words, and the two streams, it
    /// answers the exit status.
    Native(fn(&Context, &[OsString], &mut Console) -> Result<i32, Failure>),
}

/// A Node script a command hands its arguments to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Script {
    /// The script, from the checkout's root, with `/` between folders.
    pub file: &'static str,
    /// The folder it runs from, from the checkout's root; empty for the root
    /// itself, where `npm run` ran a script of the root's `package.json`.
    pub from: &'static str,
}

impl Script {
    /// A script that runs from the checkout's root.
    pub const fn at_root(file: &'static str) -> Self {
        Self { file, from: "" }
    }

    /// `node <script> <args>`, as it runs.
    pub fn invocation(&self, context: &Context, args: &[OsString]) -> Invocation {
        Invocation::new("node", context.path(self.from))
            .arg(context.path(self.file))
            .args(args.iter().cloned())
    }
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
        departures::COMMANDS,
        bench::COMMANDS,
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
        Parsed::Run { command, args } => match &command.run {
            Run::Node(script) => {
                let status = process::run(&script.invocation(context, args), &context.env)?;
                Ok(status)
            }
            Run::Native(run) => run(context, args, console),
        },
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

fn command_help(command: &Command) -> String {
    let words = command.words.join(" ");
    let usage = if command.usage.is_empty() {
        words
    } else {
        format!("{words} {}", command.usage)
    };
    let runs = match &command.run {
        Run::Node(script) => {
            let from = if script.from.is_empty() {
                "the checkout's root"
            } else {
                script.from
            };
            format!(
                "For now it runs node {} (from {from}) and hands it the arguments as they are.",
                script.file
            )
        }
        Run::Native(_) => "It runs in Rust.".to_string(),
    };
    format!(
        "Usage: cargo xtask {usage}\n\n{}\n\n{runs}\n",
        command.about
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(words: &str) -> Vec<OsString> {
        words.split_whitespace().map(OsString::from).collect()
    }

    /// What `cargo xtask <words>` is asked to do, as text for a test to read.
    fn parsed(words: &str) -> String {
        let args = line(words);
        match parse(&commands(), &args, Path::new("/checkout")) {
            Parsed::Run { command, args } => format!("run {} {args:?}", command.words.join(" ")),
            Parsed::Help(text) => format!("help\n{text}"),
            Parsed::Usage(message) => format!("usage {message}"),
        }
    }

    #[test]
    fn every_command_of_the_plan_is_there_once() {
        let words: Vec<_> = commands()
            .iter()
            .map(|command| command.words.join(" "))
            .collect();
        assert_eq!(
            words,
            [
                "build-cf",
                "stage",
                "conpty",
                "portable",
                "app test",
                "app clippy",
                "clippy-windows",
                "test daemons",
                "test clis",
                "test agents",
                "departures",
                "bench records-memory",
                "smoke",
                "smoke-updater",
                "candidate",
                "check",
            ]
        );
    }

    #[test]
    fn a_command_is_named_by_its_words_and_takes_the_rest_as_they_are() {
        assert_eq!(parsed("departures"), "run departures []");
        assert_eq!(
            parsed("app test portable::"),
            r#"run app test ["portable::"]"#
        );
        // `--` and what follows it are the command's, not xtask's.
        assert_eq!(
            parsed("test daemons --offline -- --help"),
            r#"run test daemons ["--offline", "--", "--help"]"#
        );
        // A word xtask has no meaning for is the command's too.
        assert_eq!(
            parsed("smoke --app ./x.app"),
            r#"run smoke ["--app", "./x.app"]"#
        );
        assert_eq!(parsed("check -h"), parsed("check --help"));
    }

    #[test]
    fn words_with_spaces_in_them_stay_one_word_each() {
        let args: Vec<OsString> = ["smoke", "--app", "A Folder/Consens Flow.app", "", "-"]
            .iter()
            .map(OsString::from)
            .collect();
        let Parsed::Run { command, args } = parse(&commands(), &args, Path::new("/checkout"))
        else {
            panic!("smoke is a command");
        };
        assert_eq!(command.words, ["smoke"]);
        assert_eq!(args, ["--app", "A Folder/Consens Flow.app", "", "-"]);
    }

    #[test]
    fn a_help_asked_first_is_xtasks_and_asks_nothing_to_run() {
        let all = parsed("--help");
        assert!(all.starts_with("help\nxtask: "), "{all}");
        assert!(all.contains("checkout: /checkout\n"), "{all}");
        for command in commands() {
            assert!(all.contains(&command.words.join(" ")), "{all}");
            assert!(all.contains(command.about), "{all}");
        }
        assert_eq!(parsed("-h"), all);
        // A command's own help says what it takes and what it runs.
        let help = parsed("build-cf --help");
        assert!(
            help.starts_with("help\nUsage: cargo xtask build-cf [--offline]\n"),
            "{help}"
        );
        assert!(help.contains("It runs in Rust."), "{help}");
        assert!(parsed("check --help").contains("It runs in Rust."));
        // A help that is not the first word is the command's own argument.
        assert_eq!(
            parsed("build-cf --offline --help"),
            r#"run build-cf ["--offline", "--help"]"#
        );
    }

    /// A command that hands its arguments to a script, whichever the real ones
    /// still do: the landings that port them each take one from the table.
    #[test]
    fn the_help_of_a_command_that_hands_over_names_its_script_and_where_it_runs_from() {
        let command = |from| Command {
            words: &["stand-in"],
            about: "A stand-in",
            usage: "[--x]",
            run: Run::Node(Script {
                file: "scripts/stand-in.mjs",
                from,
            }),
        };
        let help = command_help(&command(""));
        assert!(
            help.starts_with("Usage: cargo xtask stand-in [--x]\n"),
            "{help}"
        );
        assert!(
            help.contains("node scripts/stand-in.mjs (from the checkout's root)"),
            "{help}"
        );
        assert!(command_help(&command("app")).contains("node scripts/stand-in.mjs (from app)"));
    }

    #[test]
    fn a_group_of_commands_has_its_help_and_wants_a_command_of_it() {
        let help = parsed("app --help");
        assert!(
            help.starts_with("help\nUsage: cargo xtask app <command> [arguments]\n"),
            "{help}"
        );
        assert!(
            help.contains("app test") && help.contains("app clippy"),
            "{help}"
        );
        assert!(!help.contains("departures"), "{help}");
        assert_eq!(parsed("app"), "usage app takes a command: test, clippy");
        assert_eq!(
            parsed("test"),
            "usage test takes a command: daemons, clis, agents"
        );
        assert_eq!(parsed("app tests"), "usage unknown command: app tests");
    }

    #[test]
    fn words_that_name_no_command_are_refused() {
        assert_eq!(parsed(""), "usage a command is required");
        assert_eq!(parsed("deploy"), "usage unknown command: deploy");
        assert_eq!(parsed("--offline"), "usage unknown command: --offline");
        // A command's words are whole words: a longer one is another.
        assert_eq!(parsed("checks"), "usage unknown command: checks");
        assert_eq!(
            parsed("bench"),
            "usage bench takes a command: records-memory"
        );
    }

    /// `cargo xtask <words>` as `run` answers: its status, what it printed and what it said.
    fn answer(words: &str) -> (i32, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = run(&Env::from_process(), &line(words), &mut out, &mut err);
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn a_help_or_a_refusal_is_answered_without_running_anything() {
        let (status, out, err) = answer("--help");
        assert_eq!((status, err.as_str()), (0, ""));
        let root = Context::new(&Env::default()).unwrap().root;
        assert!(
            out.contains(&format!("checkout: {}\n", root.display())),
            "{out}"
        );

        let (status, out, err) = answer("stage --help");
        assert_eq!((status, err.as_str()), (0, ""));
        assert!(out.starts_with("Usage: cargo xtask stage\n"), "{out}");

        let (status, out, err) = answer("nonsense");
        assert_eq!((status, out.as_str()), (2, ""));
        assert_eq!(
            err,
            "xtask: unknown command: nonsense\nsee `cargo xtask --help`\n"
        );
        assert_eq!(answer("").0, 2);
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
        let status = run(&Env::from_process(), &line("--help"), &mut Closed, &mut err);
        assert_eq!((status, err.is_empty()), (0, true));
    }
}
