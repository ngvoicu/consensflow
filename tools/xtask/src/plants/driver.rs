//! One plant at a time: copied out of the checkout, planted, tried by its runs,
//! and put back.
//!
//! Every file a plant touches is first copied to a folder outside the checkout,
//! and is put back from that copy, byte for byte, whatever the runs came to: an
//! error, a panic and a stop (which [`Runner::stopped`] and a command ended by one
//! tell) all put the files back. Only a kill past that leaves the copies where they
//! are, in the folder the run says first. The plants are never run in parallel:
//! the sources are changed under them.
//!
//! A file is put back by writing its bytes, never by copying the file: a system
//! that copies a file's times with it (macOS and Windows do) would give the source
//! back as old as it was, older than what was built from the plant, and cargo
//! would take that build for current and leave a planted program in the build
//! folder for whatever runs next.
//!
//! A plant that does not apply (the text it replaces is not as it says, a file it
//! takes as given is not there, a command it needs first fails) is an error to
//! mend, never a pass: it is told, and counts as not caught.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::kit::{Family, Plant, Planted};
use super::report::{self, Verdict};
use super::runs::{Execution, Ran, Runner};
use super::Error;
use crate::context::Context;
use crate::dispatch::Failure;

/// The status of a run that was stopped, as a program ends that a signal stops.
const STOPPED: i32 = 130;

/// The files of a plant in the checkout while its runs go, and how they go back.
struct Planting {
    copies: Vec<Copy>,
    restored: bool,
}

struct Copy {
    path: PathBuf,
    copy: PathBuf,
    original: Vec<u8>,
}

impl Planting {
    /// Copies each file of `files` into `saved` and writes the plant in its place.
    fn make(saved: &Path, files: Vec<Planted>) -> Result<Self, Error> {
        let mut planting = Self {
            copies: Vec::new(),
            restored: false,
        };
        for (index, file) in files.into_iter().enumerate() {
            let copy = saved.join(format!("{index}-{}", file.file.replace('/', "_")));
            fs::write(&copy, &file.original).map_err(Error::files("write a copy at", &copy))?;
            // From here the file is put back, even if the write is what fails.
            planting.copies.push(Copy {
                path: file.path.clone(),
                copy,
                original: file.original,
            });
            fs::write(&file.path, file.text).map_err(Error::files("write", &file.path))?;
        }
        Ok(planting)
    }

    /// Puts every file back from its copy, and says if one is not as it was.
    fn restore(&mut self) -> Result<(), Error> {
        for Copy {
            path,
            copy,
            original,
        } in &self.copies
        {
            let saved = fs::read(copy).map_err(Error::files("read the copy of", path))?;
            fs::write(path, saved).map_err(Error::files("put back", path))?;
            if fs::read(path).map_err(Error::files("read", path))? != *original {
                return Err(Error::NotRestored {
                    path: path.clone(),
                    copy: copy.clone(),
                });
            }
        }
        self.restored = true;
        Ok(())
    }
}

impl Drop for Planting {
    /// A plant still in the sources, as a panic unwinds: put back, as well as can be.
    fn drop(&mut self) {
        if !self.restored {
            let _ = self.restore();
        }
    }
}

/// How a plant ended, and what the last run said.
struct Trial {
    verdict: Verdict,
    ran: Option<Ran>,
    /// The tests that failed, or what else caught it.
    caught: Vec<String>,
    /// Why it did not apply.
    said: String,
}

impl Trial {
    fn of(verdict: Verdict, ran: Ran, caught: Vec<String>) -> Self {
        Self {
            verdict,
            ran: Some(ran),
            caught,
            said: String::new(),
        }
    }

    fn unapplied(said: String) -> Self {
        Self {
            verdict: Verdict::DoesNotApply,
            ran: None,
            caught: Vec::new(),
            said,
        }
    }
}

/// Whether what a plant needs first is done.
enum Prepared {
    Done,
    /// A command failed.
    Failed(Error),
    Stopped,
}

/// Runs the commands a plant needs before it is planted, which must succeed.
fn prepare(plant: &Plant, runner: &mut dyn Runner) -> Result<Prepared, Error> {
    for command in &plant.prepare {
        match runner.run(command)? {
            Execution::Stopped => return Ok(Prepared::Stopped),
            Execution::Ended(ran) if ran.code != Some(0) => {
                return Ok(Prepared::Failed(Error::Prepare {
                    plant: plant.name.clone(),
                    command: command.join(" "),
                    said: report::tail(&ran.output, 1500).to_owned(),
                }));
            }
            Execution::Ended(_) => {}
        }
    }
    Ok(Prepared::Done)
}

