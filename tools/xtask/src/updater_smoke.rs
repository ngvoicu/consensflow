//! The update path, proven end to end on the packaged app (landing S12): an
//! installed app is given this checkout's app as an update, by its own
//! updater, from a feed this run serves. This checkout's app ships no Node, and
//! the update goes to an app of each release that did:
//!
//! - the flip release, the release before this one: its daemon is the native
//!   `cf` and its terminal command names the `cf` of its bundle; and the same
//!   from a home that took its way back to Node (a `use-node` file), whose
//!   daemon is Node's;
//! - the bridge (`v3.0.0-alpha.81`), for a user who skips the flip: its daemon is
//!   Node's and its terminal command names Node.
//!
//! Every case of [`cases`] runs for each: the update itself, the two updates that
//! are refused, and an app replaced by hand. (The flip is the newest release tagged
//! before the one under test. Where that ships no Node, as `v3.0.0-alpha.83` does,
//! there is no way back to take and that case is skipped for it;
//! `--flip-ref v3.0.0-alpha.82` names the last that does.)
//!
//! ```text
//! cargo xtask smoke-updater                     # builds the apps, runs every case for both releases
//! cargo xtask smoke-updater --from flip         # the flip release alone
//! cargo xtask smoke-updater --from-app A --from-release flip --to-app B   # takes built apps
//! cargo xtask smoke-updater --only refused      # the cases whose names hold a word
//! ```
//!
//! What it builds (macOS, offline): each installed release exported from its tag
//! (the flip's is the newest tag after the bridge's in this checkout's history,
//! and `--flip-ref` names another tag or a commit), and this checkout as the
//! update, which the build gives the version after the newest installed one's.
//! All are built with this run's updater public key and signed ad hoc; the key
//! pair is made for the run (`tauri signer generate`) and goes with the run's
//! folder, and the product's key and every Apple certificate stay where they are,
//! unread (`signing.rs`). The apps taken as built paths verify against the run's
//! key all the same: the packaged self-test hands the app the public key it is to
//! verify with, so a build kept from another run takes this run's signatures.
//!
//! Options (a relative path is from the checkout's root):
//!
//! - `--from <releases>`: the installed releases to run, `bridge` and `flip` (comma apart; both)
//! - `--from-app <path>`: the installed app, a built ConsensFlow.app (one release: `--from-release`)
//! - `--from-release <name>`: which release `--from-app` is (flip)
//! - `--to-app <path>`: the update, a built ConsensFlow.app
//! - `--bridge <dir>`: a checkout of the bridge to build from (else its tag is exported)
//! - `--flip <dir>`: a checkout of the flip release to build from (else its tag is exported)
//! - `--flip-ref <ref>`: the tag or commit of the flip release to export (else the newest after the bridge)
//! - `--cache <dir>`: where the builds are kept (`app/src-tauri/target/updater-smoke`)
//! - `--reuse`: take the apps a run kept in the cache, and build only what is not there
//! - `--build-only`: build, say where, and run nothing
//! - `--export-bridge`: export the bridge's source from its tag into the cache, say where, and stop
//! - `--only <words>`: run the cases whose names hold a word (comma apart)
//! - `--machines <dir>`: make each case's machine in this folder, not the system's
//! - `--timeout <ms>`: how long each wait of a case is given (180000)
//! - `--keep`: keep the run's folder, and each case's machine
//!
//! The modules are the ones the Node script this replaces had, one each:
//! `build` (the apps, exported from their tags and built the way each builds),
//! `bundle` (what a built app is, its seal, its archive and the bundles that are
//! refused), `feed` (the signed update, served over HTTPS), `signing` (the run's
//! key), `sandbox` (a case's machine), `app` (the packaged app, started on a
//! FIFO), `processes` (what the process table says, and the waits), `evidence`
//! (what proves an app and its daemon are up and hold the ledger), `ledger`
//! (the ledger read from outside), `launchers` (the terminal's command), `case`
//! (what every case shares), `cases` (the cases) and `versions`.

/// Says a check of the smoke holds, or returns the words that say what was seen
/// instead: `ensure!(condition, "what was seen: {value}")`. The checks are the
/// proof, and what they say is what whoever reads a failed run has to go on.
macro_rules! ensure {
    ($condition:expr, $($said:tt)+) => {
        if !$condition {
            return Err($crate::updater_smoke::Error::new(format!($($said)+)));
        }
    };
}

/// The words a program is run with, each made an `OsString` as it is (a path
/// stays whole, whatever is in it): `args!["--verify", &path]`.
macro_rules! args {
    ($($arg:expr),* $(,)?) => {
        [$(::std::ffi::OsString::from($arg)),*]
    };
}

mod app;
mod build;
mod bundle;
mod case;
mod cases;
mod evidence;
mod feed;
mod launchers;
mod ledger;
mod options;
mod processes;
mod sandbox;
mod say;
mod signing;
#[cfg(test)]
mod testing;
mod versions;

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::time::Duration;

