//! What a release must find true before it moves a feed, and after.
//!
//! Before: for the bridge there is nothing the feeds can say yet; for a later
//! release, the old feeds of the channels in use serve the bridge, with its
//! files ([`check_prerequisites`]). After: every file of the release downloads
//! from where it was published as the one built, its latest.json names its
//! archive, each new feed of its channels serves that latest.json, and the old
//! feeds serve the bridge ([`check_feeds`]).
//!
//! No run moves a feed backward, so a new feed may serve a later release than
//! the one checked: that is what a run of this release leaves when it is run
//! again after the later one went out. It is the only other thing a new feed
//! may serve. And since a feed that went back to the release checked would
//! serve it, the publisher leaves the check a record of what each new feed
//! named before it changed any ([`FEEDS_BEFORE`]), which a feed that now names
//! an earlier release than that is a problem against.
//!
//! Each check answers with the problems it found, as the sentences a person
//! reads: a feed or file that cannot be read is not absent, it is a problem,
//! whichever way it cannot be read.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::assets::{name_of, on_disk, ReleaseAssets, TARGET};
use crate::digest::sha256_hex;
use crate::failure::Failure;
use crate::manifest::Manifest;
use crate::read::{Patience, Reader};
use crate::rule::{role_of, Role};
use crate::version::{later_release, named_release, Channel, Version};

/// The file the publisher leaves in the release's folder, beside its
/// SHA256SUMS, for the check that follows: the release each new feed named, or
/// none, when the publisher read it before changing any. The check cannot tell,
/// from the feed alone, that it went back to the release it is asked about;
/// this is what lets it.
pub const FEEDS_BEFORE: &str = "feeds-before.json";

