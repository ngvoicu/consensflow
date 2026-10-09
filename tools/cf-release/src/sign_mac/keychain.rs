//! The release's identity in a keychain of its own: made for the run, searched
//! while it signs and deleted after it, with the user's search list as it was.

use std::ffi::OsString;
use std::path::Path;

use super::secrets::Credentials;
use super::signing::Signing;
use super::tools::{args, fail, Tools};
use crate::cli::Failure;

/// How long the keychain stays unlocked, in seconds: the two notarizations may
/// take an hour each.
const UNLOCKED_FOR: &str = "21600";

/// Runs `work` with the release's identity in a keychain made in `scratch` for
/// this run and deleted after it, whatever came of `work`, with the keychain
/// search list as it was.
///
/// The keychain is put first in the user's search list, since `codesign` builds
/// the certificate chain from the keychains it searches. Once the keychain is
/// made, the search list is set to what it was and the keychain is deleted
/// whatever else fails; if one of those two fails it is told, and the failure
/// that ended the run is still the one raised.
pub(super) fn with_identity<T>(
    tools: &mut Tools,
    credentials: &Credentials,
    password: &str,
    scratch: &Path,
    work: impl FnOnce(&mut Tools, Signing) -> Result<T, Failure>,
) -> Result<T, Failure> {
    let keychain = scratch.join("signing.keychain-db");
    let listed = tools.run("security", args!["list-keychains", "-d", "user"])?;
    let searched = search_list(&listed);
    tools.run(
        "security",
        args!["create-keychain", "-p", password, &keychain],
    )?;
    let result = sign_in(
        tools,
        credentials,
        password,
        scratch,
        &keychain,
        &searched,
        work,
    );
    let mut restored = args!["list-keychains", "-d", "user", "-s"];
    restored.extend(searched.iter().map(OsString::from));
    tools.attempt("security", restored);
    tools.attempt("security", args!["delete-keychain", &keychain]);
    result
}

/// Puts the certificate in the made `keychain` and runs `work` with it.
fn sign_in<T>(
    tools: &mut Tools,
    credentials: &Credentials,
    password: &str,
    scratch: &Path,
    keychain: &Path,
    searched: &[String],
    work: impl FnOnce(&mut Tools, Signing) -> Result<T, Failure>,
) -> Result<T, Failure> {
    let security = |words: Vec<OsString>| tools.run("security", words);
    security(args![
        "set-keychain-settings",
        "-lut",
        UNLOCKED_FOR,
        keychain
    ])?;
    security(args!["unlock-keychain", "-p", password, keychain])?;
    let certificate = scratch.join("certificate.p12");
    credentials.write_certificate(&certificate)?;
    security(args![
        "import",
        &certificate,
        "-k",
        keychain,
        "-f",
        "pkcs12",
        "-P",
        credentials.certificate_password(),
        "-T",
        "/usr/bin/codesign",
    ])?;
    // Without it codesign asks for the keychain's password, in a dialog no one sees.
    security(args![
        "set-key-partition-list",
        "-S",
        "apple-tool:,apple:,codesign:",
        "-s",
        "-k",
        password,
        keychain,
    ])?;
    let mut searching = args!["list-keychains", "-d", "user", "-s", keychain];
    searching.extend(searched.iter().map(OsString::from));
    security(searching)?;
    let found = security(args!["find-identity", "-v", "-p", "codesigning", keychain])?;
    let identity = developer_id(&found)?;
    work(
        tools,
        Signing::DeveloperId {
            identity,
            keychain: keychain.to_path_buf(),
        },
    )
}

/// The keychains `security list-keychains` lists, one to a line, each in quotes.
fn search_list(listed: &str) -> Vec<String> {
    listed
        .lines()
        .map(|line| {
            let line = line.trim();
            line.strip_prefix('"')
                .and_then(|inner| inner.strip_suffix('"'))
                .unwrap_or(line)
        })
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// The SHA-1 of the one Developer ID Application identity that `security
/// find-identity` lists.
fn developer_id(listed: &str) -> Result<String, Failure> {
    let found: Vec<_> = listed.lines().filter_map(developer_id_of).collect();
    match found[..] {
        [only] => Ok(only.to_string()),
        _ => Err(fail(format!(
            "the certificate holds {} Developer ID Application identities",
            found.len()
        ))),
    }
}

/// The SHA-1 a line of `security find-identity -v` gives a Developer ID
/// Application identity, as `  1) 0123…ABCD "Developer ID Application: Name (TEAM)"`.
fn developer_id_of(line: &str) -> Option<&str> {
    let (number, rest) = line.trim_start().split_once(") ")?;
    let (hash, name) = rest.split_once(' ')?;
    let numbered = !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit());
    let sha1 = hash.len() == 40
        && hash
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'A'..=b'F'));
    (numbered && sha1 && name.starts_with("\"Developer ID Application: ")).then_some(hash)
}

#[cfg(test)]
mod tests;