use cf_release::version::source_version;

use crate::context::Context;
use crate::dispatch::{Console, Failure};
use crate::process;
use build::{build_app, export_release, flip_release, Build, Release, BRIDGE_TAG};
use bundle::{copy_bundle, plist_value, remove_all};
use case::{Given, Inputs};
use options::Options;
pub use options::USAGE;
use processes::Waits;
use say::{Line, Say};
use signing::generate_key;
use versions::update_version;

/// How long each wait of a case is given, unless `--timeout` says.
const WAIT: Duration = Duration::from_millis(180_000);

/// Why a step of the smoke did not hold: what was seen, in words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

/// What a step of the smoke answers.
pub type Result<T = ()> = std::result::Result<T, Error>;

impl Error {
    /// An error that says `said`.
    pub fn new(said: impl Into<String>) -> Self {
        Self(said.into())
    }

    /// With `more` after what it said.
    #[must_use]
    pub fn then(mut self, more: &str) -> Self {
        self.0.push_str(more);
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<process::Failure> for Error {
    fn from(failure: process::Failure) -> Self {
        Self::new(failure.to_string())
    }
}

/// How a failed file operation is told: what was being done, and to which path.
pub fn files(what: &'static str, path: &Path) -> impl FnOnce(io::Error) -> Error {
    let path = path.to_path_buf();
    move |cause| Error::new(format!("could not {what} {}: {cause}", path.display()))
}

/// `cargo xtask smoke-updater`: builds the apps and runs every case for each
/// release asked for, and answers 0 when all passed.
pub fn run(
    context: &Context,
    args: &[OsString],
    console: &mut Console,
) -> std::result::Result<i32, Failure> {
    let options = Options::read(args).map_err(Failure::Usage)?;
    if !cfg!(target_os = "macos") {
        writeln!(
            console.err,
            "smoke:updater: the update path is macOS bundles and codesign"
        )?;
        return Ok(1);
    }
    let releases = options.releases().map_err(Failure::Usage)?;
    let (say, lines) = Say::channel();
    // The run speaks from more than one thread; this one holds the streams, and
    // writes each line as it comes.
    let (outcome, written) = thread::scope(|scope| {
        let worker = scope.spawn(|| smoke(context, &options, &releases, &say));
        let mut written = Ok(());
        loop {
            match lines.recv_timeout(Duration::from_millis(50)) {
                Ok(line) => written = written.and_then(|()| write_line(console, &line)),
                Err(RecvTimeoutError::Timeout) if !worker.is_finished() => {}
                Err(_) => break,
            }
        }
        for line in lines.try_iter() {
            written = written.and_then(|()| write_line(console, &line));
        }
        (worker.join(), written)
    });
    written?;
    match outcome {
        Ok(Ok(status)) => Ok(status),
        Ok(Err(cause)) => {
            writeln!(console.err, "smoke:updater: {cause}")?;
            Ok(1)
        }
        Err(panic) => {
            let said = panic
                .downcast_ref::<&str>()
                .map(|text| (*text).to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "no word of why".to_string());
            writeln!(
                console.err,
                "smoke:updater: the smoke stopped on a panic: {said}"
            )?;
            Ok(1)
        }
    }
}

fn write_line(console: &mut Console, line: &Line) -> io::Result<()> {
    match line {
        Line::Out(text) => writeln!(console.out, "{text}"),
        Line::Err(text) => writeln!(console.err, "{text}"),
    }
}

/// Says a line of the driver's own.
fn talk(say: &Say, text: impl AsRef<str>) {
    say.out(format!("smoke:updater: {}", text.as_ref()));
}

/// The smoke itself: its apps built or taken, then each release's cases.
fn smoke(context: &Context, options: &Options, releases: &[Release], say: &Say) -> Result<i32> {
    let root = &context.root;
    let in_the_checkout = |path: &Path| root.join(path);
    let cache = options.cache.as_deref().map_or_else(
        || context.path("app/src-tauri/target/updater-smoke"),
        in_the_checkout,
    );
    fs::create_dir_all(&cache).map_err(files("make", &cache))?;
    if options.export_bridge {
        let source = export_release(root, &cache.join("bridge-source"), BRIDGE_TAG, &context.env)?;
        talk(
            say,
            format!("the bridge's source is in {}", source.display()),
        );
        return Ok(0);
    }
    let real = fs::canonicalize(&cache).map_err(files("find", &cache))?;
    let folder = tempfile::Builder::new()
        .prefix("run-")
        .tempdir_in(&real)
        .map_err(files("make a folder in", &real))?
        .keep();
    let outcome = smoke_in(context, options, releases, &cache, &folder, say);
    if !options.keep {
        remove_all(&folder)?;
    }
    outcome
}

/// What the smoke does in the run's `folder`, in the `cache` the builds are kept in.
fn smoke_in(
    context: &Context,
    options: &Options,
    releases: &[Release],
    cache: &Path,
    folder: &Path,
    say: &Say,
) -> Result<i32> {
    let (root, env) = (&context.root, &context.env);
    let in_the_checkout = |path: &Path| root.join(path);
    let key = generate_key(root, &folder.join("keys"), env)?;
    talk(
        say,
        format!(
            "this run's updater key is {}",
            key.public_key_file.display()
        ),
    );

    let kept = |name: &str| cache.join(name).join("ConsensFlow.app");
    let take_kept = |name: &str| (options.reuse && kept(name).exists()).then(|| kept(name));

    // The source of `release` to build: the checkout it was named, else its tag exported into the cache.
    let source_of = |release: Release| -> Result<PathBuf> {
        let named = match release {
            Release::Bridge => &options.bridge,
            Release::Flip => &options.flip,
        };
        if let Some(named) = named {
            return Ok(in_the_checkout(named));
        }
        let tag = match (release, &options.flip_ref) {
            (Release::Bridge, _) => BRIDGE_TAG.to_string(),
            (Release::Flip, Some(named)) => named.clone(),
            (Release::Flip, None) => flip_release(root, env)?,
        };
        talk(say, format!("the {} release is {tag}", release.name()));
        export_release(
            root,
            &cache.join(format!("{}-source", release.name())),
            &tag,
            env,
        )
    };

    // The installed app of `release`: the one named, one a run kept, or one built from its source.
    let installed_of = |release: Release| -> Result<PathBuf> {
        if let Some(named) = &options.from_app {
            return Ok(in_the_checkout(named));
        }
        if let Some(found) = take_kept(release.name()) {
            return Ok(found);
        }
        let checkout = source_of(release)?;
        talk(
            say,
            format!(
                "building the installed {} app from {}",
                release.name(),
                checkout.display()
            ),
        );
        let built = build_app(
            &Build {
                checkout: &checkout,
                work: folder,
                public_key: &key.public_key,
                version: None,
            },
            env,
        )?;
        copy_bundle(&built, &kept(release.name()), env)
    };
    let mut installed = Vec::new();
    for release in releases {
        installed.push((*release, installed_of(*release)?));
    }
    for (release, app) in &installed {
        talk(
            say,
            format!("installed {} app: {}", release.name(), app.display()),
        );
    }

    let update = match (&options.to_app, take_kept("update")) {
        (Some(named), _) => in_the_checkout(named),
        (None, Some(found)) => found,
        (None, None) => {
            let versions = installed
                .iter()
                .map(|(_, app)| plist_value(app, "CFBundleShortVersionString"))
                .collect::<Result<Vec<_>>>()?;
            let versions: Vec<&str> = versions.iter().map(String::as_str).collect();
            let own = source_version(root).map_err(|cause| Error::new(cause.to_string()))?;
            let version = update_version(&own, &versions)?;
            talk(
                say,
                format!("building the update from {} as {version}", root.display()),
            );
            let built = build_app(
                &Build {
                    checkout: root,
                    work: folder,
                    public_key: &key.public_key,
                    version: Some(&version),
                },
                env,
            )?;
            copy_bundle(&built, &kept("update"), env)?
        }
    };
    talk(say, format!("update: {}", update.display()));
    if options.build_only {
        return Ok(0);
    }

    let machines = options
        .machines
        .as_deref()
        .map_or_else(std::env::temp_dir, in_the_checkout);
    let waits = Waits::new(options.timeout.unwrap_or(WAIT));
    let mut results = Vec::new();
    for (release, app) in &installed {
        talk(
            say,
            format!("== the update from the {} release", release.name()),
        );
        let passed = match Inputs::load(&Given {
            from_app: app,
            release: *release,
            to_app: &update,
            key: &key,
            keep: options.keep,
            waits,
            machines: &machines,
            checkout: root,
            env,
        }) {
            Ok(inputs) => {
                let tally = cases::run_cases(&inputs, &options.only, say);
                talk(
                    say,
                    format!(
                        "{}: {} passed, {} failed, {} skipped",
                        release.name(),
                        tally.passed,
                        tally.failed,
                        tally.skipped
                    ),
                );
                tally.failed == 0
            }
            Err(cause) => {
                talk(
                    say,
                    format!("the inputs are not what the update needs: {cause}"),
                );
                false
            }
        };
        results.push((*release, passed));
    }
    let said: Vec<String> = results
        .iter()
        .map(|(release, passed)| {
            format!(
                "{}: {}",
                release.name(),
                if *passed { "passed" } else { "FAILED" }
            )
        })
        .collect();
    talk(say, said.join("; "));
    Ok(i32::from(!results.iter().all(|(_, passed)| *passed)))
}

#[cfg(test)]
mod tests;
