//! `cargo xtask release verify X.Y.Z[-alpha.N]`: what a published release is, read
//! from where people download it, and the step `release alpha` and `release promote`
//! end with once the feeds serve their version. It reads and changes nothing but a
//! folder of its own, which it removes.
//!
//! It holds the release to what T-862 checked by hand on 3.0.6-alpha.3:
//!
//! 1. every file against the release's `SHA256SUMS`: each file it lists is in the
//!    release and hashes as listed, and the release holds no file it does not list;
//! 2. the update archive's `.sig` against the public key pinned in
//!    `app/src-tauri/tauri.conf.json` **at the release's tag** (what an app of that
//!    version carries), the file's signature and the trusted comment's both, by the
//!    crate Tauri's updater verifies with; and, once the archive verified, as a
//!    control that a copy of it with one byte changed is refused, so that a check
//!    which accepts anything is told;
//! 3. each feed that serves this version: its `latest.json` is the release's, its
//!    signature for the Mac is the `.sig`, and the archive its address gives is the
//!    release's, byte for byte. A feed that serves a later release is told and left
//!    alone (it no longer serves this one); one that serves an earlier one, or none,
//!    is a problem.
//!
//! Every problem found is said, not the first alone: a release that does not verify
//! is refused with all of them. A release that cannot be read at all (no such
//! release, no `SHA256SUMS`, a tag nobody has) is refused as it is.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cf_publish::digest::hex;
use cf_publish::version::{named_release, Version};
use minisign_verify::{PublicKey, Signature};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{refused, Error, Release, ALPHA_FEED, REPOSITORY, STABLE_FEED};
use crate::dispatch::Failure;
use crate::process::Invocation;

/// The file the update's archive is signed for, and the one whose entry a feed carries.
const PLATFORM: &str = "darwin-aarch64";

/// Where the updater's public key is pinned.
const CONF: &str = "app/src-tauri/tauri.conf.json";

/// What a published release was found to be.
#[derive(Debug, Default)]
pub(super) struct Report {
    /// What held, as sentences.
    pub held: Vec<String>,
    /// What did not, as sentences.
    pub problems: Vec<String>,
}

/// The version `release verify` was given: one, a canonical one.
pub(super) fn version_of(args: &[OsString]) -> Result<String, Failure> {
    let mut version = None;
    for word in args {
        match word.to_str() {
            Some(text) if version.is_none() && !text.starts_with('-') => {
                version = Some(text.to_owned());
            }
            _ => {
                return Err(Failure::Usage(format!(
                    "release verify does not take {}",
                    word.to_string_lossy()
                )))
            }
        }
    }
    let version = version.ok_or_else(|| {
        Failure::Usage("release verify takes the version to verify, X.Y.Z or X.Y.Z-alpha.N".into())
    })?;
    cf_release::version::canonical("the version", &version)
        .map_err(|cause| Failure::Usage(cause.to_string()))?;
    Ok(version)
}

/// `release verify`: what the release held to, or why it does not verify.
pub(super) fn release(release: &mut Release, version: &str) -> Result<String, Error> {
    let report = release.verify(version)?;
    if !report.problems.is_empty() {
        return Err(refused(does_not_verify(version, &report.problems)));
    }
    let lines: String = report
        .held
        .iter()
        .map(|line| format!("  {line}\n"))
        .collect();
    Ok(format!(
        "release verify: v{version} on {REPOSITORY}\n{lines}"
    ))
}

/// What a refusal says of a release with `problems`.
fn does_not_verify(version: &str, problems: &[String]) -> String {
    let plural = if problems.len() == 1 { "" } else { "s" };
    let lines: String = problems
        .iter()
        .map(|problem| format!("\n- {problem}"))
        .collect();
    format!(
        "v{version} does not verify, {} problem{plural}:{lines}",
        problems.len()
    )
}

