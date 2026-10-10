//! ConsensFlow Candidate (landing S11): this checkout built as another app and
//! installed beside the live one, which is proven untouched.
//!
//! The candidate is the same source with another identity
//! (`app/src-tauri/tauri.candidate.conf.json`): its own name, bundle identifier
//! and WebKit storage, and, because a build that is not the release chooses
//! `~/.consensflow-candidate` (`isolated_home` in `app/src-tauri/src/lib.rs`), its
//! own state, even when it is opened from Finder. It replaces only
//! `~/Applications/ConsensFlow Candidate.app`: the live app is `/Applications`'s.
//!
//! In order, and each refusal stops the run before anything is installed that was
//! not (the exit status is 1, and what it says starts `candidate:`):
//!
//! 1. the Candidate is not running from where it is installed;
//! 2. the live app's bundle and roster are fingerprinted, and the processes the
//!    live app runs as are noted;
//! 3. `npm run build -- --config tauri.candidate.conf.json`, in `app/`;
//! 4. the bundle built carries the Candidate's identity: one that kept the
//!    release's would share the live app's state root when opened from Finder;
//! 5. the packaged smoke ([`crate::smoke`]) runs on THAT bundle, with a throwaway
//!    home, and a candidate that fails it is never installed;
//! 6. the bundle is copied beside its place and put there, the one it replaces
//!    taken away after, and its seal verified;
//! 7. the Candidate starts with the live app's saved agents, once: its roster
//!    is its own after that;
//! 8. what the build was is written in `~/.consensflow-candidate/candidate-build.json`;
//! 9. the live app, its roster and its processes are as they were.
//!
//! It takes no arguments and is macOS's for now. The system it runs on is
//! [`system::System`], so that its steps are tested on a temporary tree, with a
//! system that is a script, and never on the real `/Applications` or homes.

mod canary;
mod system;
#[cfg(test)]
mod tests;

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cf_base::time::{iso, Clock, SystemClock};
use serde_json::json;

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::Invocation;
use crate::smoke;
use crate::updater_smoke::bundle::{plist_value, remove_all};
use crate::updater_smoke::processes::Row;
use canary::Canaries;
use system::{Real, System};

/// The Candidate's bundle, as the build and the install name it.
const NAME: &str = "ConsensFlow Candidate.app";

/// What the Candidate's bundle says it is, which the release's does not.
const IDENTIFIER: &str = "dev.ngvoicu.consensflow.candidate";

/// Where the live app is installed.
const APPLICATIONS: &str = "/Applications";

pub const COMMANDS: &[Command] = &[Command {
    words: &["candidate"],
    about: "Build this checkout as ConsensFlow Candidate and install it beside the live app",
    usage: "",
    run: Run::Native(run),
}];

