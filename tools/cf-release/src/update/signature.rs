//! The signature's shape. `tauri signer sign` leaves a `.sig` file: a minisign
//! signature, four lines of text, written as base64 once more. This holds a
//! `.sig` to that shape before the feed carries it, so that a file that is no
//! signature at all (an empty one, an error message the signer wrote, a
//! signature cut short) is never published. It does not verify the signature:
//! installed apps do, against the key they carry.

use std::fs;
use std::path::Path;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cf_base::js;

/// A signature the feed will not carry.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignatureError {
    #[error("could not read signature: {0}")]
    Unreadable(String),
    /// Not one line of canonical base64, of at least [`SHORTEST`] bytes of text.
    #[error("signature is not outer-base64 minisign text")]
    NotBase64,
    #[error("signature is not a Tauri minisign envelope")]
    NotEnvelope,
}

/// The fewest bytes the four lines of a minisign signature come to.
const SHORTEST: usize = 80;

/// The first line: what the signer calls the key's comment.
const UNTRUSTED_COMMENT: &str = "untrusted comment: signature from tauri secret key";

/// The signature in the file at `path`, as the feed carries it: the file's
/// text without the blank around it.
pub fn read(path: &Path) -> Result<String, SignatureError> {
    let bytes = fs::read(path)
        .map_err(|cause| SignatureError::Unreadable(format!("{}: {cause}", path.display())))?;
    let text = String::from_utf8_lossy(&bytes);
    let signature = js::trim(&text);
    check(signature)?;
    Ok(signature.to_string())
}

/// Holds `signature` to the envelope Tauri's signer writes: one line of
/// canonical base64, of four lines each of the shape it has.
fn check(signature: &str) -> Result<(), SignatureError> {
    // The engine reads the standard alphabet, a line of it and nothing else, in
    // exactly the canonical form: its padding, and no stray bits.
    let bytes = STANDARD
        .decode(signature)
        .ok()
        .filter(|bytes| bytes.len() >= SHORTEST)
        .ok_or(SignatureError::NotBase64)?;
    let text = String::from_utf8(bytes).map_err(|_| SignatureError::NotBase64)?;
    let lines: Vec<&str> = text
        .strip_suffix('\n')
        .unwrap_or(&text)
        .split('\n')
        .collect();
    let &[untrusted, key, trusted, global] = lines.as_slice() else {
        return Err(SignatureError::NotEnvelope);
    };
    let envelope = untrusted == UNTRUSTED_COMMENT
        && is_base64_line(key)
        && is_trusted_comment(trusted)
        && is_base64_line(global);
    if envelope {
        Ok(())
    } else {
        Err(SignatureError::NotEnvelope)
    }
}

/// One or more characters of the base64 alphabet, then up to two `=`.
fn is_base64_line(line: &str) -> bool {
    let body = line
        .strip_suffix("==")
        .or_else(|| line.strip_suffix('='))
        .unwrap_or(line);
    !body.is_empty()
        && body
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
}

/// `trusted comment: timestamp:<seconds>`, white space, then `file:<name>` of
/// letters, digits, `.`, `_` and `-` (the signer puts a tab between the two).
fn is_trusted_comment(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("trusted comment: timestamp:") else {
        return false;
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let (_, rest) = rest.split_at(digits);
    let name = rest.trim_start_matches(js::is_space);
    let spaced = name.len() < rest.len();
    let Some(name) = name.strip_prefix("file:") else {
        return false;
    };
    digits > 0
        && spaced
        && !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests;
