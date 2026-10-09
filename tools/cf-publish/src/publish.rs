//! Publishes a release and moves its feeds, so that a run that died or was cut
//! short is finished by running it again, and no run leaves an installed app
//! without the feed it reads.
//!
//! `dir` holds what the workflow's jobs built (the files of
//! [`ReleaseAssets`](crate::assets::ReleaseAssets)) and notes.txt; `base` is
//! where GitHub serves a release's files
//! (`https://github.com/<owner>/<repo>/releases/download`). It runs only for the
//! push of a version tag (`cli` holds that guard): a hand run publishes
//! nothing. In order:
//!
//! 1. the rule ([`rule`](crate::rule)): a release before the bridge is refused,
//!    a bridge whose archive lacks what the old apps check for is refused, and a
//!    later release is refused until the old feeds serve the bridge with its
//!    files. Nothing has been moved when it refuses;
//! 2. the versioned release, whose files are never replaced. None yet: a draft
//!    is made, filled, and published only when every file is there at its size.
//!    A draft left by a run that died is emptied and filled again, being
//!    nobody's. A published one is kept: what it lacks is added, and then every
//!    file is downloaded and held to the file built here, a mismatch refusing
//!    the run before a feed moves;
//! 3. each feed the rule names, the new ones first ([`feed`]). A feed's
//!    latest.json is never deleted for its replacement: the replacement is
//!    uploaded beside it, and the two names swapped by renames (so the feed
//!    lacks the file for one API call, not for an upload), the previous one kept
//!    until the swap is done and put back if it fails. A feed that already
//!    serves this latest.json is left alone. So is one that already names a
//!    later release, for no feed is moved backward: a job run again after a
//!    later release went out does what is left of it (the release, the feeds
//!    that are not past it, the pin) and leaves that feed where the later
//!    release put it. What a run that died left beside latest.json
//!    (latest.next.json, latest.previous.json) is settled first.
//!
//! What the feeds serve afterwards is [`checks`](crate::checks)' to say. A run
//! that finished leaves `dir` a record ([`FEEDS_BEFORE`]) of what each new feed
//! named when it read it, before it changed any, so that the check can tell a
//! feed that went back to this release from one that never left it.

mod feed;

use std::fmt;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::assets::{missing_for_old_apps, name_of, on_disk, ReleaseAssets};
use crate::checks::{asset_problems, check_prerequisites, FEEDS_BEFORE};
use crate::digest::sha256_hex;
use crate::failure::Failure;
use crate::gh::{Gh, ReleaseState, Runner};
use crate::manifest::Manifest;
use crate::read::{Patience, Reader};
use crate::rule::plan_feeds;
use crate::version::Version;

/// What a feed serves, and what a release is replaced by.
pub const LATEST: &str = "latest.json";

/// What was done to the versioned release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseDone {
    /// None was there: a draft was made, filled, and published.
    Created,
    /// A draft a run that died left was emptied, filled again and published.
    Remade,
    /// A published release that lacked files was given them.
    Completed,
    /// A published release that lacked nothing.
    Kept,
}

impl fmt::Display for ReleaseDone {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            ReleaseDone::Created => "created",
            ReleaseDone::Remade => "remade",
            ReleaseDone::Completed => "completed",
            ReleaseDone::Kept => "kept",
        })
    }
}

/// What was done to a feed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeedDone {
    /// The feed was made, with its latest.json.
    Created,
    /// A feed with no latest.json was given one.
    Uploaded,
    /// Its latest.json was swapped for this release's.
    Replaced,
    /// It already served this release's latest.json.
    Kept,
    /// It already names a later release, which it is left at: no feed is moved
    /// backward.
    Superseded(Version),
}

impl fmt::Display for FeedDone {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeedDone::Created => formatter.write_str("created"),
            FeedDone::Uploaded => formatter.write_str("uploaded"),
            FeedDone::Replaced => formatter.write_str("replaced"),
            FeedDone::Kept => formatter.write_str("kept"),
            FeedDone::Superseded(later) => write!(formatter, "superseded by {later}"),
        }
    }
}

/// What a run did: the release, and each feed in the order it was moved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Published {
    pub version: Version,
    pub release: ReleaseDone,
    pub feeds: Vec<(String, FeedDone)>,
}

