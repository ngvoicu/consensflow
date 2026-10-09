//! `codesign`: how a file is signed, and the order an app is signed in.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::macho;
use super::tools::{args, Tools};
use crate::cli::Failure;

/// Who signs.
pub(super) enum Signing {
    /// No identity, as the tests sign: the signature holds the seal and nothing
    /// vouches for who made it.
    AdHoc,
    /// The Developer ID Application identity (the SHA-1 of its certificate) that
    /// the keychain at `keychain` holds.
    DeveloperId { identity: String, keychain: PathBuf },
}

/// Signs `path`, replacing a signature it has, with `options` after the
/// identity's own.
pub(super) fn codesign(
    tools: &Tools,
    signing: &Signing,
    path: &Path,
    options: &[&str],
) -> Result<(), Failure> {
    let mut words = args!["--force"];
    match signing {
        Signing::AdHoc => words.extend(args!["--timestamp=none", "--sign", "-"]),
        Signing::DeveloperId { identity, keychain } => {
            words.extend(args![
                "--timestamp",
                "--sign",
                identity,
                "--keychain",
                keychain
            ]);
        }
    }
    words.extend(options.iter().map(|option| OsString::from(*option)));
    words.push(path.into());
    tools.run("codesign", words).map(drop)
}

/// What a key of the app's `Info.plist` says.
fn plist(tools: &Tools, app: &Path, key: &str) -> Result<String, Failure> {
    let file = app.join("Contents").join("Info.plist");
    let raw = tools.run("plutil", args!["-extract", key, "raw", "-o", "-", file])?;
    Ok(raw.trim().to_string())
}

/// Signs `app` from the inside out: the Mach-Os it carries, then the bundle,
/// whose seal covers them. All run under the hardened runtime and none carries
/// an entitlement: nothing in the app makes executable memory of its own (the
/// WebView's engine runs in the system's process), which the bundled Node's V8
/// did and was given the JIT entitlements for. The main executable is left to
/// the bundle's own signing, which signs it.
pub(super) fn sign_app(tools: &Tools, signing: &Signing, app: &Path) -> Result<(), Failure> {
    let id = plist(tools, app, "CFBundleIdentifier")?;
    let executable = plist(tools, app, "CFBundleExecutable")?;
    let main = app.join("Contents").join("MacOS").join(executable);
    for path in macho::mach_os(app)?
        .into_iter()
        .filter(|path| *path != main)
    {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let identifier = format!("{id}.{name}");
        codesign(
            tools,
            signing,
            &path,
            &["--options", "runtime", "--identifier", &identifier],
        )?;
    }
    codesign(tools, signing, app, &["--options", "runtime"])
}