/// Why the candidate was not built and installed: what, in words, as the run says it.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HOME is not set: the candidate is installed under it and keeps its state in it")]
    NoHome,
    #[error("quit ConsensFlow Candidate first — it is running from {}", .target.display())]
    Running { target: PathBuf },
    #[error("`npm run build` ended with status {status}: nothing was installed")]
    Build { status: i32 },
    #[error("{} does not carry {IDENTIFIER}", .built.display())]
    Identity { built: PathBuf },
    #[error("the packaged smoke failed; the candidate was not installed")]
    Smoke,
    #[error("`{step}` ended with status {status}")]
    Step { step: String, status: i32 },
    #[error("git {words} did not answer: {said}")]
    Git { words: &'static str, said: String },
    #[error("the live app at {} changed during the build — investigate", .0.display())]
    LiveAppChanged(PathBuf),
    #[error("{} changed during the build — expected only if you edited agents in the live app", .0.display())]
    RosterChanged(PathBuf),
    #[error("the live app (PID {0}) is no longer running")]
    LiveGone(u32),
    #[error("could not {action} {}: {source}", .path.display())]
    File {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    /// A bundle or the table of processes could not be read.
    #[error(transparent)]
    Inspecting(#[from] crate::updater_smoke::Error),
    /// A program could not be started.
    #[error(transparent)]
    Process(#[from] crate::process::Failure),
}

/// How a failed file operation is told: what was being done, and to which path.
fn file(action: &'static str, path: &Path) -> impl FnOnce(io::Error) -> Error {
    let path = path.to_path_buf();
    move |source| Error::File {
        action,
        path,
        source,
    }
}

/// Where everything of a candidate run is.
#[derive(Debug, Clone)]
struct Paths {
    /// The checkout, which is built and which `git` is asked of.
    repo: PathBuf,
    /// Its `app/`, where the build runs.
    app: PathBuf,
    /// What the build leaves.
    built: PathBuf,
    /// Where it is installed: `~/Applications`.
    target: PathBuf,
    /// The Candidate's own home: `~/.consensflow-candidate`.
    state: PathBuf,
    /// The live app, in the system's applications.
    live_app: PathBuf,
    /// The live app's roster.
    live_roster: PathBuf,
}

impl Paths {
    /// The paths of a run on the checkout `repo`, for the user whose home is
    /// `home`, beside the live app in `applications`.
    fn new(repo: &Path, home: &Path, applications: &Path) -> Self {
        let app = repo.join("app");
        let built = ["src-tauri", "target", "release", "bundle", "macos", NAME]
            .iter()
            .fold(app.clone(), |path, part| path.join(part));
        Self {
            repo: repo.to_path_buf(),
            built,
            app,
            target: home.join("Applications").join(NAME),
            state: home.join(".consensflow-candidate"),
            live_app: applications.join("ConsensFlow.app"),
            live_roster: home.join(".consensflow").join("agents.json"),
        }
    }

    /// The Tauri configuration that gives the build its identity.
    fn config(&self) -> PathBuf {
        self.app.join("src-tauri").join("tauri.candidate.conf.json")
    }

    /// What the live app runs as: the one program the Finder starts, whole.
    fn live_program(&self) -> String {
        program_in(&self.live_app, "app")
    }
}

/// What `ps` says of a program of the bundle `bundle` by its `name`: the whole
/// of its command, with `/` between the folders it is in.
fn program_in(bundle: &Path, name: &str) -> String {
    format!(
        "{}/{name}",
        bundle.join("Contents").join("MacOS").to_string_lossy()
    )
}

/// What a run does: where it is, and the two programs it hands the build and the smoke to.
struct Plan {
    paths: Paths,
    /// `npm run build -- --config tauri.candidate.conf.json`, in `app/`.
    build: Invocation,
    /// The packaged smoke, on the bundle that build leaves.
    smoke: Invocation,
}

/// The plan for the checkout `context` is, for the user whose `HOME` it names, beside the live app in `applications`.
fn plan(context: &Context, applications: &Path) -> Result<Plan, Error> {
    let home = context.env.path("HOME").ok_or(Error::NoHome)?;
    let paths = Paths::new(&context.root, home, applications);
    let build = Invocation::new("npm", &paths.app)
        .args(["run", "build", "--", "--config"])
        .arg(paths.config());
    let smoke = smoke::invocation(context, Some(&paths.built));
    Ok(Plan {
        paths,
        build,
        smoke,
    })
}

/// What a run did.
#[derive(Debug, PartialEq, Eq)]
struct Installed {
    target: PathBuf,
    version: String,
    state: PathBuf,
    /// The processes the live app was running as, which are running still.
    live: Vec<u32>,
}

impl Installed {
    /// What the run says when it is done.
    fn said(&self) -> String {
        let live = if self.live.is_empty() {
            String::new()
        } else {
            let pids: Vec<String> = self.live.iter().map(u32::to_string).collect();
            format!("; live PID {} running", pids.join(", "))
        };
        format!(
            "installed {} ({})\nstate: {}\nlive app and roster unchanged{live}\n",
            self.target.display(),
            self.version,
            self.state.display()
        )
    }
}

fn run(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    if !args.is_empty() {
        return Err(Failure::Usage("candidate takes no arguments".into()));
    }
    if !cfg!(target_os = "macos") {
        writeln!(
            console.err,
            "candidate: the candidate build is macOS-only for now"
        )?;
        return Ok(1);
    }
    let outcome = plan(context, Path::new(APPLICATIONS))
        .and_then(|plan| install(&plan, &mut Real::new(&context.env), &mut SystemClock));
    match outcome {
        Ok(installed) => {
            write!(console.out, "{}", installed.said())?;
            Ok(0)
        }
        Err(error) => {
            writeln!(console.err, "candidate: {error}")?;
            Ok(1)
        }
    }
}

/// The rows of `table` that run from inside the bundle `bundle`.
fn running_from<'a>(table: &'a [Row], bundle: &Path) -> Vec<&'a Row> {
    let prefix = program_in(bundle, "");
    table
        .iter()
        .filter(|row| row.command.starts_with(&prefix))
        .collect()
}

/// `path` with `suffix` after its name, beside it.
fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Runs `invocation` and refuses a status other than 0, naming the step.
fn step(system: &mut dyn System, invocation: &Invocation) -> Result<(), Error> {
    match system.run(invocation)? {
        0 => Ok(()),
        status => Err(Error::Step {
            step: invocation.display(),
            status,
        }),
    }
}

/// What `git` says to `words`, in the checkout.
fn git(system: &mut dyn System, repo: &Path, words: &'static str) -> Result<String, Error> {
    let asked = Invocation::new("git", repo).args(words.split(' '));
    let answered = system.capture(&asked)?;
    if answered.code != 0 {
        return Err(Error::Git {
            words,
            said: answered.stderr.trim().to_owned(),
        });
    }
    Ok(answered.stdout.trim().to_owned())
}

/// Builds, checks, installs and records the candidate by `plan` on `system`,
/// and proves the live app untouched.
fn install(
    plan: &Plan,
    system: &mut dyn System,
    clock: &mut dyn Clock,
) -> Result<Installed, Error> {
    let paths = &plan.paths;
    let table = system.table()?;
    if !running_from(&table, &paths.target).is_empty() {
        return Err(Error::Running {
            target: paths.target.clone(),
        });
    }
    let live_program = paths.live_program();
    let live: Vec<u32> = table
        .iter()
        .filter(|row| row.command == live_program)
        .map(|row| row.pid)
        .collect();
    let before = canary::take(&paths.live_app, &paths.live_roster)?;

    // The build says what it is doing itself.
    let built = system.run(&plan.build)?;
    if built != 0 {
        return Err(Error::Build { status: built });
    }
    // A bundle that kept the release identity would share the live app's state
    // root when opened from Finder: refuse it before it is installed anywhere.
    if plist_value(&paths.built, "CFBundleIdentifier")? != IDENTIFIER {
        return Err(Error::Identity {
            built: paths.built.clone(),
        });
    }
    // The packaged smoke runs THIS bundle with a throwaway home; a candidate that
    // fails it is never installed.
    if system.run(&plan.smoke)? != 0 {
        return Err(Error::Smoke);
    }

    put(system, paths)?;
    keep_the_roster(paths)?;
    let version = plist_value(&paths.target, "CFBundleShortVersionString")?;
    record(system, clock, paths, &version)?;

    prove_untouched(system, paths, &before, &live)?;
    Ok(Installed {
        target: paths.target.clone(),
        version,
        state: paths.state.clone(),
        live,
    })
}

/// Puts the bundle built where the Candidate is installed: copied beside its
/// place first, so that the place has the old one or the new and never half of
/// one, the old taken away once the new is there, and the seal verified.
fn put(system: &mut dyn System, paths: &Paths) -> Result<(), Error> {
    let (target, next, previous) = (
        &paths.target,
        beside(&paths.target, ".next"),
        beside(&paths.target, ".previous"),
    );
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(file("make", parent))?;
    }
    remove_all(&next)?;
    remove_all(&previous)?;
    let copy = Invocation::new("/usr/bin/ditto", &paths.repo)
        .arg(&paths.built)
        .arg(&next);
    step(system, &copy)?;
    if target.exists() {
        fs::rename(target, &previous).map_err(file("move aside", target))?;
    }
    fs::rename(&next, target).map_err(file("put in place", target))?;
    remove_all(&previous)?;
    let seal = Invocation::new("/usr/bin/codesign", &paths.repo)
        .args(["--verify", "--deep", "--strict"])
        .arg(target);
    step(system, &seal)
}

