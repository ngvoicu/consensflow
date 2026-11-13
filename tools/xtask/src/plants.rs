//! `cargo xtask plants <family> [words…] [--check]`: the repository's mutation
//! tests. A plant is a bug put into the product on purpose; each is
//! planted in the sources, the tests that should notice are run, and the plant is
//! reported caught or missed. The plants are data, `tests/plants/<family>/`
//! ([`kit`] tells how they are written); a family is a folder of them, run by the
//! same driver:
//!
//! ```text
//! cargo xtask plants cli                      # every plant of a family (npm run plants:cli)
//! cargo xtask plants cli parser dispatch      # the plants whose names hold a word
//! cargo xtask plants cli --check              # only that every plant still applies
//! ```
//!
//! A plant that stops applying (the text it replaces was changed) is an error to
//! mend, not a pass. A plant is caught only where the test it was meant for fails
//! with it, as the run's reporter writes it to a report of its own, never as the
//! run prints it; everything else counts as not caught, and says why (the driver
//! tells the verdicts). The status is 1 if any plant is not caught or does not
//! apply, and 130 if the run was stopped. A word that no plant's name holds is
//! refused: a filter that picks nothing would otherwise pass. A `cargo test` of a
//! plant's runs, and a suite of xtask's that is one, is run under nextest, which
//! a fresh tree copies into `app/src-tauri/target/tools/bin/` from
//! `~/Projects/ngvoicu/consensflow-trees/.runs/tools/bin/cargo-nextest`.
//!
//! The modules: [`kit`] (what a plant is, and the data), [`driver`] (planting,
//! putting back, the trial of one plant), [`runs`] (the commands, the reports their
//! tests write, and what ends a run early: `crate::stops`) and [`report`] (what is
//! said).

mod driver;
mod kit;
mod report;
mod runs;

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process;
pub(crate) use kit::families;
use kit::Family;
use runs::{Execute, Runner};

const ABOUT: &str = "Plant the bugs of a family one at a time and check that a test fails for each one (a family of tests/plants)";
const USAGE: &str = "<family> [words in a plant's name…] [--check] [--test-threads N]";
/// What `--test-threads` does and why it is there: the Mac's one machine, where a run
/// uncapped stopped the board over its heat (`this heat stopped the board once`).
const OPTIONS: &str = "      --test-threads N   nextest and cargo test runs get N threads; it wins over a table's; this heat stopped the board once\n      CF_PLANTS_TEST_THREADS=N   the same, where no flag is given";

/// The variable that carries the threads the plants driver was asked for, from the flag
/// or the environment: nextest is given them as its own option (`reporters`).
const THREADS_VAR: &str = "CF_PLANTS_TEST_THREADS";

pub const COMMANDS: &[Command] = &[Command {
    words: &["plants"],
    about: ABOUT,
    usage: USAGE,
    run: Run::Native(run),
}];

/// Why the plants could not be read, planted or put back.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A file of the plants' data that cannot be read as such.
    #[error("{}: {cause}", path.display())]
    Data { path: PathBuf, cause: String },
    #[error("could not {what} {}: {cause}", path.display())]
    Files {
        what: &'static str,
        path: PathBuf,
        cause: io::Error,
    },
    /// The text a plant replaces is not in its file, or is there more than once.
    #[error("{plant}: {file} holds {find} {found} times, not once")]
    NotOnce {
        plant: String,
        file: String,
        find: String,
        found: usize,
    },
    /// A file the runs of a plant take as given is not there.
    #[error("{plant}: {file} is not there")]
    NotThere { plant: String, file: String },
    /// A command a plant needs before it is planted failed.
    #[error("{plant}: {command} failed\n{said}")]
    Prepare {
        plant: String,
        command: String,
        said: String,
    },
    /// A file that was planted is not as it was once put back.
    #[error("{} is not as it was: its copy is {}", path.display(), copy.display())]
    NotRestored { path: PathBuf, copy: PathBuf },
    /// A file that was planted that could not be put back: where its copy is, and why.
    #[error("{} is still planted: put it back from its copy, {} ({cause})", path.display(), copy.display())]
    StillPlanted {
        path: PathBuf,
        copy: PathBuf,
        cause: io::Error,
    },
    /// Files that were planted that could not be put back, each as it failed.
    #[error("{} files of a plant are not as they were:\n{}", .0.len(), lines(.0))]
    Unrestored(Vec<Error>),
    /// Files of a plant that could not be put back once its trial had ended in an error
    /// (`cause`): they are said first, as no run is to go on over them.
    #[error("{unrestored}\nand before that: {cause}")]
    PlantedAfter {
        unrestored: Box<Error>,
        cause: Box<Error>,
    },
    /// A word that no plant's name holds.
    #[error("no plant of {family} has a name that holds `{word}`")]
    NoName { word: String, family: String },
    /// A checkout git says nothing of, where what a run changed cannot be told.
    #[error("plants need a git checkout at {} to tell a changed checkout", root.display())]
    NoCheckout { root: PathBuf },
    /// What a run started did not end once it was ended: nothing of the run is over,
    /// and the plants' run ends, its files put back as far as they can be.
    #[error("{command}: {said}")]
    LeftRunning { command: String, said: String },
    /// A command that could not be started.
    #[error(transparent)]
    Process(#[from] process::Failure),
}

/// Each of `errors`, a line each.
fn lines(errors: &[Error]) -> String {
    let each: Vec<String> = errors.iter().map(ToString::to_string).collect();
    each.join("\n")
}

