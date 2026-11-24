//! The look at one harness's CLI: its version, how it was installed, and the
//! latest release, in that order, and the extension made for the two harnesses
//! that load one.

use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_base::env::Env;
use cf_proto::agents::Harness;
use futures_util::future::{join3, Either, FutureExt};

use super::row::{Distribution, Release, Row, Setup, Tested, Update, Version};
use super::source::{release_source, Source};
use super::version::version_of;
use super::{program, Inner, PROBE};
use crate::launch::{self, Check, Said};
use crate::seams::processes::{probe, Asking, Failed, Unanswered};

/// How long Codex's version, which only names a refused Codex, may take: it ends before
/// the row's own limit, so a refusal already known is not lost to it.
const VERSION_LIMIT: Duration = Duration::from_secs(35);

/// Whether `harness`'s launch checks its CLI, so that the page's verdict is that check.
pub(super) fn checks(harness: Harness) -> bool {
    launch::checks(harness)
}

/// The row of a CLI whose look did not end within the row's limit.
pub(super) fn unfinished(harness: Harness, path: Option<String>, checked_at: i64) -> Row {
    Row {
        id: harness,
        installed: path.is_some(),
        path,
        checked_at,
        version: Version::Error {
            reason: "Version check timed out".to_owned(),
        },
        update: Update::NotChecked,
        tested: Tested::of(harness, None),
        // Said after the page's "ConsensFlow could not check it: ", so it names no subject of its own.
        setup: Some(Setup::Unchecked {
            reason: "the check didn't finish in 40 seconds.".to_owned(),
        }),
        distribution: Distribution::Unlooked,
        extension: None,
    }
}

/// The row of the harness `harness`, whose CLI is at `path` (none when it is
/// installed nowhere), the look begun at `checked_at`. Where `kept` is a row of
/// this CLI the feed was asked for lately, its release, install and extension are
/// that row's, and nothing of them is asked again.
pub(super) async fn inspect(
    inner: &Inner,
    harness: Harness,
    path: Option<String>,
    checked_at: i64,
    kept: Option<&Row>,
) -> Row {
    let mut row = Row {
        id: harness,
        installed: path.is_some(),
        path: path.clone(),
        checked_at,
        version: Version::NotInstalled,
        update: Update::NotChecked,
        tested: Tested::of(harness, None),
        setup: None,
        distribution: Distribution::Unlooked,
        extension: None,
    };
    let Some(path) = path else {
        return row;
    };
    if checks(harness) {
        return inspect_checked(inner, harness, path, row, kept).await;
    }
    let (version, release) = ask_version(inner, &path, &inner.env, harness).await;
    row.version = version;
    row.tested = Tested::of(harness, release.as_deref());
    let source = release_source(harness, &path, &inner.env);
    let latest = inner.latest.latest(harness, &source).await;
    finish(inner, row, source, latest)
}

/// The rest of a row once its version is known: the install, the latest release and what
/// it says of the version, and the extension.
fn finish(inner: &Inner, mut row: Row, source: Source, latest: Result<String, String>) -> Row {
    row.distribution = source
        .distribution
        .clone()
        .map_or(Distribution::Unrecognized, Distribution::Named);
    let command = source.update.as_deref().map(super::source::command_line);
    row.update = match latest {
        Ok(value) => {
            let release = Release {
                value,
                source: source.url,
                command,
            };
            compared(&row, release)
        }
        Err(reason) => Update::Error { reason, command },
    };
    row.extension = launch::extension(row.id, &inner.env);
    row
}

/// What `release` says of the version `row` holds.
fn compared(row: &Row, release: Release) -> Update {
    match launch::newer(row.id, row.version.value(), &release.value) {
        Some(true) => Update::Available(release),
        Some(false) => Update::Current(release),
        _ => Update::Unknown(release),
    }
}

