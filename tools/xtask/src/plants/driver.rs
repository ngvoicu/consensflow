//! One plant at a time: copied out of the checkout, planted, tried by its runs,
//! and put back.
//!
//! Every file a plant touches is first copied to a folder outside the checkout,
//! and is put back from that copy, byte for byte, whatever the runs came to: an
//! error, a panic and a stop (which [`Runner::stopped`] and a command ended by one
//! tell) all put the files back. Putting back makes each file as it was, whatever
//! came of the others (each that could not be is said), and is done again at the
//! end of the plant: what a run without the plant wrote into a file it touches is
//! put back too. Only a kill past that leaves the copies where they are, in the
//! folder the run says first. The plants are never run in parallel: the sources
//! are changed under them.
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
//! them, and is one test (a name that stands for two tests registered, whatever came
//! of each, is neither), and the same command passes once the plant is put back,
//! which is run once for the run of the plants; an error outside every test beside it
//! is said with the catch. A failure that names only other tests goes on to the
//! plant's next run, and where none names the meant test the plant is
//! MISSED-BY-MEANT, said with the tests that failed. A run that has a reporter and
//! fails with no test failing (one cancelled, aborted, a test file that failed whole)
//! catches nothing: it is BROKEN, as is one whose reporter wrote no report it could
//! read, whatever its status. Only a failure of a command that has no reporter (one
//! that says no more than its status, as the release smokes do) is the run's own
//! catch, which nothing can attribute, and it is confirmed: the run is run so again
//! and must fail again, and the command must pass twice without the plant, each run
//! ending in its time.
//!
//! Every run of a plant is tried until one catches it: a run that tells nothing of
//! it is said for itself, and the next is run with the plant in. A command that
//! fails without the plant is run again once; failing twice (a program that is not
//! there, a suite that cannot start, a test target that is gone), it tells nothing
//! of any plant: NOT RUN, a bug no test was there to see is no catch. A command
//! that passes once without the plant and fails once, or a run that fails with it
//! once only, is UNSTABLE, neither caught nor missed. A run that could not load a
//! module of its own is BROKEN. A run whose tests build the native `cf` and the
//! pane host (the rig's) is preceded by their release builds from the plant, which
//! install nothing (`preflight`): a plant that breaks one DOES NOT COMPILE, where
//! cargo's own status says the build failed; a build that ends otherwise is NOT
//! BUILT, or HUNG where its time ran out. A run that did not build, or was BROKEN,
//! is run once the plant is put back, and is NOT RUN where it fails so too. Where no
//! run catches the plant, it ends as the first run that told nothing did, else
//! MISSED-BY-MEANT, else MISSED; all of these are counted as not caught, and told
//! with what they said.

mod confirm;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use self::confirm::{unstarted, Clean, CleanRuns, Judged};
use super::kit::{Family, Plant, Planted};
use super::report::{self, Verdict};
use super::runs::{Execution, Ran, Runner, CF_E2E_PROGRAMS};
use super::Error;
use crate::context::Context;
use crate::dispatch::Failure;

/// The status of a run that was stopped, as a program ends that a signal stops.
const STOPPED: i32 = 130;

/// The files of a plant in the checkout while its runs go, and how they go back.
struct Planting {
    copies: Vec<Copy>,
}

struct Copy {
    path: PathBuf,
    copy: PathBuf,
    original: Vec<u8>,
    /// The text it holds while planted.
    planted: String,
}

impl Copy {
    /// Writes the file's bytes from its copy where it is not as it was before the
    /// plant (one that is is left as it is, its times too), and says if it is not as it
    /// was once written; one that cannot be is said with where its copy is.
    fn put_back(&self) -> Result<(), Error> {
        if fs::read(&self.path).is_ok_and(|now| now == self.original) {
            return Ok(());
        }
        let planted = |cause| Error::StillPlanted {
            path: self.path.clone(),
            copy: self.copy.clone(),
            cause,
        };
        let saved = fs::read(&self.copy).map_err(planted)?;
        fs::write(&self.path, saved).map_err(planted)?;
        if fs::read(&self.path).map_err(planted)? != self.original {
            return Err(Error::NotRestored {
                path: self.path.clone(),
                copy: self.copy.clone(),
            });
        }
        Ok(())
    }
}

