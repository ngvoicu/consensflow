//! The release's SHA256SUMS, and the signature the installers check it by.
//!
//! The one-line installers (install/) download a release's DMG or
//! installer with curl or PowerShell, which no Gatekeeper or SmartScreen looks
//! at, and hold it to the SHA-256 its SHA256SUMS lists, and that SHA256SUMS to a
//! minisign signature by the update key (the key app/src-tauri/tauri.conf.json
//! holds). The signature is made in a job of its own, the only one besides the
//! Mac job's signing step that sees the key: it asks this module for
//! SHA256SUMS ([`write_sums`], `cf-publish sums`), and signs a copy of it named
//! for the release, [`signed_name`], with `tauri signer sign`, whose trusted
//! comment names the file it signed: so a signature binds the release it is
//! for, and is not an update archive's (those name ConsensFlow.app.tar.gz).
//! That signature is published as [`SIGNATURE`], beside SHA256SUMS.
//!
//! The publisher computes SHA256SUMS again from the files it publishes and
//! publishes only the one that was signed, byte for byte ([`signed_sums`]).

use std::fs;
use std::path::Path;

use crate::assets::{name_of, on_disk, ReleaseAssets};
use crate::digest::sha256_hex;
use crate::failure::Failure;
use crate::version::Version;

/// The release's list of hashes.
pub const SUMS: &str = "SHA256SUMS";

/// The signature of [`SUMS`], as Tauri's signer leaves it.
pub const SIGNATURE: &str = "SHA256SUMS.sig";

/// The name SHA256SUMS is signed under for `version`, which the signature's
/// trusted comment says and the installers hold it to.
pub fn signed_name(version: &Version) -> String {
    format!("ConsensFlow_{version}_SHA256SUMS")
}

/// What SHA256SUMS says of the release `version` built in `dir`: one line for
/// each of its files, in the order they are uploaded in, `<sha256>  <name>` as
/// sha256sum writes them. A file missing or empty fails it.
pub fn sums_of(dir: &Path, version: &Version) -> Result<String, Failure> {
    let assets = ReleaseAssets::of(version);
    let mut sums = String::new();
    for path in assets.all() {
        let bytes =
            fs::read(on_disk(dir, path)).map_err(|_| Failure::new(format!("missing {path}")))?;
        if bytes.is_empty() {
            return Err(Failure::new(format!("missing {path}")));
        }
        sums.push_str(&format!("{}  {}\n", sha256_hex(&bytes), name_of(path)));
    }
    Ok(sums)
}

/// Writes SHA256SUMS for the release built in `dir`, and the copy of it the
/// signer signs, [`signed_name`]; resolves to that name.
pub fn write_sums(dir: &Path, version: &Version) -> Result<String, Failure> {
    let sums = sums_of(dir, version)?;
    let signed = signed_name(version);
    for name in [SUMS, signed.as_str()] {
        fs::write(dir.join(name), &sums)
            .map_err(|cause| Failure::new(format!("could not write {name}: {cause}")))?;
    }
    Ok(signed)
}

/// The SHA256SUMS to publish for the release built in `dir`: the one that was
/// signed, beside its [`SIGNATURE`], which must be what the files built say.
/// Nothing unsigned is published.
pub fn signed_sums(dir: &Path, version: &Version) -> Result<String, Failure> {
    let computed = sums_of(dir, version)?;
    let signature = fs::read(dir.join(SIGNATURE)).unwrap_or_default();
    if signature.is_empty() {
        return Err(Failure::new(format!(
            "missing {SIGNATURE}: the installers take no release whose SHA256SUMS is not signed"
        )));
    }
    let signed = fs::read(dir.join(SUMS))
        .map_err(|_| Failure::new(format!("missing {SUMS}, the one {SIGNATURE} signs")))?;
    if signed != computed.as_bytes() {
        return Err(Failure::new(format!(
            "the {SUMS} that was signed does not list the files built here"
        )));
    }
    Ok(computed)
}
