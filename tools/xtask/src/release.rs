//! `cargo xtask release alpha` and `cargo xtask release promote`: a version
//! released as an alpha, and an alpha a person has tested promoted to the plain
//! version it was cut for.
//!
//! From 3.0.6 on every batch goes out as `X.Y.Z-alpha.N`, on `feed-alpha` only
//! (`release.yml` sends a version with a pre-release part to the alpha channel,
//! and an app on a pre-release reads `feed-alpha`). Nothing reaches
//! `feed-stable` until Gabriel has tested an alpha by hand: then the plain
//! `X.Y.Z` is released from the alpha's own commit with only the version
//! changed, and the publisher puts a plain version on both feeds.
//!
//! ```text
//! cargo xtask release alpha --notes FILE [--dry-run]
//! cargo xtask release promote X.Y.Z-alpha.N --notes FILE [--dry-run]
//! ```
//!
//! Both run in the main checkout, on `main`, with nothing uncommitted, and `main`
//! equal to upstream's. Each check that fails stops the run there, before
//! anything after it is done, and says why (`release: …`, status 1).
//!
//! `release alpha` releases the version the sources agree on, which must be
//! `X.Y.Z-alpha.N`:
//!
//! 1. the notes file says something, its first line being the release's title;
//! 2. the checkout is clean, on `main`, and `main` is upstream's;
//! 3. no tag `v<version>` is in the checkout, on origin or on upstream;
//! 4. `feed-alpha` serves a version before this one, which the publisher would
//!    otherwise leave where it is;
//! 5. CI on `main`'s commit ended green, with both platforms' gates;
//! 6. the Release workflow's dry run on `main` ended green;
//! 7. the annotated tag, its message the notes, pushed to origin and upstream;
//! 8. the Release run of the tag, watched to its end;
//! 9. `feed-alpha` read until it serves the version, and `feed-stable` checked
//!    unchanged, byte for byte.
//!
//! `--dry-run` does 1 to 6 and says what it would tag.
//!
//! `release promote` is [`promote`]'s. The programs are `git`, `gh` (logged in,
//! for the runs and the dispatch), `curl` (the feeds, read where installed apps
//! read them) and `cargo` (the lockfile's version), behind [`system::System`], so
//! that the tests give it a GitHub that is a script and never reach the real one.

mod alpha;
mod github;
mod promote;
mod system;
#[cfg(test)]
mod tests;

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::{self, Invocation};
use system::{Real, System};

pub const COMMANDS: &[Command] = &[
    Command {
        words: &["release", "alpha"],
        about: "Release the version the sources agree on, X.Y.Z-alpha.N, to feed-alpha: every check, the tag, the Release run and the feeds",
        usage: "--notes FILE [--dry-run]",
        run: Run::Native(alpha),
    },
    Command {
        words: &["release", "promote"],
        about: "Release the plain X.Y.Z of a tested alpha from the alpha's commit, the version alone changed, to both feeds",
        usage: "X.Y.Z-alpha.N --notes FILE [--dry-run]",
        run: Run::Native(promote),
    },
];

/// The repository the release runs on, for `gh`.
const REPOSITORY: &str = "ngvoicu/consensflow";

/// Where installed apps read the feeds: `<this>/<feed>/latest.json`.
const DOWNLOADS: &str = "https://github.com/ngvoicu/consensflow/releases/download";

/// The feeds the bridge and every release after it move (`app/feeds.json`).
const ALPHA_FEED: &str = "feed-alpha";
const STABLE_FEED: &str = "feed-stable";

/// Why a release did not go on.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A check that failed: what it found, and what to do.
    #[error("{0}")]
    Refused(String),
    #[error("`{command}` ended with status {code}: {said}")]
    Program {
        command: String,
        code: i32,
        said: String,
    },
    /// A program answered what cannot be read as what it was asked.
    #[error("{what} could not be read: {said}")]
    Answer { what: String, said: String },
    #[error("could not {action} {}: {source}", .path.display())]
    File {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    #[error(transparent)]
    Process(#[from] process::Failure),
}

fn refused(text: impl Into<String>) -> Error {
    Error::Refused(text.into())
}

/// How often what GitHub is doing is looked at.
#[derive(Debug, Clone, Copy)]
struct Pace {
    /// A run asked for, until it is listed.
    appear: Duration,
    appear_attempts: u32,
    /// A run that is listed, until it ends.
    poll: Duration,
    /// A feed, until it serves the version.
    feed: Duration,
    feed_attempts: u32,
}

impl Pace {
    /// As release-305.sh waited (20 s for a run to show, 30 s between looks),
    /// and the feeds ten minutes: a feed just swapped can answer 404 for a few
    /// minutes while GitHub's download links catch up.
    const REAL: Pace = Pace {
        appear: Duration::from_secs(20),
        appear_attempts: 60,
        poll: Duration::from_secs(30),
        feed: Duration::from_secs(10),
        feed_attempts: 60,
    };
}

/// A release run: the checkout, the machine, where it says how it goes, and how
/// often it looks.
struct Release<'a> {
    root: PathBuf,
    system: &'a mut dyn System,
    said: &'a mut dyn Write,
    pace: Pace,
}

