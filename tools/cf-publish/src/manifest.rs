//! `app/feeds.json`, as the rule reads it: the feeds of both generations, and
//! the bridge. The binary holds the file as it was when it was built, so the
//! job that publishes needs nothing of the tree, and the app's own test
//! (`app/src-tauri/src/updates.rs`) reads the same file for the addresses the
//! installed apps read.
//!
//! An installed app reads the `latest.json` of a rolling GitHub release, the
//! feed of its channel. The apps before the bridge (3.0.0-alpha.80 and earlier)
//! read `update-alpha` and `update-stable` (`legacy`); the bridge and every
//! release after it read `feed-alpha` and `feed-stable` (`feeds`). Which release
//! is the bridge is a decision, made when the release is cut: `bridge.version`.
//! `bridge.legacy` names the channels of the old generation that have a release
//! in use, which are the ones the bridge moves an old feed for.

use serde_json::Value;

use crate::failure::Failure;
use crate::version::{Channel, Version};

/// `app/feeds.json` as it was when this was built.
const EMBEDDED: &str = include_str!("../../../app/feeds.json");

/// The name of a feed for each channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feeds {
    alpha: String,
    stable: String,
}

impl Feeds {
    /// The feed of `channel`.
    pub fn of(&self, channel: Channel) -> &str {
        match channel {
            Channel::Alpha => &self.alpha,
            Channel::Stable => &self.stable,
        }
    }

    /// Whether `feed` is one of the two.
    pub fn contains(&self, feed: &str) -> bool {
        self.alpha == feed || self.stable == feed
    }
}

/// The bridge: the first release that reads the new feeds, and the one the old
/// apps are offered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bridge {
    pub version: Version,
    /// The channels whose old feed the bridge moves.
    pub legacy: Vec<Channel>,
}

/// What the rule needs of `app/feeds.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub feeds: Feeds,
    pub legacy: Feeds,
    pub bridge: Bridge,
}

/// The feed that `group.channel` names in `document`.
fn feed_of(document: &Value, group: &str, channel: Channel) -> Result<String, Failure> {
    document
        .get(group)
        .and_then(|feeds| feeds.get(channel.name()))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            Failure::new(format!(
                "app/feeds.json: {group}.{} names no feed",
                channel.name()
            ))
        })
}

fn feeds_of(document: &Value, group: &str) -> Result<Feeds, Failure> {
    Ok(Feeds {
        alpha: feed_of(document, group, Channel::Alpha)?,
        stable: feed_of(document, group, Channel::Stable)?,
    })
}

impl Manifest {
    /// `document` itself, once it says what the rule needs of it: the feeds of
    /// both generations, none named twice, and a bridge that is a release and
    /// names, among the channels it belongs to, the ones in use.
    pub fn from_json(document: &Value) -> Result<Self, Failure> {
        let feeds = feeds_of(document, "feeds")?;
        let legacy = feeds_of(document, "legacy")?;
        let named = [
            feeds.of(Channel::Alpha),
            feeds.of(Channel::Stable),
            legacy.of(Channel::Alpha),
            legacy.of(Channel::Stable),
        ];
        let distinct: std::collections::BTreeSet<_> = named.iter().collect();
        if distinct.len() != named.len() {
            return Err(Failure::new("app/feeds.json: a feed is named twice"));
        }
        let bridge = document.get("bridge");
        let Some(version) = bridge
            .and_then(|bridge| bridge.get("version"))
            .and_then(Value::as_str)
        else {
            return Err(Failure::new(
                "app/feeds.json: bridge.version names no release",
            ));
        };
        let version = Version::parse(version)?;
        let belongs = version.channels();
        let in_use = bridge.and_then(|bridge| bridge.get("legacy"));
        let channels: Option<Vec<Channel>> = in_use
            .and_then(Value::as_array)
            .filter(|listed| !listed.is_empty())
            .and_then(|listed| {
                listed
                    .iter()
                    .map(|name| name.as_str().and_then(Channel::from_name))
                    .collect()
            })
            .filter(|channels: &Vec<Channel>| channels.iter().all(|c| belongs.contains(c)));
        let Some(channels) = channels else {
            let belongs: Vec<_> = belongs.iter().map(|channel| channel.name()).collect();
            return Err(Failure::new(format!(
                "app/feeds.json: bridge.legacy names the channels in use that {version} belongs to ({}); it names {}",
                belongs.join(", "),
                in_use.map_or_else(|| "undefined".to_string(), Value::to_string),
            )));
        };
        Ok(Self {
            feeds,
            legacy,
            bridge: Bridge {
                version,
                legacy: channels,
            },
        })
    }

    /// The manifest this was built with.
    pub fn embedded() -> Result<Self, Failure> {
        let document: Value = serde_json::from_str(EMBEDDED)
            .map_err(|cause| Failure::new(format!("app/feeds.json: {cause}")))?;
        Self::from_json(&document)
    }
}
