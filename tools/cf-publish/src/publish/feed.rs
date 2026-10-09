//! Moving one feed: its release made if there is none, what a swap that was cut
//! short left beside latest.json settled, and latest.json uploaded, left, or
//! swapped for this release's.

use std::fs;

use crate::failure::Failure;
use crate::gh::{Asset, ReleaseState};
use crate::publish::{FeedDone, Run, LATEST};
use crate::read::Reader;
use crate::version::{later_release, named_release, Version};

/// Beside latest.json while one is being swapped for another.
const NEXT: &str = "latest.next.json";
const PREVIOUS: &str = "latest.previous.json";

/// What moving a feed did, and what it named before it was changed.
pub struct Moved {
    pub did: FeedDone,
    /// The release the feed named before this run changed it, or none.
    pub named: Option<Version>,
}

/// Puts what a swap that was cut short left beside latest.json right: the
/// previous latest.json back if latest.json is gone, then what is left of a swap
/// gone. Resolves to what the feed holds now.
fn settle(run: &Run, feed: &str, state: ReleaseState) -> Result<ReleaseState, Failure> {
    let runner = &run.runner;
    let mut moved = false;
    if !state.has(LATEST) && state.has(PREVIOUS) {
        runner.rename(run.repo, state.asset(PREVIOUS), LATEST)?;
        moved = true;
    } else if state.has(PREVIOUS) {
        let args = ["release", "delete-asset", feed, PREVIOUS, "--yes"];
        runner.must(&args, &format!("clearing {PREVIOUS}"))?;
        moved = true;
    }
    if state.has(NEXT) {
        let args = ["release", "delete-asset", feed, NEXT, "--yes"];
        runner.must(&args, &format!("clearing {NEXT}"))?;
        moved = true;
    }
    if !moved {
        return Ok(state);
    }
    runner
        .state(feed)?
        .ok_or_else(|| Failure::new(format!("the feed {feed} is gone")))
}

/// Replaces a feed's latest.json by `dir/latest.json` without a moment in which
/// there is none that an upload could stretch: the new one is uploaded beside
/// it, and the names swapped by two renames. A failure puts back what was.
fn swap(run: &Run, feed: &str) -> Result<(), Failure> {
    let runner = &run.runner;
    let copied = fs::copy(run.dir.join(LATEST), run.dir.join(NEXT));
    copied.map_err(|cause| Failure::new(format!("could not copy {LATEST} to {NEXT}: {cause}")))?;
    let clear_next = ["release", "delete-asset", feed, NEXT, "--yes"];
    if let Err(cause) = runner.must(
        &["release", "upload", feed, NEXT],
        &format!("uploading the new {LATEST} to {feed}"),
    ) {
        runner.ask(&clear_next);
        return Err(cause);
    }
    let held = runner
        .state(feed)?
        .ok_or_else(|| Failure::new(format!("the feed {feed} is gone")))?;
    if let Err(cause) = runner.rename(run.repo, held.asset(LATEST), PREVIOUS) {
        runner.ask(&clear_next);
        return Err(cause);
    }
    if let Err(cause) = runner.rename(run.repo, held.asset(NEXT), LATEST) {
        let aside = held.asset(LATEST).map(|asset| Asset {
            name: PREVIOUS.to_string(),
            ..asset.clone()
        });
        if let Err(again) = runner.rename(run.repo, aside.as_ref(), LATEST) {
            return Err(Failure::new(format!(
                "{cause}; and {feed} lacks its {LATEST} until this is run again, for putting {PREVIOUS} back failed: {again}"
            )));
        }
        return Err(cause);
    }
    let cleared = runner.ask(&["release", "delete-asset", feed, PREVIOUS, "--yes"]);
    if cleared.status != 0 {
        runner.log(&format!(
            "{PREVIOUS} of {feed} was not cleared; the next run settles it"
        ));
    }
    Ok(())
}

/// The word with its first letter capitalized.
fn capitalized(word: &str) -> String {
    let mut letters = word.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

/// One feed made to serve `dir/latest.json`: the release made if there is none,
/// what a cut-short swap left settled, and latest.json uploaded, left, or
/// swapped. Resolves to what was done, which is `Superseded` where the feed
/// already names a later release, which it is left at (no feed is moved
/// backward); and to the release the feed named before it was changed, or none.
/// What a feed names is read before it is changed: one that cannot be read
/// fails the run with the feed as it was, but a latest.json that is listed and
/// not served at all is a broken one, which is swapped. A feed of the old
/// generation is marked as pinned.
pub fn move_feed(
    run: &Run,
    reads: &mut Reader,
    feed: &str,
    pinned: bool,
) -> Result<Moved, Failure> {
    let runner = &run.runner;
    let name = feed.split_once('-').map_or(feed, |(_, name)| name);
    let label = capitalized(name);
    let existing = runner.state(feed)?;
    let made = existing.is_none();
    let state = match existing {
        Some(state) => state,
        None => {
            let title = format!("{label} update feed");
            runner.must(
                &[
                    "release",
                    "create",
                    feed,
                    "--prerelease",
                    "--title",
                    &title,
                    "--notes",
                    "The latest.json installed apps read on this channel. Do not delete.",
                ],
                &format!("making the feed {feed}"),
            )?;
            ReleaseState {
                draft: false,
                assets: Vec::new(),
            }
        }
    };
    let state = settle(run, feed, state)?;
    let mut did = FeedDone::Kept;
    let mut named = None;
    if !state.has(LATEST) {
        runner.must(
            &["release", "upload", feed, LATEST],
            &format!("uploading {LATEST} to {feed}"),
        )?;
        did = if made {
            FeedDone::Created
        } else {
            FeedDone::Uploaded
        };
    } else {
        // A listed latest.json that is not served is a broken one, which a swap mends;
        // any other failure to read it leaves it unknown whether it names a later release.
        let served = reads.file(&format!("{}/{feed}/{LATEST}", run.base));
        if let Err(refusal) = &served {
            if refusal.status != Some(404) {
                return Err(Failure::new(format!(
                    "{feed} cannot be read ({}): it is not known whether it names a release after {}, and no feed is moved backward",
                    refusal.why, run.version
                )));
            }
        }
        named = served.as_ref().ok().and_then(|body| named_release(body));
        let later = served
            .as_ref()
            .ok()
            .and_then(|body| later_release(body, run.version));
        if let Some(later) = later {
            runner.log(&format!(
                "{feed} names {later}, which comes after {}: it is left alone, and nothing is moved backward",
                run.version
            ));
            did = FeedDone::Superseded(later);
        } else {
            let differs = match &served {
                Err(_) => true,
                Ok(body) => {
                    let built = fs::read(run.dir.join(LATEST)).map_err(|cause| {
                        Failure::new(format!("could not read {LATEST}: {cause}"))
                    })?;
                    *body != built
                }
            };
            if differs {
                swap(run, feed)?;
                did = FeedDone::Replaced;
            }
        }
    }
    if pinned {
        let title = format!("{label} update feed, pinned");
        let notes = format!(
            "Pinned to ConsensFlow {}, the first release to read feed-{name}: the apps before it read this feed and install it, and from it on read feed-{name}. Never move this feed, and never delete this release or the one it names.",
            run.version
        );
        runner.must(
            &[
                "release", "edit", feed, "--title", &title, "--notes", &notes,
            ],
            &format!("marking {feed} as pinned"),
        )?;
    }
    Ok(Moved { did, named })
}