/// What is published, from where, and by what rule.
pub struct Publication<'a> {
    /// The folder the jobs built the release into.
    pub dir: &'a Path,
    /// `v<version>`.
    pub tag: &'a str,
    /// Where GitHub serves a release's files.
    pub base: &'a str,
    /// `<owner>/<repo>`.
    pub repo: &'a str,
    pub manifest: &'a Manifest,
    /// How long the reads of the files and feeds wait for GitHub to catch up.
    pub patience: Patience,
}

/// What a publication is done through, so that the tests stand in for them.
pub struct Means<'a> {
    /// GitHub.
    pub gh: &'a dyn Gh,
    /// The members of an archive, as `tar -t` lists them.
    pub members: &'a dyn Fn(&Path) -> Result<Vec<String>, Failure>,
    /// Where each call, and what a run decides, is said.
    pub log: &'a dyn Fn(&str),
}

/// What a feed's run needs, which does not change from one feed to the next.
struct Run<'a> {
    runner: Runner<'a>,
    dir: &'a Path,
    repo: &'a str,
    base: &'a str,
    version: &'a Version,
}

/// The text of the release's page: its notes, and where each file is for whom.
fn page_of(notes: &str, version: &Version) -> String {
    [
        notes.trim_end_matches('\n'),
        "",
        "**Downloads.** macOS on Apple silicon: the DMG, signed with a Developer ID",
        "and notarized by Apple. Windows x64: the installer (`-setup.exe`), or the",
        "portable exe (`-portable.exe`), one file you run from anywhere: its first",
        "start unpacks its `cf` into",
        "`%LOCALAPPDATA%\\dev.ngvoicu.consensflow\\portable-runtime`. Its data stays in your user",
        "folder either way. Neither Windows file is code-signed, so Windows shows its",
        "unknown-publisher warning. The Mac app updates itself through",
        &format!(
            "ConsensFlow → Check for Updates on the {} channel.",
            if version.is_prerelease() {
                "Alpha"
            } else {
                "Stable"
            }
        ),
        "",
    ]
    .join("\n")
}

/// The versioned release, made as it should be or finished from what a run that
/// died left. Its files are checked
/// ([`asset_problems`]) by the caller, which is where a published release that
/// differs is found.
fn release_versioned(
    run: &Run,
    tag: &str,
    files: &[&str],
    notes_file: &str,
) -> Result<ReleaseDone, Failure> {
    let runner = &run.runner;
    let existing = runner.state(tag)?;
    let made = existing.is_none();
    let state = match existing {
        Some(state) => state,
        None => {
            let title = format!("ConsensFlow {}", run.version);
            let mut args = vec!["release", "create", tag, "--draft", "--verify-tag"];
            args.extend(["--title", &title, "--notes-file", notes_file]);
            if run.version.is_prerelease() {
                args.push("--prerelease");
            }
            runner.must(&args, &format!("making the release {tag}"))?;
            ReleaseState {
                draft: true,
                assets: Vec::new(),
            }
        }
    };
    if !state.draft {
        let lacking: Vec<&str> = files
            .iter()
            .copied()
            .filter(|file| !state.has(name_of(file)))
            .collect();
        if lacking.is_empty() {
            return Ok(ReleaseDone::Kept);
        }
        let mut args = vec!["release", "upload", tag];
        args.extend(&lacking);
        runner.must(&args, &format!("adding to the release {tag}"))?;
        return Ok(ReleaseDone::Completed);
    }
    for asset in &state.assets {
        let args = ["release", "delete-asset", tag, asset.name.as_str(), "--yes"];
        runner.must(&args, &format!("emptying the draft {tag}"))?;
    }
    let mut args = vec!["release", "upload", tag];
    args.extend(files);
    runner.must(&args, &format!("filling the release {tag}"))?;
    let filled = runner
        .state(tag)?
        .ok_or_else(|| Failure::new(format!("the release {tag} is gone after it was filled")))?;
    for file in files {
        let asset = filled.asset(name_of(file));
        let size = fs::metadata(on_disk(run.dir, file))
            .map_err(|cause| Failure::new(format!("could not read {file}: {cause}")))?
            .len();
        let whole = asset.is_some_and(|asset| {
            asset.size == Some(size) && asset.state.as_deref().unwrap_or("uploaded") == "uploaded"
        });
        if !whole {
            let held = match asset {
                None => "not at all".to_string(),
                Some(asset) => format!(
                    "as {} bytes, {}",
                    asset
                        .size
                        .map_or_else(|| "undefined".to_string(), |size| size.to_string()),
                    asset.state.as_deref().unwrap_or("undefined")
                ),
            };
            return Err(Failure::new(format!(
                "the draft {tag} holds {} {held}, not as the {size} built: nothing is public, run it again",
                name_of(file)
            )));
        }
    }
    runner.must(
        &["release", "edit", tag, "--draft=false"],
        &format!("publishing the release {tag}"),
    )?;
    Ok(if made {
        ReleaseDone::Created
    } else {
        ReleaseDone::Remade
    })
}

