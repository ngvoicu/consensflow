//! The switch: the adapter that launches a harness's windows, built with
//! the engine's services.

use std::path::PathBuf;
use std::rc::Rc;

use cf_base::env::Env;
use cf_proto::agents::Harness;
use futures_util::future::LocalBoxFuture;

use crate::claude::ClaudeAdapter;
use crate::codex::CodexAdapter;
use crate::contract::Adapter;
use crate::devin::DevinAdapter;
use crate::muse::MuseAdapter;
use crate::pi::PiAdapter;
use crate::seams::processes::{Asking, Probed, Unanswered};
use crate::seams::Services;

/// The adapter of `harness`'s windows.
pub fn adapter(harness: Harness, services: &Services) -> Rc<dyn Adapter> {
    match harness {
        Harness::Claude => Rc::new(ClaudeAdapter::new(services)),
        Harness::Codex => Rc::new(CodexAdapter::new(services)),
        Harness::Devin => Rc::new(DevinAdapter::new(services)),
        Harness::Pi => Rc::new(PiAdapter::new(services)),
        Harness::Muse => Rc::new(MuseAdapter::new(services)),
    }
}

/// The background work `harness` runs beside its windows, with the config of one of them: the
/// folders it works in, and the words that tell a `cf` run there why it changes nothing on the
/// board. None for a harness that runs none (Codex's memory job is the one there is:
/// `codex::memories`; the others' background forks run no board command, read from their
/// binaries, 2026-11-08).
pub(crate) fn background_work(harness: Harness, env: &Env) -> Option<(Vec<PathBuf>, &'static str)> {
    match harness {
        Harness::Codex => Some((
            crate::codex::memories::memories_folders(env),
            crate::codex::memories::WORDS,
        )),
        Harness::Claude | Harness::Devin | Harness::Pi | Harness::Muse => None,
    }
}

/// Why a check got no answer from a CLI that ran out of its time, in the Admin's words.
pub(crate) const TIMED_OUT: &str = "it did not answer in time";

/// Why a check got no version from a CLI that answered, in the Admin's words.
pub(crate) const NO_VERSION_NAMED: &str = "it named no version of Muse Code's own";

/// What a launch's check of its CLI came to. The launch opens on `Ok` and fails with
/// the sentence on `Err`. The Admin page shows the same and judges nothing itself.
pub struct Check {
    pub said: Said,
    pub verdict: Result<(), Refused>,
}

/// What the check learned of the CLI's version while it checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// It asked no version: a pinned Muse binary's name aside.
    NotAsked,
    /// What the CLI or its install said: Muse's answer to `--version`, Muse's pin.
    Read(String),
    /// It asked and got no answer: why, in the Admin's words.
    Unanswered(String),
}

/// A launch's refusal.
pub struct Refused {
    /// The sentence the launch fails with, word for word.
    pub sentence: String,
    /// True where the CLI was read and an update mends it; false where it could not
    /// be read.
    pub read: bool,
}

/// Whether `harness`'s launch checks its CLI, so that the Admin page's verdict is that check.
pub(crate) fn checks(harness: Harness) -> bool {
    matches!(harness, Harness::Codex | Harness::Devin | Harness::Muse)
}

/// Whether the Admin page asks `harness`'s CLI its version itself, beside the check, because
/// the check asks it only where it refuses (Codex): the page's question and the check's are
/// one question of a look, shared.
pub(crate) fn page_asks_version(harness: Harness) -> bool {
    harness == Harness::Codex
}

/// The release `harness`'s own `--version` line proves in `said`: exactly one line wholly in
/// the harness's own form (Muse's, or a pin's line alone), none where there is not.
pub(crate) fn release_of(harness: Harness, said: &str) -> Option<&str> {
    match harness {
        Harness::Claude => crate::claude::declared::release(said),
        Harness::Codex => crate::codex::declared::release(said),
        Harness::Pi => crate::pi::declared::release(said),
        Harness::Devin => crate::devin::declared::release(said),
        Harness::Muse => crate::muse::declared::release(said)
            .or_else(|| crate::muse::declared::pin_release(said)),
    }
}

/// The version the Admin page shows of what `harness`'s CLI said: the release it proves, else
/// the first version number in it (a Codex's, as its refusal names it).
pub(crate) fn shown_in(harness: Harness, said: &str) -> Option<&str> {
    match harness {
        Harness::Codex => crate::codex::launch::named_version(said),
        Harness::Muse => release_of(harness, said)
            .or_else(|| crate::admin::version::version_of(said))
            .or_else(|| (!said.contains(char::is_whitespace)).then_some(said)),
        _ => release_of(harness, said).or_else(|| crate::admin::version::version_of(said)),
    }
}

/// Whether `remote`, a release a feed names, is later than `local`, the version the CLI
/// says, as `harness` writes versions.
pub(crate) fn newer(harness: Harness, local: Option<&str>, remote: &str) -> Option<bool> {
    match harness {
        Harness::Muse => crate::muse::version::newer(local, remote),
        _ => crate::admin::version::newer(local, remote),
    }
}

/// The extension made for `harness`'s CLI, where it loads one (Pi's).
pub(crate) fn extension(harness: Harness, env: &Env) -> Option<crate::admin::Extension> {
    match harness {
        Harness::Pi => Some(crate::admin::Extension::of_pi(
            crate::pi::prepare_extension(env),
        )),
        _ => None,
    }
}

/// The newest release `harness` was tested on and the oldest tested or enforced, where it
/// has one.
pub(crate) fn declared(harness: Harness) -> (&'static str, Option<String>) {
    match harness {
        Harness::Claude => (
            crate::claude::declared::NEWEST_TESTED,
            Some(crate::claude::declared::OLDEST_TESTED.to_owned()),
        ),
        Harness::Codex => (crate::codex::declared::NEWEST_TESTED, None),
        Harness::Pi => (
            crate::pi::declared::NEWEST_TESTED,
            Some(crate::pi::declared::OLDEST_TESTED.to_owned()),
        ),
        Harness::Devin => (
            crate::devin::declared::NEWEST_TESTED,
            Some(crate::devin::minimum_version()),
        ),
        Harness::Muse => (
            crate::muse::declared::NEWEST_TESTED,
            Some(crate::muse::version::minimum()),
        ),
    }
}

/// The check `harness`'s launch makes of the CLI at `executable`, through the questions
/// `asking` carries: none for a harness whose launch makes none (Claude Code, Pi). The
/// Admin page calls this, so that its verdict is the launch's own.
pub async fn check(harness: Harness, asking: &Asking<'_>, executable: &str) -> Option<Check> {
    check_with(harness, asking, executable, None).await
}

/// [`check`], where a Codex that is refused names its version by `version`, a question the
/// caller shares with others (the Admin page asks it beside the check): where none is given,
/// the check asks its own.
pub(crate) async fn check_with(
    harness: Harness,
    asking: &Asking<'_>,
    executable: &str,
    version: Option<LocalBoxFuture<'_, Result<Probed, Unanswered>>>,
) -> Option<Check> {
    match harness {
        Harness::Codex => Some(match version {
            Some(version) => crate::codex::launch::native_queue(asking, executable, version).await,
            None => crate::codex::launch::check(asking, executable).await,
        }),
        Harness::Devin => Some(crate::devin::install::version_check(asking, executable).await),
        Harness::Muse => Some(crate::muse::launch::pinned(asking, executable).await.check),
        Harness::Claude | Harness::Pi => None,
    }
}