impl Release<'_> {
    /// Checks the release of `version` as it is published. Every problem is in the
    /// report; what could not be read at all is the error.
    pub(super) fn verify(&mut self, version: &str) -> Result<Report, Error> {
        let tag = format!("v{version}");
        let parent = self
            .root
            .join("app")
            .join("src-tauri")
            .join("target")
            .join("verify");
        let folder = parent.join(&tag);
        // One a run that died left is taken away first.
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).map_err(|source| Error::File {
            action: "make",
            path: folder.clone(),
            source,
        })?;
        let checked = self.verify_in(version, &tag, &folder);
        let removed = fs::remove_dir_all(&folder);
        // The folder that held it, once nothing else is in it.
        let _ = fs::remove_dir(&parent);
        let report = checked?;
        removed.map_err(|source| Error::File {
            action: "remove",
            path: folder,
            source,
        })?;
        Ok(report)
    }

    /// The same, with the release's files downloaded into `folder`.
    fn verify_in(&mut self, version: &str, tag: &str, folder: &Path) -> Result<Report, Error> {
        let in_folder = folder.to_string_lossy().into_owned();
        let download = Invocation::new("gh", &self.root).args([
            "release", "download", tag, "--repo", REPOSITORY, "--dir", &in_folder,
        ]);
        let answered = self.system.capture(&download)?;
        if answered.code != 0 {
            return Err(refused(format!(
                "{tag} has no release to download: `gh release download {tag} --repo {REPOSITORY} --dir <the folder>` \
                 ended with status {}: {}",
                answered.code,
                answered.stderr.trim()
            )));
        }
        let names = names_in(folder)?;
        if !names.iter().any(|name| name == "SHA256SUMS") {
            return Err(refused(format!(
                "{tag}'s release has no SHA256SUMS, so no file of it can be checked"
            )));
        }
        let mut report = Report::default();
        self.check_sums(folder, &names, &mut report)?;

        let archive = format!("ConsensFlow_{version}_aarch64.app.tar.gz");
        let signature = format!("{archive}.sig");
        let mut lacking = false;
        for needed in [archive.as_str(), signature.as_str(), "latest.json"] {
            if !names.iter().any(|name| name == needed) {
                report.problems.push(format!(
                    "{needed} is not in the release, and the update needs it"
                ));
                lacking = true;
            }
        }
        if lacking {
            return Ok(report);
        }
        self.check_signature(tag, folder, &archive, &signature, &mut report)?;
        for feed in feeds_of(version) {
            self.check_feed(feed, version, folder, &archive, &signature, &mut report)?;
        }
        Ok(report)
    }

    /// Every file `SHA256SUMS` lists is in the release and hashes as listed, and
    /// the release holds no file it does not list.
    fn check_sums(
        &mut self,
        folder: &Path,
        names: &[String],
        report: &mut Report,
    ) -> Result<(), Error> {
        let path = folder.join("SHA256SUMS");
        let text = fs::read_to_string(&path).map_err(|source| Error::File {
            action: "read",
            path,
            source,
        })?;
        let (listed, odd) = listed_in(&text);
        let before = report.problems.len();
        for line in odd {
            report.problems.push(format!(
                "SHA256SUMS holds a line that is not \"<sha-256>  <name>\": \"{line}\""
            ));
        }
        for (name, hash) in &listed {
            if !names.contains(name) {
                report.problems.push(format!(
                    "{name} is listed in SHA256SUMS and is not in the release"
                ));
                continue;
            }
            let got = sha256_of(&folder.join(name))?;
            if got != *hash {
                report.problems.push(format!(
                    "{name} hashes to {got}, and SHA256SUMS lists {hash}"
                ));
            }
        }
        for name in names {
            if name != "SHA256SUMS" && !listed.contains_key(name) {
                report.problems.push(format!(
                    "{name} is in the release and SHA256SUMS does not list it"
                ));
            }
        }
        if report.problems.len() == before {
            report.held.push(format!(
                "SHA256SUMS lists {} files, each in the release and hashing as listed",
                listed.len()
            ));
        }
        Ok(())
    }

    /// The archive's `.sig` against the key the tag's `tauri.conf.json` pins, and a
    /// control that a copy with a byte changed is refused.
    fn check_signature(
        &mut self,
        tag: &str,
        folder: &Path,
        archive: &str,
        signature: &str,
        report: &mut Report,
    ) -> Result<(), Error> {
        let key = self.pinned_key(tag)?;
        let sig_path = folder.join(signature);
        let sig_text = fs::read_to_string(&sig_path).map_err(|source| Error::File {
            action: "read",
            path: sig_path,
            source,
        })?;
        let parsed = STANDARD
            .decode(sig_text.trim())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or(minisign_verify::Error::InvalidEncoding)
            .and_then(|text| Signature::decode(&text));
        let parsed = match parsed {
            Ok(parsed) => parsed,
            Err(cause) => {
                report
                    .problems
                    .push(format!("{signature} is not a minisign signature: {cause}"));
                return Ok(());
            }
        };
        let path = folder.join(archive);
        let mut bytes = fs::read(&path).map_err(|source| Error::File {
            action: "read",
            path,
            source,
        })?;
        if let Err(cause) = key.verify(&bytes, &parsed, false) {
            report.problems.push(format!(
                "{archive} does not verify against the key {tag}'s tauri.conf.json pins: {cause}"
            ));
            return Ok(());
        }
        report.held.push(format!(
            "{archive} is signed by the key {tag}'s tauri.conf.json pins ({}): \
             the file and its trusted comment hold, \"{}\"",
            key.untrusted_comment()
                .map(|comment| comment.trim_start_matches("untrusted comment: "))
                .unwrap_or("a key with no comment"),
            parsed.trusted_comment()
        ));
        // The control, once the archive verified: the same check on a copy that is not
        // what was signed. A check that accepts it accepts anything.
        match bytes.first_mut() {
            Some(first) => *first ^= 1,
            None => bytes.push(0),
        }
        if key.verify(&bytes, &parsed, false).is_ok() {
            report.problems.push(format!(
                "the signature check accepted a copy of {archive} with one byte changed"
            ));
        } else {
            report.held.push(format!(
                "a copy of {archive} with one byte changed is refused"
            ));
        }
        Ok(())
    }

    /// The updater's public key as the tag's `tauri.conf.json` pins it: what an app
    /// of that version carries. A tag the checkout lacks is fetched by name first.
    fn pinned_key(&mut self, tag: &str) -> Result<PublicKey, Error> {
        let spec = format!("{tag}:{CONF}");
        let conf = match self.git(&["show", &spec]) {
            Ok(conf) => conf,
            Err(_) => {
                let reference = format!("refs/tags/{tag}");
                let fetch = format!("+{reference}:{reference}");
                self.git(&["fetch", "-q", "--no-tags", "upstream", &fetch])
                    .map_err(|cause| {
                        refused(format!(
                            "{tag} is not in the checkout, and upstream does not have it either: {cause}"
                        ))
                    })?;
                self.git(&["show", &spec]).map_err(|cause| {
                    refused(format!(
                        "{tag} holds no {CONF} to read the key from: {cause}"
                    ))
                })?
            }
        };
        let unusable =
            |why: String| refused(format!("{CONF} at {tag} pins no usable updater key: {why}"));
        let document: Value =
            serde_json::from_str(&conf).map_err(|cause| unusable(cause.to_string()))?;
        let pinned = document
            .pointer("/plugins/updater/pubkey")
            .and_then(Value::as_str)
            .ok_or_else(|| unusable("plugins.updater.pubkey is not there".to_owned()))?;
        let text = STANDARD
            .decode(pinned.trim())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or_else(|| unusable("it is not the base64 of a minisign public key".to_owned()))?;
        PublicKey::decode(&text).map_err(|cause| unusable(cause.to_string()))
    }

    /// What a feed serves, held to the release when it serves this version.
    fn check_feed(
        &mut self,
        feed: &str,
        version: &str,
        folder: &Path,
        archive: &str,
        signature: &str,
        report: &mut Report,
    ) -> Result<(), Error> {
        let address = Self::feed_address(feed);
        let invocation = Invocation::new("curl", &self.root)
            .args(["-sfL", "--max-time", "60"])
            .arg(&address);
        let answered = self.system.capture(&invocation)?;
        let body = match answered.code {
            0 => answered.stdout,
            22 => String::new(),
            code => {
                report.problems.push(format!(
                    "{feed} could not be read: curl ended {code}: {}",
                    answered.stderr.trim()
                ));
                return Ok(());
            }
        };
        let Some(served) = named_release(body.as_bytes()) else {
            report
                .problems
                .push(format!("{feed} could not be read: it serves no release"));
            return Ok(());
        };
        let ours = Version::parse(version).map_err(|cause| refused(cause.to_string()))?;
        if served > ours {
            report.held.push(format!(
                "{feed} serves {served}, a later release than {version}: not checked"
            ));
            return Ok(());
        }
        if served < ours {
            report
                .problems
                .push(format!("{feed} serves {served}, not {version}"));
            return Ok(());
        }
        let before = report.problems.len();
        let read = |name: &str| -> Result<String, Error> {
            let path = folder.join(name);
            fs::read_to_string(&path).map_err(|source| Error::File {
                action: "read",
                path,
                source,
            })
        };
        if body != read("latest.json")? {
            report
                .problems
                .push(format!("{feed}'s latest.json is not the release's"));
        }
        let entry = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|document| document.pointer(&format!("/platforms/{PLATFORM}")).cloned());
        let named = |key: &str| {
            entry
                .as_ref()
                .and_then(|entry| entry.get(key))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        let (Some(signed), Some(url)) = (named("signature"), named("url")) else {
            report
                .problems
                .push(format!("{feed}'s latest.json names no {PLATFORM} archive"));
            return Ok(());
        };
        if signed.trim() != read(signature)?.trim() {
            report
                .problems
                .push(format!("{feed}'s signature for {PLATFORM} is not the .sig"));
        }
        let fetched = folder.join(format!("{feed}-archive"));
        let download = Invocation::new("curl", &self.root)
            .args(["-sfL", "--max-time", "600", "-o"])
            .arg(&fetched)
            .arg(&url);
        let answered = self.system.capture(&download)?;
        if answered.code != 0 {
            report.problems.push(format!(
                "{feed}'s archive could not be read from {url}: curl ended {}: {}",
                answered.code,
                answered.stderr.trim()
            ));
        } else if sha256_of(&fetched)? != sha256_of(&folder.join(archive))? {
            report.problems.push(format!(
                "{feed}'s archive, read from {url}, is not the release's"
            ));
        }
        if report.problems.len() == before {
            report.held.push(format!(
                "{feed} serves {version}: its latest.json is the release's, its signature the .sig, \
                 and its archive, read from its address, the release's"
            ));
        }
        Ok(())
    }

    /// The step a release ends with, once the Release run `run` ended green and
    /// the feeds serve `version`: the release is published, so what it finds is
    /// said as that, and never as a reason to run again.
    pub(super) fn verify_published(&mut self, version: &str, run: u64) -> Result<(), Error> {
        let published = format!(
            "release run {run} ended green and the feeds serve {version}, so the release is published"
        );
        let read_by_hand =
            format!("run nothing again: `cargo xtask release verify {version}` checks it again");
        let report = self.verify(version).map_err(|cause| {
            refused(format!(
                "{published}, yet it could not be checked ({cause}); {read_by_hand}"
            ))
        })?;
        if !report.problems.is_empty() {
            return Err(refused(format!(
                "{published}, yet checking it found {}; {read_by_hand}",
                does_not_verify(version, &report.problems)
            )));
        }
        for line in &report.held {
            self.say(&format!("verify: {line}"));
        }
        Ok(())
    }
}