impl Release<'_> {
    /// Says a line of how the run is going. A line that cannot be written is
    /// no reason to stop a release half done.
    fn say(&mut self, line: &str) {
        let _ = writeln!(self.said, "release: {line}");
    }

    /// What `git` says to `args` in the folder `dir`, as it wrote it; a status
    /// other than 0 is an error.
    fn git_in(&mut self, dir: &Path, args: &[&str]) -> Result<String, Error> {
        let invocation = Invocation::new("git", dir).args(args.iter().copied());
        let answered = self.system.capture(&invocation)?;
        if answered.code != 0 {
            return Err(Error::Program {
                command: invocation.display(),
                code: answered.code,
                said: answered.stderr.trim().to_owned(),
            });
        }
        Ok(answered.stdout)
    }

    /// What `git` says to `args` in the checkout, without the space around it.
    fn git(&mut self, args: &[&str]) -> Result<String, Error> {
        let root = self.root.clone();
        Ok(self.git_in(&root, args)?.trim().to_owned())
    }

    /// Whether `git` ends `args` with 0 in the checkout: for the questions it
    /// answers with its status.
    fn git_says_yes(&mut self, args: &[&str]) -> Result<bool, Error> {
        let invocation = Invocation::new("git", &self.root).args(args.iter().copied());
        Ok(self.system.capture(&invocation)?.code == 0)
    }

    /// The checks every release starts with: the checkout is clean, on `main`,
    /// and `main` is upstream's. Its commit.
    fn main_as_upstream_has_it(&mut self) -> Result<String, Error> {
        let changes = self.git(&["status", "--porcelain", "--untracked-files=no"])?;
        if !changes.is_empty() {
            return Err(refused(
                "the checkout has changes: a release is made from main as it was committed",
            ));
        }
        let branch = self.git(&["rev-parse", "--abbrev-ref", "HEAD"])?;
        if branch != "main" {
            return Err(refused(format!(
                "the checkout is on {branch}, not main: a release is made from main"
            )));
        }
        // Tags are asked of each remote by name, not followed in.
        self.git(&["fetch", "-q", "--no-tags", "upstream", "main"])?;
        let head = self.git(&["rev-parse", "HEAD"])?;
        let upstream = self.git(&["rev-parse", "upstream/main"])?;
        if head != upstream {
            return Err(refused(format!(
                "main is at {}, and upstream's main at {}: push or pull until they are one",
                short(&head),
                short(&upstream)
            )));
        }
        Ok(head)
    }

    /// Refuses `tag` where the checkout, origin or upstream already has it.
    fn tag_is_new(&mut self, tag: &str) -> Result<(), Error> {
        let reference = format!("refs/tags/{tag}");
        if self.git_says_yes(&["rev-parse", "-q", "--verify", &reference])? {
            return Err(refused(format!(
                "{tag} is already a tag in the checkout: that version was released, or begun"
            )));
        }
        for remote in ["origin", "upstream"] {
            if !self
                .git(&["ls-remote", "--tags", remote, &reference])?
                .is_empty()
            {
                return Err(refused(format!(
                    "{tag} is already a tag on {remote}: that version was released, or begun"
                )));
            }
        }
        Ok(())
    }

    /// Tags `commit` as `tag` with the notes as its message and pushes it, to
    /// origin first and then to upstream, whose push starts the Release run.
    fn tag_and_push(&mut self, tag: &str, notes: &Path, commit: &str) -> Result<(), Error> {
        let notes = notes.to_string_lossy().into_owned();
        self.git(&["tag", "-a", tag, "-F", &notes, commit])?;
        self.git(&["push", "-q", "origin", tag])?;
        self.git(&["push", "-q", "upstream", tag])?;
        self.say(&format!("tag {tag} pushed on {}", short(commit)));
        Ok(())
    }
}