/// The files its runs take as given: one that is not there makes a run fail by
/// itself, which would be taken for a catch.
fn required(plant: &Plant, context: &Context) -> Result<(), Error> {
    match plant
        .requires
        .iter()
        .find(|file| !context.path(file).exists())
    {
        Some(file) => Err(Error::NotThere {
            plant: plant.name.clone(),
            file: file.clone(),
        }),
        None => Ok(()),
    }
}

/// Plants `plant`, tries its runs in order until one decides, and puts it back.
/// `built` is made true where a run that builds the native `cf` is about to be
/// run. `None` is a stop.
fn trial(
    plant: &Plant,
    context: &Context,
    saved: &Path,
    runner: &mut dyn Runner,
    built: &mut bool,
) -> Result<Option<Trial>, Error> {
    if let Err(unready) = required(plant, context) {
        return Ok(Some(Trial::unapplied(unready.to_string())));
    }
    match prepare(plant, runner)? {
        Prepared::Stopped => return Ok(None),
        Prepared::Failed(unready) => return Ok(Some(Trial::unapplied(unready.to_string()))),
        Prepared::Done => {}
    }
    let files = match plant.planted(context) {
        Ok(files) => files,
        Err(unapplied) => return Ok(Some(Trial::unapplied(unapplied.to_string()))),
    };
    let mut planting = Planting::make(saved, files)?;
    let mut last = None;
    for run in &plant.runs {
        *built = *built || run.builds || plant.builds;
        let ran = match runner.run(&run.command)? {
            Execution::Ended(ran) => ran,
            Execution::Stopped => {
                planting.restore()?;
                return Ok(None);
            }
        };
        let caught = ran.caught();
        let verdict = if !ran.compiled() {
            Some(Verdict::DoesNotCompile)
        } else if ran.hung {
            Some(Verdict::Hung)
        } else if !caught.is_empty() {
            Some(Verdict::Caught)
        } else {
            None
        };
        if let Some(verdict) = verdict {
            planting.restore()?;
            return Ok(Some(Trial::of(verdict, ran, caught)));
        }
        last = Some(ran);
    }
    planting.restore()?;
    Ok(Some(match last {
        Some(ran) => Trial::of(Verdict::Missed, ran, Vec::new()),
        None => Trial::unapplied(format!("{}: it has no run", plant.name)),
    }))
}

/// Says a plant, and what follows from how it ended.
fn say(
    out: &mut dyn Write,
    plant: &Plant,
    trial: &Trial,
    started: Instant,
    limit: u64,
) -> Result<(), Failure> {
    let seconds = u64::try_from((started.elapsed().as_millis() + 500) / 1000).unwrap_or(u64::MAX);
    let by = if trial.verdict == Verdict::Caught {
        report::caught_by(&trial.caught, &plant.meant)
    } else {
        String::new()
    };
    writeln!(
        out,
        "{}",
        report::line(trial.verdict, &plant.name, seconds, &by)
    )?;
    let Some(ran) = &trial.ran else {
        writeln!(out, "    {}", trial.said)?;
        return Ok(());
    };
    match trial.verdict {
        Verdict::Caught => {
            if let Some(failure) = report::first_failure(&ran.output) {
                writeln!(out, "    first failure: {failure}")?;
            }
        }
        Verdict::DoesNotCompile => writeln!(out, "{}", report::tail(&ran.output, 1500))?,
        Verdict::Hung => writeln!(
            out,
            "    {} did not end in {limit} s",
            ran.command.join(" ")
        )?,
        Verdict::Missed => {
            for line in report::what_ran(&ran.output) {
                writeln!(out, "    {line}")?;
            }
        }
        Verdict::DoesNotApply => {}
    }
    Ok(())
}

/// How building the native `cf` again went.
enum Rebuilt {
    Done,
    Failed,
    Stopped,
}

/// Builds again what a run built from planted sources, if one did.
fn rebuild(
    family: &Family,
    runner: &mut dyn Runner,
    built: &mut bool,
    out: &mut dyn Write,
) -> Result<Rebuilt, Failure> {
    let was_built = std::mem::take(built);
    let Some(command) = family.rebuild.as_ref().filter(|_| was_built) else {
        return Ok(Rebuilt::Done);
    };
    match runner.run(command)? {
        Execution::Stopped => Ok(Rebuilt::Stopped),
        Execution::Ended(ran) if ran.code == Some(0) => Ok(Rebuilt::Done),
        Execution::Ended(ran) => {
            writeln!(
                out,
                "the native cf of bin/ is not built again:\n{}",
                ran.output
            )?;
            Ok(Rebuilt::Failed)
        }
    }
}

