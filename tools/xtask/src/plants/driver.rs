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
//!
//! A catch is the test the plant was meant for (`meant`) failing with it. Where a
//! run that fails with the plant names the tests that failed (`cargo test`, `node
//! --test` and Playwright do), the plant is CAUGHT where the meant test is among
//! them and the same command passes once the plant is put back, which is run once
//! for the run of the plants; a failure that names only other tests goes on to the
//! plant's next run, and where none names the meant test the plant is
//! MISSED-BY-MEANT, said with the tests that failed. Where a failure names no test
//! (a command that says no more than its status, as the release smokes do),
//! nothing can be attributed, and the catch is confirmed: the run is run so again
//! and must fail again, and the command must pass twice without the plant.
//!
//! A command that fails without the plant is run again once; failing twice (a
//! program that is not there, a suite that cannot start), it leaves every plant it
//! decides NOT RUN: a bug no test was there to see is no catch. A command that
//! passes once without the plant and fails once, or a run that fails with it once
//! only, is UNSTABLE, neither caught nor missed. A run that could not load a module
//! of its own is BROKEN. A run whose tests build the native `cf` and the pane host
//! (the rig's) is preceded by their release build from the plant, which installs
//! nothing (`PREFLIGHT`): a plant that breaks it DOES NOT COMPILE, where cargo's
//! own status says the build failed; a build that ends otherwise is NOT BUILT, or
//! HUNG where its time ran out. All of these are counted as not caught, and told
//! with what they said.

mod confirm;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use self::confirm::{unstarted, Clean, CleanRuns, Judged};
use super::kit::{Family, Plant, Planted};
use super::report::{self, Verdict};
use super::runs::{Execution, Ran, Runner};
use super::Error;
use crate::context::Context;
use crate::dispatch::Failure;

/// The status of a run that was stopped, as a program ends that a signal stops.
const STOPPED: i32 = 130;