/// Publishes the release `publication.tag` built in `publication.dir`, and
/// moves its feeds. Resolves to what was done; fails, with nothing moved, when
/// the rule refuses it.
pub fn publish_release(publication: &Publication, means: &Means) -> Result<Published, Failure> {
    let Publication {
        dir,
        tag,
        base,
        repo,
        manifest,
        patience,
    } = *publication;
    if !tag
        .strip_prefix('v')
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
    {
        return Err(Failure::new(format!("{tag} is not a version tag")));
    }
    let version = Version::parse(&tag[1..])?;
    let assets = ReleaseAssets::of(&version);
    let paths = assets.all();
    for path in paths.into_iter().chain(["notes.txt"]) {
        let size = fs::metadata(on_disk(dir, path)).map_or(0, |found| found.len());
        if size == 0 {
            return Err(Failure::new(format!("missing {path}")));
        }
    }
    let missing = missing_for_old_apps(&(means.members)(&on_disk(dir, &assets.archive))?);
    let feeds = plan_feeds(&version, &missing, manifest)?;
    let problems = check_prerequisites(&version, base, manifest, patience);
    if !problems.is_empty() {
        return Err(Failure::new(format!(
            "{version} may not move a feed, and nothing was moved:\n- {}",
            problems.join("\n- ")
        )));
    }

    let read = |path: &str| {
        fs::read(on_disk(dir, path))
            .map_err(|cause| Failure::new(format!("could not read {path}: {cause}")))
    };
    let mut sums = String::new();
    for path in paths {
        sums.push_str(&format!(
            "{}  {}\n",
            sha256_hex(&read(path)?),
            name_of(path)
        ));
    }
    write(dir, "SHA256SUMS", sums.as_bytes())?;
    let notes = read("notes.txt")?;
    write(
        dir,
        "body.md",
        page_of(&String::from_utf8_lossy(&notes), &version).as_bytes(),
    )?;

    let run = Run {
        runner: Runner::new(means.gh, dir, means.log),
        dir,
        repo,
        base,
        version: &version,
    };
    let mut files = paths.to_vec();
    files.push("SHA256SUMS");
    let mut reads = Reader::new(patience);
    let versioned = release_versioned(&run, tag, &files, "body.md")?;
    let wrong = asset_problems(dir, &version, base, &mut reads)?;
    if !wrong.is_empty() {
        return Err(Failure::new(format!(
            "the release {tag} ({versioned}) does not hold the files built here, and no feed was moved: a published release's files are not replaced, so delete the release if it was a mistake, and run again:\n- {}",
            wrong.join("\n- ")
        )));
    }
    let mut moved = Vec::new();
    let mut before = Map::new();
    for feed in &feeds {
        let pinned = manifest.legacy.contains(feed);
        let done = feed::move_feed(&run, &mut reads, feed, pinned)?;
        (means.log)(&format!("{feed}: {}", done.did));
        if !pinned {
            before.insert(
                feed.clone(),
                done.named
                    .map_or(Value::Null, |named| Value::String(named.to_string())),
            );
        }
        moved.push((feed.clone(), done.did));
    }
    // What the check that follows holds the new feeds to: none went back to this release.
    write(
        dir,
        FEEDS_BEFORE,
        format!("{}\n", Value::Object(before)).as_bytes(),
    )?;
    Ok(Published {
        version,
        release: versioned,
        feeds: moved,
    })
}

/// Writes `bytes` to `name` in `dir`.
fn write(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), Failure> {
    fs::write(dir.join(name), bytes)
        .map_err(|cause| Failure::new(format!("could not write {name}: {cause}")))
}