impl Planting {
    /// Copies each file of `files` into `saved` and writes the plant in its place. Where
    /// a write fails, what was planted is put back (`failed`), and a file that cannot be is
    /// named with its copy, before the write's error (Calliope's look at round 8: a full
    /// disk fails both).
    fn make(saved: &Path, files: Vec<Planted>) -> Result<Self, Error> {
        let mut planting = Self { copies: Vec::new() };
        match planting.plant(saved, files) {
            Ok(()) => Ok(planting),
            Err(cause) => Err(planting.failed(cause)),
        }
    }

    /// `make`'s writes, each copy before its plant.
    fn plant(&mut self, saved: &Path, files: Vec<Planted>) -> Result<(), Error> {
        for (index, file) in files.into_iter().enumerate() {
            let copy = saved.join(format!("{index}-{}", file.file.replace('/', "_")));
            fs::write(&copy, &file.original).map_err(Error::files("write a copy at", &copy))?;
            // From here the file is put back, even if the write is what fails.
            self.copies.push(Copy {
                path: file.path.clone(),
                copy,
                original: file.original,
                planted: file.text.clone(),
            });
            fs::write(&file.path, file.text).map_err(Error::files("write", &file.path))?;
        }
        Ok(())
    }

    /// Writes the plant again in each file where it is not, for the runs after one
    /// whose command was run without it.
    fn replant(&self) -> Result<(), Error> {
        for copy in &self.copies {
            if fs::read(&copy.path).is_ok_and(|now| now == copy.planted.as_bytes()) {
                continue;
            }
            fs::write(&copy.path, &copy.planted).map_err(Error::files("write", &copy.path))?;
        }
        Ok(())
    }

    /// Puts every file back from its copy where it is not as it was before the
    /// plant, whatever made it so (the plant, or what a run wrote, a run without the
    /// plant too), each whatever came of another; and says each that could not be.
    fn restore(&self) -> Result<(), Error> {
        let mut unrestored: Vec<Error> = self
            .copies
            .iter()
            .filter_map(|copy| copy.put_back().err())
            .collect();
        match unrestored.len() {
            0 => Ok(()),
            1 => Err(unrestored.remove(0)),
            _ => Err(Error::Unrestored(unrestored)),
        }
    }
}

impl Planting {
    /// `ended`, what came of the plant's trial or of a part of it, once every file is
    /// put back (`restore`), which is tried whatever it came to, an error too (a run
    /// whose program left a process running, both reviewers' look at round 7). A file
    /// that cannot be put back is never left unsaid: the error names each one still
    /// planted and where its copy is, before the error the trial ended in, if it did.
    fn ended<T>(&self, ended: Result<T, Error>) -> Result<T, Error> {
        match ended {
            Ok(ended) => self.restore().map(|()| ended),
            Err(cause) => Err(self.failed(cause)),
        }
    }

    /// `ended` where it came to the error `cause`: the error the run ends with, once
    /// every file is put back as far as it can be.
    fn failed(&self, cause: Error) -> Error {
        match self.restore() {
            Ok(()) => cause,
            // What ended it was that files could not be put back: they are said again,
            // as they are now.
            Err(unrestored) if cause.unrestored() => unrestored,
            Err(unrestored) => Error::PlantedAfter {
                unrestored: Box::new(unrestored),
                cause: Box::new(cause),
            },
        }
    }
}