/// What a run whose tests build the native `cf` and the pane host is preceded by,
/// with the plant in: the release build of the two that the rig makes
/// (crates/cf-e2e), which puts nothing in `bin/` (that is `build-cf`'s), and says
/// in cargo's structured words whether it built.
const PREFLIGHT: [&str; 14] = [
    "cargo",
    "build",
    "--release",
    "--locked",
    "--offline",
    "--message-format=json-render-diagnostics",
    "-p",
    "cf",
    "--bin",
    "cf",
    "-p",
    "cf-panes",
    "--bin",
    "consensflow-bridge",
];

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

    /// Puts every file back from its copy, and says if one is not as it was; once.
    fn restore(&mut self) -> Result<(), Error> {
        if self.restored {
            return Ok(());
        }
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

    fn not_run(said: String) -> Self {
        Self {
            verdict: Verdict::NotRun,
            ..Self::unapplied(said)
        }
    }

    fn unstable(said: String) -> Self {
        Self {
            verdict: Verdict::Unstable,
            ..Self::unapplied(said)
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

/// One run of `command` with the plant in the sources: `Err` is that it could not
/// be started. `None` is a stop.
fn planted_run(
    command: &[String],
    runner: &mut dyn Runner,
) -> Result<Option<Result<Ran, String>>, Error> {
    match runner.run(command) {
        Ok(Execution::Ended(ran)) => Ok(Some(Ok(ran))),
        Ok(Execution::Stopped) => Ok(None),
        Err(Error::Process(failure)) => Ok(Some(Err(unstarted(command, &failure)))),
        Err(other) => Err(other),
    }
}

/// How a trial ended before its verdict, once its plant is put back.
enum Ended {
    Stopped,
    Trial(Trial),
}

/// Plants `plant`, tries its runs in order until one decides (`tried`), and puts it
/// back. `clean` is how each command does without a plant, as far as it was run so,
/// and is forgotten once the plant's `prepare` has run. `built` is made true where a
/// run that builds is about to be run. `None` is a stop.
fn trial(
    plant: &Plant,
    context: &Context,
    saved: &Path,
    runner: &mut dyn Runner,
    built: &mut bool,
    clean: &mut CleanRuns,
) -> Result<Option<Trial>, Error> {
    if let Err(unready) = required(plant, context) {
        return Ok(Some(Trial::unapplied(unready.to_string())));
    }
    match prepare(plant, runner)? {
        Prepared::Stopped => return Ok(None),
        Prepared::Failed(unready) => return Ok(Some(Trial::unapplied(unready.to_string()))),
        Prepared::Done => {}
    }
    if !plant.prepare.is_empty() {
        clean.forget();
    }
    let files = match plant.planted(context) {
        Ok(files) => files,
        Err(unapplied) => return Ok(Some(Trial::unapplied(unapplied.to_string()))),
    };
    // The state before the plant is the one its runs are known without it in.
    let unplanted = runner.state();
    let mut planting = Planting::make(saved, files)?;
    let ended = tried(plant, runner, built, clean, &unplanted, &mut planting)?;
    planting.restore()?;
    match ended {
        Ended::Stopped => Ok(None),
        Ended::Trial(trial) => Ok(Some(trial)),
    }
}

/// The build before a run whose tests build the native `cf`, with the plant in
/// (`PREFLIGHT`): `None` where it built, else how the trial ends.
fn preflight(runner: &mut dyn Runner) -> Result<Option<Ended>, Error> {
    let words: Vec<String> = PREFLIGHT.iter().map(|word| (*word).to_owned()).collect();
    let ran = match planted_run(&words, runner)? {
        None => return Ok(Some(Ended::Stopped)),
        Some(Err(said)) => return Ok(Some(Ended::Trial(Trial::not_run(said)))),
        Some(Ok(ran)) => ran,
    };
    let verdict = if ran.hung {
        Verdict::Hung
    } else if ran.build_finished() == Some(false) {
        Verdict::DoesNotCompile
    } else if ran.code == Some(0) {
        return Ok(None);
    } else {
        Verdict::NotBuilt
    };
    Ok(Some(Ended::Trial(Trial::of(verdict, ran, Vec::new()))))
}

/// How a trial ends that a run of `command` decided, by how the command did without
/// the plant: caught by the tests `caught` where it passes.
fn confirmed(clean: Option<Clean>, ran: Ran, caught: Vec<String>) -> Ended {
    match clean {
        None => Ended::Stopped,
        Some(Clean::Passes) => Ended::Trial(Trial::of(Verdict::Caught, ran, caught)),
        Some(Clean::Fails(said)) => Ended::Trial(Trial::not_run(said)),
        Some(Clean::Unstable(said)) => Ended::Trial(Trial::unstable(said)),
    }
}

/// The runs of `plant`, planted, until one decides; `planting` is put back before a
/// command is run without the plant. `unplanted` is the state of the checkout before
/// the plant, in which a command known to fail without it tells nothing.
fn tried(
    plant: &Plant,
    runner: &mut dyn Runner,
    built: &mut bool,
    clean: &mut CleanRuns,
    unplanted: &str,
    planting: &mut Planting,
) -> Result<Ended, Error> {
    let mut last = None;
    // The last run that failed naming only tests the plant was not meant for, and the
    // tests every such run named.
    let mut others: Option<(Ran, Vec<String>)> = None;
    for run in &plant.runs {
        if let Some(said) = clean.fails(&run.command, unplanted) {
            return Ok(Ended::Trial(Trial::not_run(said)));
        }
        let builds = run.builds || plant.builds;
        *built = *built || builds;
        if builds {
            if let Some(ended) = preflight(runner)? {
                return Ok(ended);
            }
        }
        let (ran, caught) = match planted_run(&run.command, runner)? {
            None => return Ok(Ended::Stopped),
            Some(Err(said)) => return Ok(Ended::Trial(Trial::not_run(said))),
            Some(Ok(ran)) => match Judged::of(ran) {
                Judged::Passed(ran) => {
                    last = Some(ran);
                    continue;
                }
                Judged::Not(verdict, ran) => {
                    return Ok(Ended::Trial(Trial::of(verdict, ran, Vec::new())));
                }
                Judged::Caught(ran, caught) => (ran, caught),
            },
        };
        if ran.names_tests() && !plant.meant.is_empty() {
            if caught
                .iter()
                .any(|name| report::is_meant(name, &plant.meant))
            {
                planting.restore()?;
                return Ok(confirmed(
                    clean.measure(&run.command, 1, runner)?,
                    ran,
                    caught,
                ));
            }
            let mut named = others.take().map(|(_, named)| named).unwrap_or_default();
            for name in caught {
                if !named.contains(&name) {
                    named.push(name);
                }
            }
            others = Some((ran, named));
            continue;
        }
        // Nothing to attribute it by: once more with the plant, which must fail again.
        let again = match planted_run(&run.command, runner)? {
            None => return Ok(Ended::Stopped),
            Some(Err(said)) => return Ok(Ended::Trial(Trial::not_run(said))),
            Some(Ok(ran)) => Judged::of(ran),
        };
        planting.restore()?;
        if !matches!(again, Judged::Caught(..)) {
            return Ok(Ended::Trial(Trial::unstable(format!(
                "{} failed with the plant, then {}",
                run.command.join(" "),
                again.then()
            ))));
        }
        return Ok(confirmed(
            clean.measure(&run.command, 2, runner)?,
            ran,
            caught,
        ));
    }
    Ok(Ended::Trial(match (others, last) {
        (Some((ran, named)), _) => Trial::of(Verdict::MissedByMeant, ran, named),
        (None, Some(ran)) => Trial::of(Verdict::Missed, ran, Vec::new()),
        (None, None) => Trial::unapplied(format!("{}: it has no run", plant.name)),
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
    let by = match trial.verdict {
        Verdict::Caught | Verdict::MissedByMeant => report::caught_by(&trial.caught, &plant.meant),
        _ => String::new(),
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
        Verdict::MissedByMeant => {
            for name in &trial.caught {
                writeln!(out, "    failed: {name}")?;
            }
        }
        Verdict::DoesNotCompile | Verdict::NotBuilt => {
            writeln!(out, "{}", report::tail(&ran.output, 1500))?;
        }
        Verdict::Broken => writeln!(
            out,
            "    {}\n{}",
            ran.broken().unwrap_or_default(),
            report::tail(&ran.output, 800)
        )?,
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
        Verdict::DoesNotApply | Verdict::NotRun | Verdict::Unstable => {}
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
    let temp = std::env::temp_dir();
    let saved = tempfile::Builder::new()
        .prefix("cf-plants-")
        .tempdir_in(&temp)
        .map_err(Error::files("make a folder for the copies in", &temp))?
        .keep();
    writeln!(
        out,
        "copies of what is planted are kept in {}",
        saved.display()
    )?;
    let limit = family.limit.as_secs();
    let mut built = false;
    let mut clean = CleanRuns::default();
    let mut wrong = 0;
    let mut unstable = 0;
    for plant in plants {
        if runner.stopped() {
            return stopped(&saved, out);
        }
        let started = Instant::now();
        let Some(trial) = trial(plant, context, &saved, runner, &mut built, &mut clean)? else {
            return stopped(&saved, out);
        };
        say(out, plant, &trial, started, limit)?;
        if trial.verdict != Verdict::Caught {
            wrong += 1;
        }
        if trial.verdict == Verdict::Unstable {
            unstable += 1;
        }
        // Built again, the native `cf` the commands run on is another.
        if built && family.rebuild.is_some() {
            clean.forget();
        }
        match rebuild(family, runner, &mut built, out)? {
            Rebuilt::Done => {}
            Rebuilt::Failed => wrong += 1,
            Rebuilt::Stopped => return stopped(&saved, out),
        }
    }
    let _ = fs::remove_dir_all(&saved);
    let caught = plants.len().saturating_sub(wrong);
    writeln!(out, "{}", report::totals(caught, plants.len(), unstable))?;
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
