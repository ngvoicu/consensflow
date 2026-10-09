//! The tests of `sign-mac` against a script in place of Apple's tools: the order
//! the calls are made in, what is cleaned up when one fails, and what is kept
//! secret. Nothing here runs `codesign` or `hdiutil`, so they run on every
//! platform; the real tools are tried by `tests/sign_mac.rs`.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::*;
use fake::Fake;

mod cleanup;
mod command;
mod detaching;
mod fake;
mod flow;
mod machine;
mod notarizing;
mod redaction;
mod telling;

const ID: &str = "dev.ngvoicu.consensflow";
const DMG: &str = "ConsensFlow_3.0.0-alpha.99_aarch64.dmg";

/// The release's secrets as a test has them, each one told apart by its text.
pub const CERTIFICATE: &str = "cGtjczEyLWJ5dGVzLW9mLXRoZS1jZXJ0aWZpY2F0ZQ==";
pub const CERTIFICATE_PASSWORD: &str = "p12-passw0rd-S3CRET";
pub const API_KEY_LINES: [&str; 4] = [
    "-----BEGIN PRIVATE KEY-----",
    "MIGTAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBHkwdwIBAQQgZm9yLXRlc3Rz",
    "LW9ubHktbm90LWEta2V5LWF0LWFsbA",
    "-----END PRIVATE KEY-----",
];
pub const API_KEY_ID: &str = "KEYID12345";
pub const API_ISSUER: &str = "69a6de70-1111-47e3-e053-5b8c7c11a4d1";
/// What the script's source of bytes gives a 24-byte password.
pub const KEYCHAIN_PASSWORD: &str = "000102030405060708090a0b0c0d0e0f1011121314151617";

/// Every text a run must keep out of what it says.
pub fn secrets() -> Vec<String> {
    let mut all = vec![
        CERTIFICATE.to_string(),
        CERTIFICATE_PASSWORD.to_string(),
        API_KEY_ID.to_string(),
        API_ISSUER.to_string(),
        KEYCHAIN_PASSWORD.to_string(),
    ];
    // The key's own lines: the header and footer say nothing, the rest is it.
    all.extend(API_KEY_LINES[1..3].iter().map(ToString::to_string));
    all
}

/// The identity of the release as the environment holds it.
pub fn credentials() -> Credentials {
    let key = format!("{}\n", API_KEY_LINES.join("\n"));
    let env = Env::from_vars([
        ("APPLE_CERTIFICATE", CERTIFICATE),
        ("APPLE_CERTIFICATE_PASSWORD", CERTIFICATE_PASSWORD),
        ("APPLE_API_KEY", key.as_str()),
        ("APPLE_API_KEY_ID", API_KEY_ID),
        ("APPLE_API_ISSUER", API_ISSUER),
    ]);
    Credentials::from_env(&env).unwrap()
}

/// What a Mach-O of one architecture begins with: the 64-bit magic, then filler.
const MACH_O: [u8; 8] = [0xcf, 0xfa, 0xed, 0xfe, 0x07, 0x00, 0x00, 0x01];

/// A bundle folder as Tauri leaves one: the app, with the code it carries at
/// different depths and files that are not code among it, and a DMG of it.
pub struct Bundle {
    dir: TempDir,
}

impl Bundle {
    pub fn new() -> Self {
        let bundle = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        let contents = bundle.app().join("Contents");
        let info = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
             <key>CFBundleIdentifier</key><string>{ID}</string>\
             <key>CFBundleExecutable</key><string>app</string></dict></plist>\n"
        );
        bundle.write(&contents.join("Info.plist"), info.as_bytes());
        // The main executable, left to the bundle's own signing.
        bundle.write(&contents.join("MacOS").join("app"), &MACH_O);
        bundle.write(&contents.join("MacOS").join("helper"), &MACH_O);
        bundle.write(&contents.join("Frameworks").join("libz.dylib"), &MACH_O);
        let resources = contents.join("Resources");
        bundle.write(&resources.join("cli").join("bin").join("cf"), &MACH_O);
        bundle.write(&resources.join("cli").join("bin").join("cf.json"), b"{}");
        // A universal binary: two slices.
        bundle.write(
            &resources.join("fat"),
            &[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 2],
        );
        // A Java class file shares the universal magic, with its version where the slices are counted.
        bundle.write(
            &resources.join("Foo.class"),
            &[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 52],
        );
        bundle.write(&resources.join("notes.txt"), b"notes\n");
        bundle.write(&bundle.dmg(), b"a disk image");
        bundle
    }

    fn write(&self, path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::File::create(path).unwrap().write_all(bytes).unwrap();
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn app(&self) -> PathBuf {
        self.path().join("macos").join("ConsensFlow.app")
    }

    pub fn dmg(&self) -> PathBuf {
        self.path().join("dmg").join(DMG)
    }
}

/// A run of `sign` on a fresh bundle, with a script for Apple's tools, and
/// everything it did.
pub struct Trial {
    pub bundle: Bundle,
    /// The folder the run makes its own in: empty when it is done with it.
    pub tmp: TempDir,
    pub fake: Fake,
    pub out: String,
    pub err: String,
}

impl Trial {
    pub fn new(fake: Fake) -> Self {
        Self {
            bundle: Bundle::new(),
            tmp: tempfile::tempdir().unwrap(),
            fake,
            out: String::new(),
            err: String::new(),
        }
    }

    /// Signs the bundle under `credentials`, or ad hoc.
    pub fn sign(&mut self, credentials: Option<Credentials>) -> Result<(), Failure> {
        let request = Request {
            bundle: self.bundle.path().to_path_buf(),
            credentials,
            tmp: self.tmp.path().to_path_buf(),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let result = sign(
            &self.fake,
            &request,
            &mut Console {
                out: &mut out,
                err: &mut err,
            },
        );
        self.out = String::from_utf8(out).unwrap();
        self.err = String::from_utf8(err).unwrap();
        result
    }

    /// Every call the run made, as a line each, with the run's own places named
    /// and `/` between the parts of a path.
    pub fn transcript(&self) -> Vec<String> {
        let places = [
            (self.scratch(), "$SCRATCH"),
            (self.bundle.app(), "$APP"),
            (self.bundle.dmg(), "$DMG"),
        ];
        self.fake
            .calls()
            .iter()
            .map(|call| {
                let mut line = call.clone();
                for (place, name) in &places {
                    line = line.replace(&place.display().to_string(), name);
                }
                line.replace(KEYCHAIN_PASSWORD, "$PASSWORD")
                    .replace('\\', "/")
            })
            .collect()
    }

    /// The folder the run made for itself, which the script's source of bytes names.
    pub fn scratch(&self) -> PathBuf {
        self.tmp.path().join("sign-mac-abcdef")
    }

    /// What the run left in the folder it makes its own in.
    pub fn left_behind(&self) -> Vec<String> {
        fs::read_dir(self.tmp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }
}
