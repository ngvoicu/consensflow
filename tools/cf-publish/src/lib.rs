//! The release's publisher, `cf-publish`: the rule of the update feeds and its
//! checks (`feeds plan`, `feeds prerequisites`, `feeds check`), and `publish`,
//! which makes a version's release on GitHub and moves the feeds installed apps
//! read, so that a run that died is finished by running it again.
//!
//! An installed app reads the `latest.json` of a rolling GitHub release, the
//! feed of its channel. The apps before the bridge (3.0.0-alpha.80 and earlier)
//! read `update-alpha` and `update-stable`; the bridge and every release after
//! it read `feed-alpha` and `feed-stable` (`app/feeds.json` names both
//! generations of feeds, and the bridge, and is [`manifest`]'s). The bridge is
//! the first release that reads the new feeds, and the one the old apps are
//! offered: they install it because it keeps the layout they check for (Node,
//! cf.mjs, src, hosts, package.json), and from it on they read the new feeds.
//!
//! Two facts, kept apart. Which release is the bridge is recorded in the
//! manifest: it is a decision, made when the release is cut. That the bridge
//! reached the old apps is not implied by it, nor by a tag that holds the
//! manifest, nor by any git history: a release that failed before or while it
//! moved `update-alpha` leaves that tag behind and the old apps where they
//! were. It is read from where the old apps read it, every time it matters
//! ([`checks`]): the old feed of each channel in use serves the bridge's own
//! latest.json byte for byte, and the bridge's Mac archive, installer and
//! portable download as they were published (its SHA256SUMS). The rule
//! ([`rule`]):
//!
//! - the bridge moves the new feed of each of its channels and the old feed of
//!   each channel in use, once. A bridge whose archive lacks what the old apps
//!   check for is refused before anything moves;
//! - a later release moves the new feeds only, and only when the bridge has
//!   reached the old feeds, which it establishes before it moves anything: when
//!   the old feeds do not serve the bridge, with its files, it is refused and
//!   nothing moves. The old feeds stay pinned to the bridge, and its files stay:
//!   nothing here deletes a release;
//! - a release before the bridge is refused: it would read feeds no installed
//!   app reads, and name a bridge that is not the first;
//! - no run moves a feed backward. A feed that already names a later release
//!   than the run's own (by the precedence [`version::Version`] gives) is left
//!   as it is, each feed judged alone: the job of a release run again after a
//!   later one went out finishes what is left of it and leaves that feed at the
//!   later release, and a stable release leaves the alpha feed alone while the
//!   alpha line is ahead and still moves the stable one. The check afterwards
//!   accepts such a feed, and no other that does not serve the release.
//!
//! Where it runs. The Mac job of the release workflow builds this crate (with
//! `--locked`, in a step that holds no secret) and asks the binary for the
//! `plan` and the `prerequisites`. The `publish` job runs the binary that a job
//! of its own built, where no secret is, after checking it against the hash
//! that job reported; that job holds the repository's write token, and so this
//! crate is built with `serde_json` and `sha2` and nothing else, which
//! `tests/closure.rs` holds to a list. It starts three programs, `curl` for
//! what it reads, `gh` for GitHub and `tar` for the list of an archive, all of
//! them the runner's ([`process`] is the one place that does).
//!
//! The modules follow the rule's parts: [`manifest`], [`version`], [`rule`],
//! [`assets`] (the release's files), [`read`] (the reads through `curl`),
//! [`checks`], [`gh`] and [`publish`], with [`cli`] the command line. `testing`
//! (behind `test-support`) is GitHub on this machine, for the tests.

#![forbid(unsafe_code)]

pub mod assets;
pub mod checks;
pub mod cli;
pub mod digest;
pub mod failure;
pub mod gh;
pub mod manifest;
mod process;
pub mod publish;
pub mod read;
pub mod rule;
pub mod version;

// GitHub on this machine, and what the tests build their worlds of: a failure
// in it is the test's.
#[cfg(any(test, feature = "test-support"))]
#[allow(clippy::expect_used, clippy::unwrap_used)]
pub mod testing;

pub use cli::{run, Environment};