impl Drop for Planting {
    /// What is not put back yet (a panic unwinding, which has its own words; a file that
    /// could not be put back before, which was said): put back, as well as can be.
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

/// How a plant ended, and what the last run said.
struct Trial {
    verdict: Verdict,
    ran: Option<Ran>,
    /// The tests that failed, or what else caught it.
    caught: Vec<String>,
    /// Why it did not apply, or why its run told nothing of it.
    said: String,
    /// What else is to be said of it: that no run ran a test of the name it was meant
    /// for, or that the name stands for more than one test, what broke beside a catch.
    notes: Vec<String>,
    /// The other runs that told nothing of it, each as it ended.
    also: Vec<Trial>,
}

impl Trial {
    fn of(verdict: Verdict, ran: Ran, caught: Vec<String>) -> Self {
        Self {
            verdict,
            ran: Some(ran),
            caught,
            said: String::new(),
            notes: Vec::new(),
            also: Vec::new(),
        }
    }

    fn unapplied(said: String) -> Self {
        Self {
            verdict: Verdict::DoesNotApply,
            ran: None,
            caught: Vec::new(),
            said,
            notes: Vec::new(),
            also: Vec::new(),
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
    Trial(Box<Trial>),
}

/// Plants `plant`, tries its runs in order until one decides (`tried`), and puts it
/// back. `clean` is how each command does without a plant, as far as it was run so,
/// and is forgotten as soon as the plant has a `prepare` to run, which may change
/// what the commands run on however it ends. `built` is made true where a run that
/// builds is about to be run. `None` is a stop.
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
    if !plant.prepare.is_empty() {
        clean.forget();
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
    // The state before the plant is the one its runs are known without it in.
    let Some(unplanted) = runner.state() else {
        return Ok(None);
    };
    let planting = Planting::make(saved, files)?;
    let ended = tried(plant, runner, built, clean, &unplanted, &planting);
    // Whatever a run without the plant wrote into its files since they were put back,
    // and the plant itself where the trial ended in an error.
    match planting.ended(ended)? {
        Ended::Stopped => Ok(None),
        Ended::Trial(trial) => Ok(Some(*trial)),
    }
}

/// The builds before a run whose tests build the native `cf`, with the plant in: the
/// release builds of `cf` and the pane host that the rig makes (`CF_E2E_PROGRAMS`),
/// which put nothing in `bin/` (that is `build-cf`'s). `None` where they built, else
/// how the trial ends, by the first that did not.
fn preflight(runner: &mut dyn Runner) -> Result<Option<Ended>, Error> {
    for build in CF_E2E_PROGRAMS {
        let words: Vec<String> = build.iter().map(|word| (*word).to_owned()).collect();
        let ran = match planted_run(&words, runner)? {
            None => return Ok(Some(Ended::Stopped)),
            Some(Err(said)) => return Ok(Some(Ended::Trial(Box::new(Trial::not_run(said))))),
            Some(Ok(ran)) => ran,
        };
        let verdict = if ran.hung {
            Verdict::Hung
        } else if ran.build_finished() == Some(false) {
            Verdict::DoesNotCompile
        } else if ran.code == Some(0) {
            continue;
        } else {
            Verdict::NotBuilt
        };
        return Ok(Some(Ended::Trial(Box::new(Trial::of(
            verdict,
            ran,
            Vec::new(),
        )))));
    }
    Ok(None)
}

/// What a run that told nothing of the plant ends as, by how its command did without
/// the plant: one that fails so too tells nothing of any plant, NOT RUN.
fn unconfirmed(clean: Clean) -> Option<Trial> {
    match clean {
        Clean::Passes => None,
        Clean::Fails(said) => Some(Trial::not_run(said)),
        Clean::Unstable(said) => Some(Trial::unstable(said)),
    }
}

/// A run of a plant that told nothing of it, and the command to run without the plant
/// where how it does so tells what it is: a run that did not build, or was BROKEN.
struct Told {
    trial: Trial,
    unsure: Option<Vec<String>>,
}

impl Told {
    /// What `command` came to, with the plant in, where it told nothing of it.
    fn of(trial: Trial, command: &[String]) -> Self {
        let unsure = matches!(trial.verdict, Verdict::DoesNotCompile | Verdict::Broken)
            .then(|| command.to_vec());
        Self { trial, unsure }
    }

    fn sure(trial: Trial) -> Self {
        Self {
            trial,
            unsure: None,
        }
    }
}

/// The runs of a plant that told nothing of it (`told`), each as it is to be said once
/// the plant is put back: a run that did not build, or was BROKEN, is NOT RUN where its
/// command fails without the plant too (a test target that is gone, a module that is
/// not there), and UNSTABLE where it fails once in two. `None` is a stop.
fn settled(
    told: Vec<Told>,
    clean: &mut CleanRuns,
    runner: &mut dyn Runner,
    unplanted: &str,
) -> Result<Option<Vec<Trial>>, Error> {
    // Where a run with the plant changed the checkout, how a command does without it
    // would be told of another.
    let moved = if told.iter().any(|told| told.unsure.is_some()) {
        let Some(state) = runner.state() else {
            return Ok(None);
        };
        state != unplanted
    } else {
        false
    };
    let mut settled = Vec::new();
    for Told { trial, unsure } in told {
        let Some(command) = unsure.filter(|_| !moved) else {
            settled.push(trial);
            continue;
        };
        let Some(clean) = clean.measure(&command, 1, runner)? else {
            return Ok(None);
        };
        settled.push(unconfirmed(clean).unwrap_or(trial));
    }
    Ok(Some(settled))
}

/// Where the checkout, its plant put back, is not as it was before the plant: a run
/// with the plant changed it (a fixture it mended, a file it wrote), and how the
/// command does without the plant would be told of another checkout. A stop heard
/// while the state is asked ends the trial.
fn moved(runner: &mut dyn Runner, unplanted: &str, command: &[String]) -> Option<Ended> {
    let Some(state) = runner.state() else {
        return Some(Ended::Stopped);
    };
    (state != unplanted).then(|| {
        Ended::Trial(Box::new(Trial::unstable(format!(
            "{} changed the checkout: once the plant is put back, it is not as it was before it",
            command.join(" ")
        ))))
    })
}

/// The runs of `plant`, planted, until one catches it; `planting` is put back before a
/// command is run without the plant, in the state of the checkout before the plant
/// (`unplanted`) alone, and planted again for the next run. A run that tells nothing
/// of the plant (a command known to fail without it in that state, one that cannot be
/// started, does not build, is BROKEN or HUNG, a catch that its command does not
/// confirm) is kept to be said, and the plant's next run is tried; where none catches
/// it, the plant ends as the first of them did.
fn tried(
    plant: &Plant,
    runner: &mut dyn Runner,
    built: &mut bool,
    clean: &mut CleanRuns,
    unplanted: &str,
    planting: &Planting,
) -> Result<Ended, Error> {
    let mut last = None;
    // The last run that failed naming only tests the plant was not meant for, and the
    // tests every such run named.
    let mut others: Option<(Ran, Vec<String>)> = None;
    // The runs that told nothing of the plant, in turn.
    let mut told: Vec<Told> = Vec::new();
    // Why a meant name that a run's tests failed under was not taken for a catch.
    let mut doubts: Vec<String> = Vec::new();
    // Every test a run's report said it ran, where any run's report said.
    let mut tests_ran: Option<Vec<String>> = None;
    for run in &plant.runs {
        if let Some(said) = clean.fails(&run.command, unplanted) {
            told.push(Told::sure(Trial::not_run(said)));
            continue;
        }
        // An earlier run's command may have been run without it.
        planting.replant()?;
        let builds = run.builds || plant.builds;
        *built = *built || builds;
        if builds {
            match preflight(runner)? {
                None => {}
                Some(Ended::Stopped) => return Ok(Ended::Stopped),
                Some(Ended::Trial(unbuilt)) => {
                    told.push(Told::sure(*unbuilt));
                    continue;
                }
            }
        }
        let ran = match planted_run(&run.command, runner)? {
            None => return Ok(Ended::Stopped),
            Some(Err(said)) => {
                told.push(Told::sure(Trial::not_run(said)));
                continue;
            }
            Some(Ok(ran)) => ran,
        };
        if matches!(ran.report, Some(Ok(_))) {
            let names = ran.tests().iter().map(|test| test.name.clone());
            tests_ran.get_or_insert_with(Vec::new).extend(names);
        }
        let (measured, ran, caught, beside) = match Judged::of(ran, &plant.meant) {
            Judged::Passed(ran) => {
                last = Some(ran);
                continue;
            }
            Judged::Not(verdict, ran) => {
                told.push(Told::of(Trial::of(verdict, ran, Vec::new()), &run.command));
                continue;
            }
            Judged::Others { ran, caught, doubt } => {
                doubts.extend(doubt);
                let mut named = others.take().map(|(_, named)| named).unwrap_or_default();
                for name in caught {
                    if !named.contains(&name) {
                        named.push(name);
                    }
                }
                others = Some((ran, named));
                continue;
            }
            Judged::Meant {
                ran,
                caught,
                beside,
            } => {
                planting.restore()?;
                if let Some(ended) = moved(runner, unplanted, &run.command) {
                    return Ok(ended);
                }
                (clean.measure(&run.command, 1, runner)?, ran, caught, beside)
            }
            // Nothing to attribute it by: once more with the plant, which must fail again.
            Judged::Unnamed(ran, caught) => {
                let again = match planted_run(&run.command, runner)? {
                    None => return Ok(Ended::Stopped),
                    Some(Err(said)) => {
                        told.push(Told::sure(Trial::not_run(said)));
                        continue;
                    }
                    Some(Ok(again)) => Judged::of(again, &plant.meant),
                };
                if !again.failed() {
                    told.push(Told::sure(Trial::unstable(format!(
                        "{} failed with the plant, then {}",
                        run.command.join(" "),
                        again.then()
                    ))));
                    continue;
                }
                planting.restore()?;
                if let Some(ended) = moved(runner, unplanted, &run.command) {
                    return Ok(ended);
                }
                (clean.measure(&run.command, 2, runner)?, ran, caught, None)
            }
        };
        let Some(measured) = measured else {
            return Ok(Ended::Stopped);
        };
        if let Some(unconfirmed) = unconfirmed(measured) {
            told.push(Told::sure(unconfirmed));
            continue;
        }
        let Some(told) = settled(told, clean, runner, unplanted)? else {
            return Ok(Ended::Stopped);
        };
        let mut trial = Trial::of(Verdict::Caught, ran, caught);
        trial.also = told;
        trial.notes = doubts;
        trial
            .notes
            .extend(beside.map(|beside| format!("beside it: {beside}")));
        return Ok(Ended::Trial(Box::new(trial)));
    }
    planting.restore()?;
    let Some(mut told) = settled(told, clean, runner, unplanted)? else {
        return Ok(Ended::Stopped);
    };
    let missed = match (others, last) {
        (Some((ran, named)), _) => Some(Trial::of(Verdict::MissedByMeant, ran, named)),
        (None, Some(ran)) => Some(Trial::of(Verdict::Missed, ran, Vec::new())),
        (None, None) => None,
    };
    let mut trial = if told.is_empty() {
        missed.unwrap_or_else(|| Trial::unapplied(format!("{}: it has no run", plant.name)))
    } else {
        // The first run that told nothing is the plant's end; a run that failed naming
        // other tests is said with the others.
        let mut first = told.remove(0);
        told.extend(missed.filter(|missed| missed.verdict == Verdict::MissedByMeant));
        first.also = told;
        first
    };
    trial.notes = doubts;
    // A meant test that no run ran: a name the table holds wrong, or a test that is gone.
    if let Some(ran) = tests_ran {
        if !ran.iter().any(|name| report::is_meant(name, &plant.meant)) {
            trial
                .notes
                .push(format!("no test named {} ran in its runs", plant.meant));
        }
    }
    Ok(Ended::Trial(Box::new(trial)))
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
    tell(out, trial, limit)?;
    for other in &trial.also {
        let Some(ran) = &other.ran else {
            writeln!(out, "    and {}: {}", other.verdict.label(), other.said)?;
            continue;
        };
        let words = ran.command.join(" ");
        writeln!(out, "    and {}: {words}", other.verdict.label())?;
        tell(out, other, limit)?;
    }
    for note in &trial.notes {
        writeln!(out, "    {note}")?;
    }
    Ok(())
}

/// What follows from how `trial` ended, after the line that says it.
fn tell(out: &mut dyn Write, trial: &Trial, limit: u64) -> Result<(), Failure> {
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
        Verdict::Hung if ran.overflowed => writeln!(
            out,
            "    {} wrote more than a run may to a stream, and was ended",
            ran.command.join(" ")
        )?,
        Verdict::Hung => writeln!(
            out,
            "    {} did not end in {limit} s",
            ran.command.join(" ")
        )?,
        Verdict::Missed => match &ran.report {
            Some(Ok(results)) => writeln!(
                out,
                "    {} tests ran, and none of them failed",
                results.tests.len()
            )?,
            _ => {
                for line in report::what_ran(&ran.output) {
                    writeln!(out, "    {line}")?;
                }
            }
        },
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

/// The system this runs on, as a plant's `platforms` names it.
const HERE: &str = std::env::consts::OS;

/// Plants each of `plants` in turn and says how each ended: the status is 0 where
/// every one run was caught. One for other systems than this one is said SKIPPED
/// (platform), and one whose runs need a harness the machine lacks SKIPPED (absent):
/// neither is run nor counted.
pub fn run(
    context: &Context,
    family: &Family,
    plants: &[&Plant],
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> Result<i32, Failure> {
    // A checkout git says nothing of has no state a command's run can be known in: a
    // run that changed it would go untold, and a clean pass taken from another state.
    // Refused before the run makes its folder, which it would leave empty.
    if let Some(root) = runner.gitless() {
        return Err(Error::NoCheckout { root }.into());
    }
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
    let mut clean = CleanRuns::default();
    let mut wrong = 0;
    let mut unstable = 0;
    let mut skipped = 0;
    let mut absent = 0;
    for plant in plants {
        if runner.stopped() {
            return stopped(&saved, out);
        }
        if !plant.runs_on(HERE) {
            skipped += 1;
            writeln!(
                out,
                "{}",
                report::skipped(&plant.name, &plant.platforms, HERE)
            )?;
            continue;
        }
        // Its runs would pass saying the harness is not there, and read as missed.
        if let Some(why) = plant
            .harnesses
            .iter()
            .find_map(|harness| runner.lacks(harness))
        {
            absent += 1;
            writeln!(out, "{}", report::absent(&plant.name, &why))?;
            continue;
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
    // Gone with the run only when every plant was caught. What a plant that was not leaves
    // of it (the home of a rig a hung run was ended in, the report of a case caught by
    // another test than the one meant) is in the run's temp folder, and nowhere else. It is
    // said before the totals, which stay the last line.
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
    let run = plants.len() - skipped - absent;
    let caught = run.saturating_sub(wrong);
    writeln!(
        out,
        "{}",
        report::totals(caught, run, unstable, skipped, absent)
    )?;
    Ok(i32::from(wrong != 0))
}

/// The first XML entity `meant` holds (`&quot;`, `&amp;`, `&#34;`), if it holds one:
/// a name copied from a report that escapes it, which is not the test's name.
fn entity(meant: &str) -> Option<&str> {
    meant.match_indices('&').find_map(|(at, _)| {
        let body = &meant[at + 1..];
        let body = &body[..body.find(';')?];
        let number = body
            .strip_prefix("#x")
            .or_else(|| body.strip_prefix('#'))
            .filter(|digits| !digits.is_empty());
        let entity = match number {
            Some(digits) => digits.chars().all(|digit| digit.is_ascii_hexdigit()),
            None => !body.is_empty() && body.chars().all(|letter| letter.is_ascii_alphanumeric()),
        };
        entity.then(|| &meant[at..at + body.len() + 2])
    })
}

/// What `check` finds of one plant.
enum Fare {
    /// It applies to the sources as they are.
    Applies,
    /// It does not, for the reason `check` says.
    NotApplies(String),
    /// It is for another system: said, and not prepared or applied.
    Skipped,
    /// The run was stopped before it was checked.
    Stopped,
}

/// How `plant` fares against the sources as they are. A meant name that holds an XML
/// entity, and a run that cannot be run as it is written, are plants that do not apply
/// (they are refused before anything is prepared). A plant for another system is
/// skipped, and any other has what it needs first run and is then planted in memory
/// only.
fn fare(context: &Context, plant: &Plant, runner: &mut dyn Runner) -> Result<Fare, Failure> {
    if let Some(entity) = entity(&plant.meant) {
        return Ok(Fare::NotApplies(format!(
            "{}: its meant name holds {entity}, a report's escape and no part of a test's name: write the name as the test does",
            plant.name
        )));
    }
    let refused = plant
        .runs
        .iter()
        .find_map(|run| super::runs::refused(&run.command, context));
    if let Some(why) = refused {
        return Ok(Fare::NotApplies(format!("{}: {why}", plant.name)));
    }
    // What it needs first may be of its own system, and what it plants of that one.
    if !plant.runs_on(HERE) {
        return Ok(Fare::Skipped);
    }
    Ok(match prepare(plant, runner)? {
        Prepared::Stopped => Fare::Stopped,
        Prepared::Failed(unready) => Fare::NotApplies(unready.to_string()),
        Prepared::Done => match plant.planted(context) {
            Ok(_) => Fare::Applies,
            Err(cause) => Fare::NotApplies(cause.to_string()),
        },
    })
}

/// Whether each of `plants` applies, which plants nothing: the status is 0 where every
/// one does, or does not and is listed in `family`'s `known_unapplied` (said as known,
/// and counted apart). A listed plant that applies again fails the check, so the list
/// is emptied as its plants are mended. The commands a plant needs first are run. A
/// meant name that holds an XML entity is refused, and so is a run that cannot be run as
/// it is written (an option of `cargo test` that nextest has no word for). A plant for
/// other systems is said SKIPPED (platform), its names checked (the table's data was),
/// nothing of it prepared or applied.
pub fn check(
    context: &Context,
    family: &Family,
    plants: &[&Plant],
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> Result<i32, Failure> {
    let (mut unapplied, mut known, mut stale, mut skipped) = (0, 0, 0, 0);
    for plant in plants {
        let listed = family.known_unapplied.contains(&plant.name);
        match fare(context, plant, runner)? {
            Fare::Stopped => return Ok(STOPPED),
            Fare::Skipped => {
                skipped += 1;
                writeln!(
                    out,
                    "{}",
                    report::skipped(&plant.name, &plant.platforms, HERE)
                )?;
            }
            Fare::Applies if listed => {
                stale += 1;
                writeln!(
                    out,
                    "{}: applies, so it is not known not to apply: take it off known_unapplied in kit.toml",
                    plant.name
                )?;
            }
            Fare::Applies => {}
            Fare::NotApplies(cause) if listed => {
                known += 1;
                writeln!(out, "known not to apply: {cause}")?;
            }
            Fare::NotApplies(cause) => {
                unapplied += 1;
                writeln!(out, "{cause}")?;
            }
        }
    }
    let checked = plants.len() - skipped;
    let mut said = if unapplied == 0 && known == 0 {
        format!("{checked} plants apply")
    } else {
        format!("{} of {checked} plants apply", checked - unapplied - known)
    };
    if known > 0 {
        said.push_str(&format!(", {known} known not to apply"));
    }
    if skipped > 0 {
        said.push_str(&format!(", {skipped} skipped (platform)"));
    }
    writeln!(out, "{said}")?;
    Ok(i32::from(unapplied + stale != 0))
}

#[cfg(test)]
mod tests;