/// The candidate starts with the live app's saved agents; after that its roster is its own.
fn keep_the_roster(paths: &Paths) -> Result<(), Error> {
    make_private_folder(&paths.state)?;
    let roster = paths.state.join("agents.json");
    if !roster.exists() && paths.live_roster.exists() {
        fs::copy(&paths.live_roster, &roster).map_err(file("copy the roster to", &roster))?;
        make_private_file(&roster)?;
    }
    Ok(())
}

/// Writes what the build was, in the Candidate's home.
fn record(
    system: &mut dyn System,
    clock: &mut dyn Clock,
    paths: &Paths,
    version: &str,
) -> Result<(), Error> {
    let head = git(system, &paths.repo, "rev-parse HEAD")?;
    let uncommitted = git(system, &paths.repo, "status --porcelain")?
        .lines()
        .filter(|line| !line.is_empty())
        .count();
    let build = json!({
        "version": version,
        "app": paths.target.to_string_lossy(),
        "source": paths.repo.to_string_lossy(),
        "head": head,
        "uncommittedFiles": uncommitted,
        "builtAt": iso(clock.now_ms()),
    });
    let record = paths.state.join("candidate-build.json");
    // `{:#}` is the JSON written over lines, indented by two spaces, as the script wrote it.
    fs::write(&record, format!("{build:#}\n")).map_err(file("write", &record))
}

/// The live app, its roster and its processes are as they were.
fn prove_untouched(
    system: &mut dyn System,
    paths: &Paths,
    before: &Canaries,
    live: &[u32],
) -> Result<(), Error> {
    let after = canary::take(&paths.live_app, &paths.live_roster)?;
    if after.app != before.app {
        return Err(Error::LiveAppChanged(paths.live_app.clone()));
    }
    if after.roster != before.roster {
        return Err(Error::RosterChanged(paths.live_roster.clone()));
    }
    for pid in live {
        if !system.alive(*pid) {
            return Err(Error::LiveGone(*pid));
        }
    }
    Ok(())
}

/// Makes the folder `path` for its owner alone, and the ones above it that are not there.
fn make_private_folder(path: &Path) -> Result<(), Error> {
    let mut folder = fs::DirBuilder::new();
    folder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        folder.mode(0o700);
    }
    folder.create(path).map_err(file("make", path))
}

/// Makes the file `path` its owner's alone, where there are modes to give.
fn make_private_file(path: &Path) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(file("make private", path))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