/// How a run's folder is named: three letters and as few random ones as keep two runs
/// apart, which is for the length of the path (see [`run_folder`]).
const RUN_PREFIX: &str = "cfp";
const RUN_RANDOM: usize = 4;

/// Makes the folder a run keeps its copies in, and gives its commands as their temp folder:
/// `/tmp/cfpXXXX` on Unix, else under the system's temp folder.
///
/// It is short on purpose. The Codex session of a rig makes a Unix socket in the temp folder
/// it is given (or in its home, which is made there), and a socket's path is 104 bytes at
/// most on a Mac, a Mac's own temp folder 48 bytes long before any name of ours. Under it
/// the longest such path is 93 bytes with `cf-plants-XXXXXX`, and no name leaves it 20 to
/// spare; at `/tmp/cfpXXXX` (12 bytes) the longest is 84, 20 to spare.
fn run_folder() -> Result<PathBuf, Error> {
    let make = |base: &Path| {
        tempfile::Builder::new()
            .prefix(RUN_PREFIX)
            .rand_bytes(RUN_RANDOM)
            .tempdir_in(base)
            .map(tempfile::TempDir::keep)
    };
    if cfg!(unix) {
        if let Ok(folder) = make(Path::new("/tmp")) {
            return Ok(folder);
        }
    }
    let temp = std::env::temp_dir();
    make(&temp).map_err(Error::files("make a folder for the copies in", &temp))
}

/// Ends a run that was stopped: the copies are not wanted, everything is back.
fn stopped(saved: &Path, out: &mut dyn Write) -> Result<i32, Failure> {
    let _ = fs::remove_dir_all(saved);
    writeln!(out, "stopped: every file is as it was")?;
    Ok(STOPPED)
}

/// Plants each of `plants` in turn and says how each ended: the status is 0 where
/// every one was caught.
pub fn run(
    context: &Context,
    family: &Family,
    plants: &[&Plant],
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> Result<i32, Failure> {
    let saved = run_folder()?;
    writeln!(
        out,
        "copies of what is planted are kept in {}",
        saved.display()
    )?;
    // The run's folder is the temp folder of every command of it: what a plant makes fail
    // on purpose leaves reports and homes there, and they go with it.
    runner.temp_in(&saved);
    let limit = family.limit.as_secs();
    let mut built = false;
    let mut wrong = 0;
    for plant in plants {
        if runner.stopped() {
            return stopped(&saved, out);
        }
        let started = Instant::now();
        let Some(trial) = trial(plant, context, &saved, runner, &mut built)? else {
            return stopped(&saved, out);
        };
        say(out, plant, &trial, started, limit)?;
        if trial.verdict != Verdict::Caught {
            wrong += 1;
        }
        match rebuild(family, runner, &mut built, out)? {
            Rebuilt::Done => {}
            Rebuilt::Failed => wrong += 1,
            Rebuilt::Stopped => return stopped(&saved, out),
        }
    }
    // Gone with the run only when every plant was caught. What a plant that was not leaves
    // of it (the home of a rig a hung run was ended in, the report of a case caught by
    // another test than the one meant) is in the run's temp folder, and nowhere else.
    if wrong == 0 {
        let _ = fs::remove_dir_all(&saved);
    } else {
        writeln!(
            out,
            "{wrong} {} not caught: what {} runs left is kept in {}",
            if wrong == 1 {
                "plant was"
            } else {
                "plants were"
            },
            if wrong == 1 { "its" } else { "their" },
            saved.display()
        )?;
    }
    let caught = plants.len().saturating_sub(wrong);
    writeln!(out, "{}", report::totals(caught, plants.len()))?;
    Ok(i32::from(wrong != 0))
}

/// Whether each of `plants` applies, which plants nothing: the status is 0 where
/// every one does. The commands a plant needs first are run.
pub fn check(
    context: &Context,
    plants: &[&Plant],
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> Result<i32, Failure> {
    let mut unapplied = 0;
    for plant in plants {
        let applied = match prepare(plant, runner)? {
            Prepared::Stopped => return Ok(STOPPED),
            Prepared::Failed(unready) => Err(unready),
            Prepared::Done => plant.planted(context).map(drop),
        };
        if let Err(cause) = applied {
            unapplied += 1;
            writeln!(out, "{cause}")?;
        }
    }
    if unapplied == 0 {
        writeln!(out, "{} plants apply", plants.len())?;
        return Ok(0);
    }
    writeln!(
        out,
        "{} of {} plants apply",
        plants.len() - unapplied,
        plants.len()
    )?;
    Ok(1)
}

#[cfg(test)]
mod tests;
