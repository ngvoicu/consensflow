//! The rule: which feeds a release moves, and what it must be refused for.
//!
//! The bridge moves the new feed of each of its channels and the old feed of
//! each channel in use, once. A later release moves the new feeds only. A
//! release before the bridge is refused: it would read feeds no installed app
//! reads, and name a bridge that is not the first. That the bridge reached the
//! old apps is not implied by the manifest naming it: `checks` reads it, from
//! the feeds, every time it matters.

use std::cmp::Ordering;

use crate::failure::Failure;
use crate::manifest::Manifest;
use crate::version::Version;

/// What a release is to the bridge the manifest names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The bridge itself.
    Bridge,
    /// A release after it.
    Later,
}

impl Role {
    /// How the plan says it.
    pub fn describe(self) -> &'static str {
        match self {
            Role::Bridge => "the bridge",
            Role::Later => "a later release",
        }
    }
}

/// What `version` is to the bridge `manifest` names. A release before it is refused.
pub fn role_of(version: &Version, manifest: &Manifest) -> Result<Role, Failure> {
    let bridge = &manifest.bridge.version;
    let order = version.cmp(bridge);
    if order == Ordering::Less {
        return Err(Failure::new(format!(
            "{version} comes before the bridge {bridge} that app/feeds.json names, and it would read feeds that no installed app does: set bridge.version to the first release made from this tree"
        )));
    }
    Ok(if order == Ordering::Equal {
        Role::Bridge
    } else {
        Role::Later
    })
}

/// The feeds a release moves: the new feed of each of its channels, and for the
/// bridge the old ones in use too.
pub fn feeds_moved(version: &Version, manifest: &Manifest) -> Result<Vec<String>, Failure> {
    let mut feeds: Vec<String> = version
        .channels()
        .iter()
        .map(|channel| manifest.feeds.of(*channel).to_string())
        .collect();
    if role_of(version, manifest)? == Role::Later {
        return Ok(feeds);
    }
    feeds.extend(
        manifest
            .bridge
            .legacy
            .iter()
            .map(|channel| manifest.legacy.of(*channel).to_string()),
    );
    Ok(feeds)
}

/// The feeds a release moves, or why it may not: it comes before the bridge, or
/// it is the bridge and its archive (`missing`, see
/// [`missing_for_old_apps`](crate::assets::missing_for_old_apps)) lacks what the
/// old apps would refuse it for.
pub fn plan_feeds(
    version: &Version,
    missing: &[String],
    manifest: &Manifest,
) -> Result<Vec<String>, Failure> {
    let feeds = feeds_moved(version, manifest)?;
    if role_of(version, manifest)? == Role::Bridge && !missing.is_empty() {
        return Err(Failure::new(format!(
            "the bridge must keep the layout the apps before it check for: its archive lacks {}",
            missing.join(", ")
        )));
    }
    Ok(feeds)
}
