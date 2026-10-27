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
//! publishes only the one that was signed, byte for byte, whose signature holds
//! under the update key for that release, as the installers will check it
//! ([`signed_sums`]): a signature the installers would refuse never reaches a
//! published release.

use std::fs;
use std::path::Path;

use minisign_verify::{PublicKey, Signature};

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

/// The update key the installers pin and the app's updater holds: the minisign
/// key line inside app/src-tauri/tauri.conf.json's own base64, as this binary
/// was built with it.
pub fn update_key() -> Result<String, Failure> {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../../../app/src-tauri/tauri.conf.json"))
            .map_err(|cause| Failure::new(format!("tauri.conf.json is not JSON: {cause}")))?;
    let pubkey = config["plugins"]["updater"]["pubkey"]
        .as_str()
        .ok_or_else(|| Failure::new("tauri.conf.json names no updater key"))?;
    let text = base64(pubkey)
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or_else(|| Failure::new("the updater key in tauri.conf.json is not base64 text"))?;
    text.lines()
        .nth(1)
        .map(str::to_string)
        .ok_or_else(|| Failure::new("the updater key in tauri.conf.json has no key line"))
}

/// The SHA256SUMS to publish for the release built in `dir`: the one that was
/// signed, beside its [`SIGNATURE`], which must be what the files built say,
/// signed by `key` (a minisign key line) for this release
/// ([`check_signature`]). Nothing the installers would refuse is published.
pub fn signed_sums(dir: &Path, version: &Version, key: &str) -> Result<String, Failure> {
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
    check_signature(key, &signature, &signed, &signed_name(version))
        .map_err(|why| Failure::new(format!("{SIGNATURE} does not hold: {why}")))?;
    Ok(computed)
}

/// Whether `sig` (a .sig file as Tauri's signer leaves it: the minisign
/// signature's four lines in base64) signs `bytes` under `key` with a trusted
/// comment `timestamp:<n>\tfile:<name>`, in the very shape the installers take:
/// minisign's prehashed kind, each line ending in a line feed and nothing after.
/// The signatures are checked by `minisign-verify`, the crate the app's updater
/// checks its updates with.
pub fn check_signature(key: &str, sig: &[u8], bytes: &[u8], name: &str) -> Result<(), String> {
    let not_minisign = || "the signature file is not a minisign signature".to_string();
    let text = std::str::from_utf8(sig)
        .ok()
        .and_then(|text| base64(text.trim()))
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or_else(not_minisign)?;
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.len() != 5
        || !lines[4].is_empty()
        || lines.iter().any(|line| line.contains('\r'))
        || !lines[0].starts_with("untrusted comment: ")
    {
        return Err(not_minisign());
    }
    let signature = Signature::decode(&text).map_err(|_| not_minisign())?;
    let key =
        PublicKey::from_base64(key).map_err(|_| "the key is not a minisign key".to_string())?;
    key.verify(bytes, &signature, false)
        .map_err(|cause| format!("it does not sign SHA256SUMS under the update key ({cause})"))?;
    let comment = signature.trusted_comment();
    let named = comment
        .strip_prefix("timestamp:")
        .and_then(|rest| rest.split_once("\tfile:"))
        .filter(|(stamp, _)| !stamp.is_empty() && stamp.bytes().all(|b| b.is_ascii_digit()))
        .map(|(_, file)| file);
    match named {
        Some(file) if file == name => Ok(()),
        _ => Err(format!("the signature is for {comment:?}, not {name}")),
    }
}

/// `text` as strict base64: padded, of its 64 letters alone, and the encoding
/// of what it decodes to (no stray bits), or None.
fn base64(text: &str) -> Option<Vec<u8>> {
    const LETTERS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let pad = bytes.iter().rev().take_while(|b| **b == b'=').count();
    if pad > 2 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let mut n: u32 = 0;
        for byte in chunk {
            let value = match byte {
                b'=' => 0,
                _ => LETTERS.iter().position(|letter| letter == byte)? as u32,
            };
            n = (n << 6) | value;
        }
        out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
    }
    out.truncate(out.len() - pad);
    // The padding only at the end, and the encoding of what it decodes to.
    if bytes[..bytes.len() - pad].contains(&b'=') || encode(&out) != text {
        return None;
    }
    Some(out)
}

fn encode(bytes: &[u8]) -> String {
    const LETTERS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (at, byte)| n | u32::from(*byte) << (16 - 8 * at));
        for at in 0..4 {
            if at <= chunk.len() {
                out.push(LETTERS[(n >> (18 - 6 * at) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
