//! What the release keeps secret, how it reaches the disk and how it is kept
//! out of every word the run says.
//!
//! The release gives the identity in five variables: the certificate (the
//! `.p12`, in base64) and its password, and the notary's key (the `.p8`), its id
//! and its issuer. Apple's tools read them from files and arguments of their own
//! (see `keychain` and `notary`); [`Secrets`] holds the same values, and the
//! keychain's password, so that no failure and no line the run speaks can carry
//! one.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use cf_base::env::Env;

use super::tools::fail;
use crate::cli::Failure;

const CERTIFICATE: &str = "APPLE_CERTIFICATE";
const CERTIFICATE_PASSWORD: &str = "APPLE_CERTIFICATE_PASSWORD";
const API_KEY: &str = "APPLE_API_KEY";
const API_KEY_ID: &str = "APPLE_API_KEY_ID";
const API_ISSUER: &str = "APPLE_API_ISSUER";

/// The variables, in the order a missing one is told.
const NAMES: [&str; 5] = [
    CERTIFICATE,
    CERTIFICATE_PASSWORD,
    API_KEY,
    API_KEY_ID,
    API_ISSUER,
];

/// What stands where a secret was.
const BLANK: &str = "[redacted]";

/// The identity of the release. It has no `Debug`: a secret is never formatted
/// by accident.
pub(super) struct Credentials {
    certificate: String,
    certificate_password: String,
    api_key: String,
    api_key_id: String,
    api_issuer: String,
}

impl Credentials {
    /// The identity the environment holds, or the variables it lacks. One set to
    /// nothing is lacking, as it was for the script.
    pub fn from_env(env: &Env) -> Result<Self, Failure> {
        let missing: Vec<_> = NAMES
            .into_iter()
            .filter(|name| env.text(name).is_none())
            .collect();
        if !missing.is_empty() {
            return Err(fail(format!(
                "{} not set; --adhoc signs with no identity",
                missing.join(", ")
            )));
        }
        let value = |name: &str| env.text(name).unwrap_or_default().to_string();
        Ok(Self {
            certificate: value(CERTIFICATE),
            certificate_password: value(CERTIFICATE_PASSWORD),
            api_key: value(API_KEY),
            api_key_id: value(API_KEY_ID),
            api_issuer: value(API_ISSUER),
        })
    }

    /// The certificate's password, for `security import`.
    pub fn certificate_password(&self) -> &str {
        &self.certificate_password
    }

    /// The id of the notary's key.
    pub fn key_id(&self) -> &str {
        &self.api_key_id
    }

    /// The issuer of the notary's key.
    pub fn issuer(&self) -> &str {
        &self.api_issuer
    }

    /// Writes the certificate, decoded, to `path`, readable by this user alone.
    pub fn write_certificate(&self, path: &Path) -> Result<(), Failure> {
        let bytes = decode_base64(&self.certificate)
            .ok_or_else(|| fail(format!("{CERTIFICATE} is not base64")))?;
        write_private(path, &bytes)
    }

    /// Writes the notary's key to `path`, readable by this user alone.
    pub fn write_key(&self, path: &Path) -> Result<(), Failure> {
        write_private(path, self.api_key.as_bytes())
    }
}

/// Writes `bytes` to a new file at `path`, readable by this user alone.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|cause| fail(format!("could not write {}: {cause}", path.display())))
}

/// Base64 as Node reads it: either alphabet, white space anywhere and the
/// padding left off or not. Anything else is not base64, and so is a last
/// character that is left alone with no byte to make.
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    let (mut bits, mut held, mut padded) = (0u32, 0u32, false);
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => {
                padded = true;
                continue;
            }
            _ if byte.is_ascii_whitespace() => continue,
            _ => return None,
        };
        if padded {
            return None;
        }
        bits = (bits << 6) | u32::from(value);
        held += 6;
        if held >= 8 {
            held -= 8;
            bytes.push(u8::try_from((bits >> held) & 0xff).ok()?);
            bits &= (1 << held) - 1;
        }
    }
    (held < 6).then_some(bytes)
}

/// The secrets of a run, and the means to blank them out of a text.
#[derive(Default)]
pub(super) struct Secrets {
    /// Every text to blank, the longest first: the values whole and trimmed, and
    /// each line of one that has several (the notary's key is a file of lines, and
    /// a tool may say one of them).
    words: Vec<String>,
}

impl Secrets {
    /// The secrets of a run that signs under `credentials`, with the password of
    /// the keychain it makes.
    pub fn of(credentials: &Credentials, keychain_password: &str) -> Self {
        let Credentials {
            certificate,
            certificate_password,
            api_key,
            api_key_id,
            api_issuer,
        } = credentials;
        Self::new([
            certificate.as_str(),
            certificate_password,
            api_key,
            api_key_id,
            api_issuer,
            keychain_password,
        ])
    }

    fn new<'a>(values: impl IntoIterator<Item = &'a str>) -> Self {
        let mut words: Vec<String> = values
            .into_iter()
            .flat_map(|value| {
                [value, value.trim()]
                    .into_iter()
                    .chain(value.lines().map(str::trim))
            })
            .filter(|word| !word.trim().is_empty())
            .map(str::to_string)
            .collect();
        words.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        words.dedup();
        Self { words }
    }

    /// `text` with every secret in it blanked out. Overlapping secrets are
    /// blanked as one, so that the part of a secret that a longer one covers is
    /// not left to show.
    pub fn redact(&self, text: &str) -> String {
        let mut found: Vec<(usize, usize)> = self
            .words
            .iter()
            .flat_map(|word| {
                text.match_indices(word.as_str())
                    .map(|(start, found)| (start, start + found.len()))
            })
            .collect();
        found.sort_unstable();
        let mut blanked = String::with_capacity(text.len());
        let mut done = 0;
        let mut spans = found.into_iter().peekable();
        while let Some((start, mut end)) = spans.next() {
            while let Some((_, further)) = spans.next_if(|(next, _)| *next <= end) {
                end = end.max(further);
            }
            blanked.push_str(&text[done..start]);
            blanked.push_str(BLANK);
            done = end;
        }
        blanked.push_str(&text[done..]);
        blanked
    }
}

#[cfg(test)]
mod tests;
