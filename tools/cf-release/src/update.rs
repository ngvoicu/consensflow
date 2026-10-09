//! `cf-release prepare-update`: the update feed's `latest.json` for one build.
//! Installed apps read it to learn that a newer app is there, and fetch the
//! archive it names; so it is written only for a build that is what it says it
//! is, and nothing in it comes from anywhere but the build:
//!
//! - the bundle's `Info.plist` and its own `cf` say one canonical version, and
//!   it is the version of the sources (`package.json`, `Cargo.toml` and
//!   `tauri.conf.json`, which [`crate::version`] holds to each other);
//! - the channel takes that version, the date is a time, the notes fit, and the
//!   signature has the shape the signer writes ([`signature`]);
//! - the archive is the bundle, path for path and byte for byte, named for the
//!   version, with nothing in it unpacking could use against the machine
//!   ([`archive`], [`tree`]).
//!
//! The first that fails ends it, in that order, the cheap checks before the
//! reading of an archive that is a hundred megabytes, and no file is written.
//! The words of a refusal are those of `app/scripts/prepare-update.mjs`, which
//! this replaced, with the detail it did not give (which path of the archive is
//! not the bundle's, and how) after them.

mod archive;
mod bundle;
mod channel;
mod date;
mod feed;
mod info;
mod signature;
mod tree;

use std::ffi::OsString;
use std::fmt::Display;
use std::path::PathBuf;

use cf_base::env::Env;

use crate::args::Flags;
use crate::cli::{Command, Console, Failure};
use crate::version;

use channel::Channel;

/// `cf-release prepare-update`.
pub const COMMAND: Command = Command {
    name: "prepare-update",
    about: "Write the update feed's latest.json for one build",
    usage: "--bundle APP --archive FILE --signature FILE --notes FILE --output FILE \
            --channel alpha|stable --date RFC3339 [--repo DIR]",
    run,
};

/// The flags it needs, in the order a missing one is asked for.
const REQUIRED: [&str; 7] = [
    "bundle",
    "archive",
    "signature",
    "notes",
    "output",
    "channel",
    "date",
];

/// Every flag it takes: the ones it needs, and the checkout (by default the folder it is run in).
const FLAGS: [&str; 8] = [
    "bundle",
    "archive",
    "signature",
    "notes",
    "output",
    "channel",
    "date",
    "repo",
];

/// What it was asked to do.
struct Options {
    repo: PathBuf,
    bundle: PathBuf,
    archive: PathBuf,
    signature: PathBuf,
    notes: PathBuf,
    output: PathBuf,
    channel: String,
    date: String,
}

impl Options {
    fn read(args: &[OsString]) -> Result<Self, Failure> {
        let flags = Flags::read(args, &FLAGS)?;
        for name in REQUIRED {
            flags.require(name)?;
        }
        let path = |name| flags.require(name).map(PathBuf::from);
        let text = |name| {
            flags
                .require(name)
                .map(|value| value.to_string_lossy().into_owned())
        };
        Ok(Self {
            repo: flags.repo()?,
            bundle: path("bundle")?,
            archive: path("archive")?,
            signature: path("signature")?,
            notes: path("notes")?,
            output: path("output")?,
            channel: text("channel")?,
            date: text("date")?,
        })
    }
}

fn failed(cause: impl Display) -> Failure {
    Failure::Failed(cause.to_string())
}

fn run(env: &Env, args: &[OsString], _console: &mut Console) -> Result<(), Failure> {
    let options = Options::read(args)?;
    let bundle = bundle::read(&options.bundle, env).map_err(failed)?;
    let source = version::source_version(&options.repo).map_err(failed)?;
    if source != bundle.version {
        return Err(Failure::Failed(format!(
            "source version {source} does not match bundle version {}",
            bundle.version
        )));
    }
    Channel::named(&options.channel)
        .and_then(|channel| channel.admits(&bundle.version))
        .map_err(failed)?;
    date::check(&options.date).map_err(failed)?;
    let notes = feed::notes(&options.notes).map_err(failed)?;
    let signature = signature::read(&options.signature).map_err(failed)?;
    let asset = archive::inspect(&options.archive, &bundle).map_err(failed)?;

    let entry = feed::render(&feed::Build {
        version: &bundle.version,
        notes: &notes,
        date: &options.date,
        archive: &asset,
        signature: &signature,
    });
    feed::write(&options.output, &entry).map_err(failed)
}