/// The row of a harness whose launch checks its CLI: the launch's own check, asked now,
/// and beside it the version the row shows (Codex's own question, which a refusal shares)
/// and, where the feed is not kept, the release feed.
async fn inspect_checked(
    inner: &Inner,
    harness: Harness,
    path: String,
    mut row: Row,
    kept: Option<&Row>,
) -> Row {
    let asking = Asking {
        probes: &inner.probes,
        processes: &inner.processes,
        env: &inner.env,
    };
    // Codex's version: one question for the look, asked once for the row and for the
    // sentence a refusal names it in, and its failure is the look's one answer.
    let version = async {
        let asked = Box::pin(probe(
            &inner.probes,
            &inner.processes,
            Path::new(&path),
            &["--version"],
            &inner.env,
        ));
        let mut timer = inner.time.sleep(VERSION_LIMIT);
        match futures_util::future::select(asked, timer.as_mut()).await {
            Either::Left((answer, _)) => answer,
            Either::Right(_) => Err(Unanswered::Failed(Failed {
                message: String::new(),
                code: None,
                killed: true,
                stdout: String::new(),
            })),
        }
    }
    .boxed_local()
    .shared();
    let own_question = launch::page_asks_version(harness);
    let checking = async {
        let shared = own_question.then(|| version.clone().boxed_local());
        // Only a harness that checks is looked at here: its check is there.
        launch::check_with(harness, &asking, &path, shared)
            .await
            .unwrap_or(Check {
                said: Said::NotAsked,
                verdict: Ok(()),
            })
    };
    let own_version = async {
        if own_question {
            Some(version.clone().await)
        } else {
            None
        }
    };
    let feed = async {
        if kept.is_some() {
            return None;
        }
        let source = release_source(harness, &path, &inner.env);
        let latest = inner.latest.latest(harness, &source).await;
        Some((source, latest))
    };
    let (check, asked, feed) = join3(checking, own_version, feed).await;
    // What the page shows of the version is what the look's one question said, where the page
    // asks its own, else what the check read.
    let said = match asked {
        Some(Ok(answered)) => Said::Read(answered.stdout),
        Some(Err(Unanswered::Unread(sentence))) => Said::Unanswered(sentence),
        Some(Err(Unanswered::Failed(failed))) => Said::Unanswered(if failed.killed {
            launch::TIMED_OUT.to_owned()
        } else {
            failed.message
        }),
        None => check.said.clone(),
    };
    let (version, release) = shown(harness, &said);
    row.version = version;
    row.tested = Tested::of(harness, release.as_deref());
    row.setup = Some(setup_of(&check));
    match (feed, kept) {
        (Some((source, latest)), _) => finish(inner, row, source, latest),
        (None, Some(kept)) => {
            row.distribution = kept.distribution.clone();
            row.extension = None;
            row.update = match &kept.update {
                Update::Available(release)
                | Update::Current(release)
                | Update::Unknown(release) => compared(&row, release.clone()),
                other => other.clone(),
            };
            row
        }
        (None, None) => row,
    }
}

/// What the launch's check came to, as the row says it.
fn setup_of(check: &Check) -> Setup {
    match &check.verdict {
        Ok(()) => Setup::Ready,
        Err(refused) if refused.read => Setup::UpdateRequired {
            reason: refused.sentence.clone(),
        },
        Err(refused) => Setup::Unchecked {
            reason: refused.sentence.clone(),
        },
    }
}

fn failure(killed: bool) -> String {
    if killed {
        "Version check timed out"
    } else {
        "Version command failed"
    }
    .to_owned()
}

/// The version the look read of a CLI that a launch checks, and the release its own line proves.
fn shown(harness: Harness, said: &Said) -> (Version, Option<String>) {
    let unrecognized = || Version::Unknown {
        reason: "Version output was not recognized".to_owned(),
    };
    match said {
        Said::Read(text) => {
            let release = launch::release_of(harness, text).map(str::to_owned);
            let version = launch::shown_in(harness, text).map_or_else(unrecognized, |value| {
                Version::Checked {
                    value: value.to_owned(),
                }
            });
            (version, release)
        }
        Said::Unanswered(why) if why == launch::NO_VERSION_NAMED => (unrecognized(), None),
        Said::Unanswered(why) => (
            Version::Error {
                reason: failure(why == launch::TIMED_OUT),
            },
            None,
        ),
        Said::NotAsked => (
            Version::Unknown {
                reason: "Version was not asked".to_owned(),
            },
            None,
        ),
    }
}

/// What the CLI at `path` says to `--version`, run in `env`, through the admin's
/// own capture: Claude Code's and Pi's, which no launch checks.
async fn ask_version(
    inner: &Inner,
    path: &str,
    env: &Env,
    harness: Harness,
) -> (Version, Option<String>) {
    let asked = program(env, PathBuf::from(path), vec!["--version".to_owned()]);
    match inner.capture.capture(asked, PROBE).await {
        Ok(said) => {
            let release = launch::release_of(harness, &said.stdout);
            let version = match release.or(version_of(&said.stdout)) {
                Some(value) => Version::Checked {
                    value: value.to_owned(),
                },
                None => Version::Unknown {
                    reason: "Version output was not recognized".to_owned(),
                },
            };
            (version, release.map(str::to_owned))
        }
        Err(failed) => (
            Version::Error {
                reason: failure(failed.killed),
            },
            None,
        ),
    }
}