impl Error {
    /// How a failed file operation is told: what was being done, and to which path.
    fn files(what: &'static str, path: &Path) -> impl FnOnce(io::Error) -> Self {
        let path = path.to_path_buf();
        move |cause| Self::Files { what, path, cause }
    }

    /// Whether it says that files of a plant could not be put back.
    fn unrestored(&self) -> bool {
        matches!(
            self,
            Self::NotRestored { .. }
                | Self::StillPlanted { .. }
                | Self::Unrestored(_)
                | Self::PlantedAfter { .. }
        )
    }

    fn data(path: &Path, cause: impl Into<String>) -> Self {
        Self::Data {
            path: path.to_path_buf(),
            cause: cause.into(),
        }
    }
}

/// What the command line asks for.
#[derive(Debug)]
struct Request {
    family: Option<String>,
    words: Vec<String>,
    check: bool,
    /// `--help`, wherever it is: a family comes first, so the help xtask answers
    /// for the first word of a command's arguments is not the one asked for here.
    help: bool,
    /// The threads every nextest run is capped at, where asked for.
    test_threads: Option<u32>,
}

/// A number of test threads, where `value` is one and more than none.
fn threads_value(value: &str) -> Option<u32> {
    value.parse::<u32>().ok().filter(|threads| *threads > 0)
}

impl Request {
    fn read(args: &[OsString]) -> Result<Self, Failure> {
        let mut request = Self {
            family: None,
            words: Vec::new(),
            check: false,
            help: false,
            test_threads: None,
        };
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let Some(word) = arg.to_str() else {
                return Err(Failure::Usage(
                    "plants takes text, not a name that is none".into(),
                ));
            };
            if word == "--check" {
                request.check = true;
            } else if word == "--help" {
                request.help = true;
            } else if word == "--test-threads" {
                let Some(value) = args.next().and_then(|arg| arg.to_str()) else {
                    return Err(Failure::Usage(
                        "plants --test-threads takes a number of threads".into(),
                    ));
                };
                match threads_value(value) {
                    Some(threads) => request.test_threads = Some(threads),
                    _ => {
                        return Err(Failure::Usage(format!(
                            "plants --test-threads takes a number of threads, not {value}"
                        )))
                    }
                }
            } else if let Some(value) = word.strip_prefix("--test-threads=") {
                match threads_value(value) {
                    Some(threads) => request.test_threads = Some(threads),
                    _ => {
                        return Err(Failure::Usage(format!(
                            "plants --test-threads takes a number of threads, not {value}"
                        )))
                    }
                }
            } else if word.starts_with("--") {
                return Err(Failure::Usage(format!(
                    "plants takes --check and --test-threads, not {word}"
                )));
            } else if request.family.is_none() {
                request.family = Some(word.to_owned());
            } else {
                request.words.push(word.to_owned());
            }
        }
        Ok(request)
    }
}

/// `context` as the caller's threads are to be given to runs: the flag's, where given
/// (`--test-threads` in `args`), and the variable's otherwise; the flag's win, and a
/// variable of no number of threads is refused, never run wide.
fn with_caller_threads(context: &Context, args: &[OsString]) -> Result<Context, Failure> {
    let threads = match Request::read(args)?.test_threads {
        Some(threads) => Some(threads),
        None => match context.env.text(THREADS_VAR) {
            None => None,
            Some(value) => match threads_value(value) {
                Some(threads) => Some(threads),
                _ => {
                    return Err(Failure::Usage(format!(
                        "{THREADS_VAR} takes a number of threads, not {value}"
                    )))
                }
            },
        },
    };
    match threads {
        Some(threads) => Ok(Context {
            root: context.root.clone(),
            env: context
                .env
                .clone()
                .with_var(THREADS_VAR, &threads.to_string()),
        }),
        None => Ok(context.clone()),
    }
}

pub(crate) fn run(
    context: &Context,
    args: &[OsString],
    console: &mut Console,
) -> Result<i32, Failure> {
    // This xtask is what `cargo xtask` is to the commands that run.
    let own = std::env::current_exe()?;
    let context = with_caller_threads(context, args)?;
    plant(&context, args, console, |family| {
        Execute::new(&context, own, family.limit)
    })
}

/// The command, given the runner of the commands it is to run: made of the family,
/// once it is known which it is.
fn plant<R: Runner>(
    context: &Context,
    args: &[OsString],
    console: &mut Console,
    runner: impl FnOnce(&Family) -> Result<R, Error>,
) -> Result<i32, Failure> {
    let request = Request::read(args)?;
    let folder = context.path("tests/plants");
    let families = kit::families(&folder)?;
    if request.help {
        writeln!(
            console.out,
            "Usage: cargo xtask plants {USAGE}\n\n{ABOUT}\n\n{OPTIONS}\n\nThe families: {}.",
            families.join(", ")
        )?;
        return Ok(0);
    }
    let Some(name) = request.family.filter(|name| families.contains(name)) else {
        return Err(Failure::Usage(format!(
            "plants takes a family: {}",
            families.join(", ")
        )));
    };
    let family = Family::load(&folder, &name)?;
    let chosen = family.choose(&request.words).map_err(|error| match error {
        Error::NoName { .. } => Failure::Usage(error.to_string()),
        other => Failure::from(other),
    })?;
    let mut runner = runner(&family)?;
    if request.check {
        driver::check(context, &family, &chosen, &mut runner, console.out)
    } else {
        driver::run(context, &family, &chosen, &mut runner, console.out)
    }
}

#[cfg(test)]
mod tests;