/// The plain version an alpha was cut for: `3.0.6` of `3.0.6-alpha.1`. A version
/// that is not `X.Y.Z-alpha.N` is refused, though the workflow takes any
/// pre-release for an alpha.
fn plain_of(alpha: &str) -> Result<&str, Error> {
    cf_release::version::canonical("the version", alpha)
        .map_err(|cause| refused(cause.to_string()))?;
    let number = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    match alpha.split_once('-') {
        Some((plain, prerelease)) if prerelease.strip_prefix("alpha.").is_some_and(number) => {
            Ok(plain)
        }
        _ => Err(refused(format!(
            "{alpha} is not an alpha: an alpha is X.Y.Z-alpha.N"
        ))),
    }
}

/// The notes a release's tag carries, at a path git can be handed from the
/// checkout: a file that says something, its first line its title. The path is
/// made whole, not canonical, which on Windows is a `\\?\` path git does not read.
fn notes_at(path: &Path) -> Result<PathBuf, Error> {
    let found = std::path::absolute(path).map_err(|source| Error::File {
        action: "find the notes",
        path: path.to_path_buf(),
        source,
    })?;
    let text = fs::read_to_string(&found).map_err(|source| Error::File {
        action: "read the notes",
        path: found.clone(),
        source,
    })?;
    if text
        .lines()
        .next()
        .is_none_or(|title| title.trim().is_empty())
    {
        return Err(refused(format!(
            "the notes {} have no title: their first line is the release's",
            found.display()
        )));
    }
    Ok(found)
}

/// The first eight characters of a commit, as it is said.
fn short(commit: &str) -> &str {
    commit.get(..8).unwrap_or(commit)
}

/// What both commands are given after their words.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    /// `promote`'s alpha; none for `alpha`.
    version: Option<String>,
    notes: PathBuf,
    dry_run: bool,
}

/// Reads `args`: `--notes FILE`, `--dry-run`, and the alpha when `takes_version`.
fn options(args: &[OsString], takes_version: bool) -> Result<Options, Failure> {
    let (mut version, mut notes, mut dry_run) = (None, None, false);
    let mut words = args.iter();
    while let Some(word) = words.next() {
        match word.to_str() {
            Some("--dry-run") => dry_run = true,
            Some("--notes") => {
                let file = words
                    .next()
                    .ok_or_else(|| Failure::Usage("--notes takes a file".into()))?;
                notes = Some(PathBuf::from(file));
            }
            Some(text) if takes_version && version.is_none() && !text.starts_with('-') => {
                version = Some(text.to_owned());
            }
            _ => {
                return Err(Failure::Usage(format!(
                    "release does not take {}",
                    word.to_string_lossy()
                )))
            }
        }
    }
    if takes_version && version.is_none() {
        return Err(Failure::Usage(
            "release promote takes the alpha to promote, X.Y.Z-alpha.N".into(),
        ));
    }
    let notes = notes.ok_or_else(|| Failure::Usage("release takes --notes FILE".into()))?;
    Ok(Options {
        version,
        notes,
        dry_run,
    })
}

fn alpha(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    let options = options(args, false)?;
    let mut system = Real::new(&context.env);
    let mut release = Release {
        root: context.root.clone(),
        system: &mut system,
        said: console.err,
        pace: Pace::REAL,
    };
    let outcome = alpha::release(&mut release, &options);
    finish(outcome, console.out, release.said)
}

fn promote(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    let options = options(args, true)?;
    let mut system = Real::new(&context.env);
    let mut release = Release {
        root: context.root.clone(),
        system: &mut system,
        said: console.err,
        pace: Pace::REAL,
    };
    let outcome = promote::release(&mut release, &options);
    finish(outcome, console.out, release.said)
}

/// The run's answer: what it did on `out` and 0, or why it stopped on `err` and 1.
fn finish(
    outcome: Result<String, Error>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32, Failure> {
    match outcome {
        Ok(done) => {
            write!(out, "{done}")?;
            Ok(0)
        }
        Err(error) => {
            writeln!(err, "release: {error}")?;
            Ok(1)
        }
    }
}