/// The feeds that serve a release of `version`: an alpha's moves `feed-alpha`
/// alone, a plain version's both.
fn feeds_of(version: &str) -> Vec<&'static str> {
    if version.contains('-') {
        vec![ALPHA_FEED]
    } else {
        vec![ALPHA_FEED, STABLE_FEED]
    }
}

/// The names of the files in `folder`.
fn names_in(folder: &Path) -> Result<Vec<String>, Error> {
    let read = |source: io::Error| Error::File {
        action: "read",
        path: folder.to_path_buf(),
        source,
    };
    let mut names = Vec::new();
    for entry in fs::read_dir(folder).map_err(read)? {
        names.push(
            entry
                .map_err(read)?
                .file_name()
                .to_string_lossy()
                .into_owned(),
        );
    }
    names.sort();
    Ok(names)
}

/// The hashes a SHA256SUMS lists, by file name (lines of `<64 hex>`, a space, a
/// space or `*`, and the name, as `sha256sum` writes them), and the lines that are
/// no such thing.
pub(super) fn listed_in(sums: &str) -> (BTreeMap<String, String>, Vec<String>) {
    let mut listed = BTreeMap::new();
    let mut odd = Vec::new();
    for line in sums.lines().filter(|line| !line.trim().is_empty()) {
        let parsed = line
            .split_once(' ')
            .filter(|(hash, _)| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(|(hash, rest)| (hash, rest.strip_prefix([' ', '*']).unwrap_or(rest)))
            .filter(|(_, name)| !name.is_empty());
        match parsed {
            Some((hash, name)) => {
                listed.insert(name.to_owned(), hash.to_ascii_lowercase());
            }
            None => odd.push(line.to_owned()),
        }
    }
    (listed, odd)
}

/// The SHA-256 of the file at `path`, as `sha256sum` writes it.
fn sha256_of(path: &Path) -> Result<String, Error> {
    let fail = |source: io::Error| Error::File {
        action: "read",
        path: path.to_path_buf(),
        source,
    };
    let mut file = fs::File::open(path).map_err(fail)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(fail)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}