/// How a JSON value reads in a sentence: text as it is, and what is not there as
/// `undefined`.
fn shown(value: Option<&Value>) -> String {
    match value {
        None => "undefined".to_string(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// The archive a latest.json names for the Mac.
fn archive_named(document: &Value) -> Option<&Value> {
    document.get("platforms")?.get(TARGET)?.get("url")
}

/// The hashes a SHA256SUMS lists, by file name: lines of `<64 hex> <space or
/// *><name>`, as sha256sum writes them. A name listed twice keeps the last.
fn listed_in(sums: &str) -> HashMap<&str, &str> {
    let mut listed = HashMap::new();
    for line in sums.lines() {
        let bytes = line.as_bytes();
        let hash_ok = bytes.len() > 66
            && bytes[..64]
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte));
        if hash_ok && bytes[64] == b' ' && (bytes[65] == b' ' || bytes[65] == b'*') {
            listed.insert(&line[66..], &line[..64]);
        }
    }
    listed
}

/// What the old channels in use must serve, and have, for the bridge to have
/// reached the apps that read them: each serves the bridge's own latest.json
/// (the one its release published) byte for byte, that names the bridge and its
/// archive, and the bridge's Mac archive, installer and portable download as
/// its SHA256SUMS says.
fn bridge_problems(base: &str, manifest: &Manifest, reads: &mut Reader) -> Vec<String> {
    let version = &manifest.bridge.version;
    let tag = format!("v{version}");
    let assets = ReleaseAssets::of(version);
    let published = match reads.file(&format!("{base}/{tag}/latest.json")) {
        Ok(body) => body,
        Err(refusal) => {
            return vec![format!(
                "the bridge's {tag}/latest.json cannot be read ({}), so no feed can be held to it",
                refusal.why
            )]
        }
    };
    let Ok(document) = serde_json::from_slice::<Value>(&published) else {
        return vec![format!(
            "the bridge's {tag}/latest.json is not a latest.json"
        )];
    };
    let mut problems = Vec::new();
    let archive = format!("{base}/{tag}/{}", name_of(&assets.archive));
    if document.get("version").and_then(Value::as_str) != Some(version.as_str()) {
        problems.push(format!(
            "the bridge's {tag}/latest.json names {}, not {version}",
            shown(document.get("version"))
        ));
    }
    if archive_named(&document).and_then(Value::as_str) != Some(archive.as_str()) {
        problems.push(format!(
            "the bridge's {tag}/latest.json names {} for the Mac, not {archive}",
            archive_named(&document)
                .map_or_else(|| "no archive".to_string(), |url| shown(Some(url)))
        ));
    }
    for channel in &manifest.bridge.legacy {
        let feed = manifest.legacy.of(*channel);
        let found = match reads.feed(&format!("{base}/{feed}/latest.json"), &published) {
            Ok(body) => body,
            Err(refusal) => {
                problems.push(format!(
                    "{feed} cannot be read ({}): the apps before the bridge read it",
                    refusal.why
                ));
                continue;
            }
        };
        if found == published {
            continue;
        }
        let named = match serde_json::from_slice::<Value>(&found) {
            Ok(Value::Null) | Err(_) => {
                problems.push(format!("{feed} serves something that is not a latest.json"));
                continue;
            }
            Ok(document) => document,
        };
        let named = named.get("version");
        problems.push(if named.and_then(Value::as_str) == Some(version.as_str()) {
            format!("{feed} names the bridge {version}, but is not the bridge's own latest.json byte for byte")
        } else {
            format!(
                "{feed} serves {}, not the bridge {version}: the apps that read it do not reach the bridge",
                shown(named)
            )
        });
    }
    let sums = match reads.file(&format!("{base}/{tag}/SHA256SUMS")) {
        Ok(body) => body,
        Err(refusal) => {
            problems.push(format!(
                "the bridge's {tag}/SHA256SUMS cannot be read ({})",
                refusal.why
            ));
            return problems;
        }
    };
    let sums = String::from_utf8_lossy(&sums);
    let listed = listed_in(&sums);
    for path in assets.retained() {
        let name = name_of(path);
        let Some(wanted) = listed.get(name) else {
            problems.push(format!("the bridge's {tag}/SHA256SUMS lists no {name}"));
            continue;
        };
        match reads.hash(&format!("{base}/{tag}/{name}")) {
            Err(refusal) => problems.push(format!(
                "the bridge's {name} cannot be downloaded ({})",
                refusal.why
            )),
            Ok(got) if got != *wanted => problems.push(format!(
                "the bridge's {name} does not download as the one its SHA256SUMS lists"
            )),
            Ok(_) => {}
        }
    }
    problems
}

/// What the release's own latest.json (`latest`, bytes) must say: its version,
/// and its archive at the download address it was published to.
fn metadata_problems(latest: &[u8], version: &Version, base: &str) -> Vec<String> {
    let wanted = format!(
        "{base}/v{version}/{}",
        name_of(&ReleaseAssets::of(version).archive)
    );
    let Ok(document) = serde_json::from_slice::<Value>(latest) else {
        return vec!["this release's latest.json is not JSON".to_string()];
    };
    let mut problems = Vec::new();
    if document.get("version").and_then(Value::as_str) != Some(version.as_str()) {
        problems.push(format!(
            "this release's latest.json names {}, not {version}",
            shown(document.get("version"))
        ));
    }
    if archive_named(&document).and_then(Value::as_str) != Some(wanted.as_str()) {
        problems.push(format!(
            "this release's latest.json names {} for the Mac, not {wanted}",
            archive_named(&document)
                .map_or_else(|| "no archive".to_string(), |url| shown(Some(url)))
        ));
    }
    problems
}

/// The bytes of `path` in the folder `dir` that was built.
fn built(dir: &Path, path: &str) -> Result<Vec<u8>, Failure> {
    fs::read(on_disk(dir, path)).map_err(|cause| {
        Failure::new(format!(
            "could not read {path} in {}: {cause}",
            dir.display()
        ))
    })
}

/// Whether every file of the release built in `dir` downloads from where it was
/// published as the one built, which is a hash read off the download beside a
/// hash of the file: the archive the apps install, the Windows files, the
/// metadata, the sums.
pub fn asset_problems(
    dir: &Path,
    version: &Version,
    base: &str,
    reads: &mut Reader,
) -> Result<Vec<String>, Failure> {
    let tag = format!("v{version}");
    let assets = ReleaseAssets::of(version);
    let mut problems = Vec::new();
    for path in assets.all().into_iter().chain(["SHA256SUMS"]) {
        let name = name_of(path);
        match reads.hash(&format!("{base}/{tag}/{name}")) {
            Err(refusal) => problems.push(format!(
                "{name} cannot be downloaded from {tag} ({})",
                refusal.why
            )),
            Ok(got) if got != sha256_hex(&built(dir, path)?) => {
                problems.push(format!(
                    "{name} does not download from {tag} as the one built"
                ));
            }
            Ok(_) => {}
        }
    }
    Ok(problems)
}

/// What a release must find true before it moves any feed. For the bridge there
/// is nothing the feeds can say yet; for a later release, the old channels in
/// use serve the bridge with its files. The problems found refuse the release.
pub fn check_prerequisites(
    version: &Version,
    base: &str,
    manifest: &Manifest,
    patience: Patience,
) -> Vec<String> {
    match role_of(version, manifest) {
        Err(refusal) => vec![refusal.to_string()],
        Ok(Role::Bridge) => Vec::new(),
        Ok(Role::Later) => bridge_problems(base, manifest, &mut Reader::new(patience)),
    }
}

/// What `dir`'s [`FEEDS_BEFORE`] says each feed named: the ones that named a
/// release, and none where there is no such file (a check run by hand).
fn named_before(dir: &Path) -> HashMap<String, Version> {
    let Ok(text) = fs::read(dir.join(FEEDS_BEFORE)) else {
        return HashMap::new();
    };
    let Ok(Value::Object(record)) = serde_json::from_slice(&text) else {
        return HashMap::new();
    };
    record
        .into_iter()
        .filter_map(|(feed, named)| Some((feed, Version::parse(named.as_str()?).ok()?)))
        .collect()
}

/// What the feeds serve once the release built in `dir` is published, as the
/// rule says: every file of the release downloads as the one built and its
/// latest.json names its archive, each new feed of its channels serves that
/// latest.json, and the old channels in use serve the bridge with its files,
/// this release being that bridge or a later one. A new feed may name a later
/// release instead: that is what a run of this release leaves when it is run
/// again after the later one went out, since a feed is never moved backward. It
/// is the only other thing a new feed may serve: one that names an earlier
/// release, or this one with other bytes, or cannot be read, is a problem. So is
/// one that names an earlier release than it did when the publisher read it
/// ([`FEEDS_BEFORE`], where the folder holds one): a feed that serves this
/// release having gone back to it is told by that alone. An old feed of a
/// channel not in use may be absent, and when it is there does not serve this
/// release, which the rule does not move it to.
pub fn check_feeds(
    dir: &Path,
    version: &Version,
    base: &str,
    manifest: &Manifest,
    patience: Patience,
) -> Result<Vec<String>, Failure> {
    if let Err(refusal) = role_of(version, manifest) {
        return Ok(vec![refusal.to_string()]);
    }
    let mut reads = Reader::new(patience);
    let latest = built(dir, &ReleaseAssets::of(version).metadata)?;
    let mut problems = asset_problems(dir, version, base, &mut reads)?;
    problems.extend(metadata_problems(&latest, version, base));
    let before = named_before(dir);
    for channel in version.channels() {
        let feed = manifest.feeds.of(*channel);
        let found = reads.feed(&format!("{base}/{feed}/latest.json"), &latest);
        let body = match &found {
            Ok(body) if *body == latest || later_release(body, version).is_some() => body,
            Ok(_) => {
                problems.push(format!("{feed} does not serve this release's latest.json"));
                continue;
            }
            Err(refusal) => {
                problems.push(format!(
                    "{feed} does not serve this release's latest.json ({})",
                    refusal.why
                ));
                continue;
            }
        };
        if let (Some(now), Some(was)) = (named_release(body), before.get(feed)) {
            if now < *was {
                problems.push(format!(
                    "{feed} went backward: it named {was} when this release was published, and names {now} now"
                ));
            }
        }
    }
    problems.extend(bridge_problems(base, manifest, &mut reads));
    for channel in Channel::ALL
        .into_iter()
        .filter(|channel| !manifest.bridge.legacy.contains(channel))
    {
        let feed = manifest.legacy.of(channel);
        let body = match reads.once(&format!("{base}/{feed}/latest.json")) {
            Ok(body) => body,
            Err(refusal) if refusal.status == Some(404) => continue,
            Err(refusal) => {
                problems.push(format!("{feed} cannot be read ({})", refusal.why));
                continue;
            }
        };
        match serde_json::from_slice::<Value>(&body) {
            Ok(Value::Null) | Err(_) => {
                problems.push(format!("{feed} serves something that is not a latest.json"));
            }
            Ok(document) => {
                if document.get("version").and_then(Value::as_str) == Some(version.as_str()) {
                    problems.push(format!(
                        "{feed} serves this release, which the rule does not move it to"
                    ));
                }
            }
        }
    }
    Ok(problems)
}
