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
pub(super) fn plist(tools: &Tools, app: &Path, key: &str) -> Result<String, Failure> {
    let file = app.join("Contents").join("Info.plist");
    let raw = tools.run("plutil", args!["-extract", key, "raw", "-o", "-", file])?;
    Ok(raw.trim().to_string())
}

/// What the bundle is signed with (`app/src-tauri/Entitlements.plist`, which a
/// local build is signed with too): the microphone and Apple Events the app's own
/// process may ask macOS for under the hardened runtime, as a terminal app is
/// entitled to. A program in its panes carries its own signature; what it needs of
/// ConsensFlow is the `Info.plist`'s word for each ([`USAGE`]), which macOS asks the
/// person with (T-540).
pub(super) const ENTITLEMENTS: &str = include_str!("../../../../app/src-tauri/Entitlements.plist");

/// The entitlements the signed bundle holds, those and no others ([`ENTITLEMENTS`]),
/// and the `Info.plist` keys that say why to the person macOS asks.
pub(super) const ENTITLED: [&str; 2] = [
    "com.apple.security.device.audio-input",
    "com.apple.security.automation.apple-events",
];
pub(super) const USAGE: [&str; 2] = [
    "NSMicrophoneUsageDescription",
    "NSAppleEventsUsageDescription",
];

/// Signs `app` from the inside out: the Mach-Os it carries, then the bundle,
/// whose seal covers them. All run under the hardened runtime. Its code carries no
/// entitlement: nothing in the app makes executable memory of its own (the
/// WebView's engine runs in the system's process), which the bundled Node's V8
/// did and was given the JIT entitlements for. The bundle carries `entitlements`
/// ([`ENTITLEMENTS`], written there by the release), which macOS reads for the
/// app's own process. The main executable is left to the
/// bundle's own signing, which signs it.
pub(super) fn sign_app(
    tools: &Tools,
    signing: &Signing,
    app: &Path,
    entitlements: &Path,
) -> Result<(), Failure> {
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
    let entitlements = entitlements.to_string_lossy();
    codesign(
        tools,
        signing,
        app,
        &["--options", "runtime", "--entitlements", &entitlements],
    )
}
