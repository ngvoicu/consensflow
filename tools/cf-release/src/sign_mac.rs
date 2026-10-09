//! `cf-release sign-mac`: the Mac release under the Developer ID, so that a
//! download opens with no Gatekeeper warning, online or not. Every Mach-O in the
//! app is signed from the inside out with hardened runtime and a secure
//! timestamp, the app is notarized and its ticket stapled, then the DMG Tauri
//! built is made again around it, signed, notarized and stapled too.
//!
//! Run after `npm --prefix app run build` on a Mac:
//! `cf-release sign-mac [--bundle <target/release/bundle>] [--adhoc]`.
//!
//! The identity comes from the environment, as the release keeps it:
//! `APPLE_CERTIFICATE` (the .p12, in base64) and `APPLE_CERTIFICATE_PASSWORD`, and
//! the notary's key `APPLE_API_KEY` (the .p8), `APPLE_API_KEY_ID` and
//! `APPLE_API_ISSUER`. Only Apple's tools read it, from a keychain made for this
//! run and deleted after it. `--adhoc` signs the same files with no identity and
//! leaves the notary out: what the tests run.
//!
//! No secret is in a word this says: the failure a run ends in and every line it
//! speaks have the identity's values, and the keychain's password, blanked out
//! of them (see `secrets`). Every program runs through `tools`, whose runner a
//! test replaces with a script.
//!
//! - `macho`: which files are code, and where;
//! - `signing`: `codesign`, and the order an app is signed in;
//! - `keychain`: the identity in a keychain of the run's own;
//! - `notary`: a file notarized and its ticket stapled;
//! - `dmg`: the DMG made again around the signed app;
//! - `verify`: what a download meets.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::file;

use crate::cli::{Command, Console, Failure};
use secrets::{Credentials, Secrets};
use signing::Signing;
use tools::{fail, Runner, System, Tools};

mod dmg;
mod keychain;
mod macho;
mod notary;
mod secrets;
mod signing;
mod tools;
mod verify;

#[cfg(test)]
mod tests;

/// `cf-release sign-mac`.
pub const COMMAND: Command = Command {
    name: "sign-mac",
    about: "Sign, notarize and staple the Mac release under the Developer ID",
    usage: "[--bundle DIR] [--adhoc]",
    run,
};

/// Where Tauri leaves the bundles of a release build, from the root of the checkout.
const DEFAULT_BUNDLE: &str = "app/src-tauri/target/release/bundle";

/// What the words after `sign-mac` ask for.
#[derive(Debug)]
struct Options {
    bundle: PathBuf,
    adhoc: bool,
}

impl Options {
    fn parse(args: &[OsString]) -> Result<Self, Failure> {
        let mut options = Self {
            bundle: PathBuf::from(DEFAULT_BUNDLE),
            adhoc: false,
        };
        let mut words = args.iter();
        while let Some(word) = words.next() {
            if word == "--adhoc" {
                options.adhoc = true;
            } else if word == "--bundle" {
                let value = words
                    .next()
                    .filter(|value| !value.to_string_lossy().starts_with("--"));
                let Some(value) = value else {
                    return Err(Failure::Usage("--bundle needs a value".into()));
                };
                options.bundle = PathBuf::from(value);
            } else {
                return Err(Failure::Usage(format!(
                    "unknown argument: {}",
                    word.to_string_lossy()
                )));
            }
        }
        Ok(options)
    }
}

/// What a run is asked to do.
struct Request {
    /// The folder Tauri's bundles are in: `macos/` holds the app and `dmg/` the DMG.
    bundle: PathBuf,
    /// The identity to sign under, or none to sign ad hoc.
    credentials: Option<Credentials>,
    /// The folder the run's own is made in.
    tmp: PathBuf,
}

fn run(env: &Env, args: &[OsString], console: &mut Console) -> Result<(), Failure> {
    let options = Options::parse(args)?;
    let credentials = if options.adhoc {
        None
    } else {
        Some(Credentials::from_env(env)?)
    };
    if !cfg!(target_os = "macos") {
        return Err(fail("it signs with Apple's tools, which are on a Mac"));
    }
    let request = Request {
        bundle: options.bundle,
        credentials,
        tmp: env
            .path("TMPDIR")
            .map_or_else(|| PathBuf::from("/tmp"), Path::to_path_buf),
    };
    sign(&System { env }, &request, console)
}

/// The identity a run signs under, and the password of the keychain it is put in.
struct Login<'a> {
    credentials: &'a Credentials,
    keychain_password: String,
}

/// Signs the release `request` names, with `runner` for the machine.
///
/// Nothing leaves here that has a secret in it: the failure is blanked as the
/// lines spoken on the way are. The one failure that is not is the draw of the
/// keychain's password, which comes before there is a password to blank and says
/// the system's words alone.
fn sign(runner: &dyn Runner, request: &Request, console: &mut Console) -> Result<(), Failure> {
    let login = request
        .credentials
        .as_ref()
        .map(|credentials| {
            keychain_password(runner).map(|keychain_password| Login {
                credentials,
                keychain_password,
            })
        })
        .transpose()?;
    let secrets = login.as_ref().map_or_else(Secrets::default, |login| {
        Secrets::of(login.credentials, &login.keychain_password)
    });
    let mut tools = Tools::new(runner, &secrets, console);
    in_scratch(&mut tools, runner, request, login.as_ref()).map_err(|failure| match failure {
        Failure::Failed(text) => Failure::Failed(secrets.redact(&text)),
        other => other,
    })
}

/// Runs the release in a folder made for this run alone. The certificate and the
/// notary's key are written there, and go with it, whatever the run comes to.
fn in_scratch(
    tools: &mut Tools,
    runner: &dyn Runner,
    request: &Request,
    login: Option<&Login>,
) -> Result<(), Failure> {
    let scratch = scratch_folder(runner, &request.tmp)?;
    let outcome = release_in(tools, &request.bundle, login, &scratch);
    if let Err(cause) = file::remove_all(&scratch) {
        tools.say(&format!("could not remove its own folder: {cause}"));
    }
    outcome
}

/// The release in `bundle`, signed ad hoc or under `login`.
fn release_in(
    tools: &mut Tools,
    bundle: &Path,
    login: Option<&Login>,
    scratch: &Path,
) -> Result<(), Failure> {
    let app = only(&bundle.join("macos"), ".app")?;
    let dmg = only(&bundle.join("dmg"), ".dmg")?;
    let Some(Login {
        credentials,
        keychain_password,
    }) = login
    else {
        let release = Release {
            app,
            dmg,
            signing: Signing::AdHoc,
            notary: None,
            scratch,
        };
        return release.run(tools);
    };
    let notary = notary::Notary::new(credentials, scratch)?;
    keychain::with_identity(
        tools,
        credentials,
        keychain_password,
        scratch,
        |tools, signing| {
            let release = Release {
                app,
                dmg,
                signing,
                notary: Some(notary),
                scratch,
            };
            release.run(tools)
        },
    )
}

/// The release in hand: what is signed, by whom, and whether the notary checks it.
struct Release<'a> {
    app: PathBuf,
    dmg: PathBuf,
    signing: Signing,
    notary: Option<notary::Notary>,
    scratch: &'a Path,
}

impl Release<'_> {
    fn run(&self, tools: &mut Tools) -> Result<(), Failure> {
        let (app, dmg) = (&self.app, &self.dmg);
        tools.say(&format!("signing {}", name(app)));
        signing::sign_app(tools, &self.signing, app)?;
        if let Some(notary) = &self.notary {
            notary::notarize(tools, app, notary, self.scratch)?;
        }
        tools.say(&format!("making {} again around it", name(dmg)));
        dmg::rebuild(tools, dmg, app, self.scratch)?;
        signing::codesign(tools, &self.signing, dmg, &[])?;
        if let Some(notary) = &self.notary {
            notary::notarize(tools, dmg, notary, self.scratch)?;
        }
        verify::verify(tools, app, dmg, self.notary.is_some())?;
        tools.say(&format!("{} and {} signed", name(app), name(dmg)));
        Ok(())
    }
}

/// The one entry of `dir` named with `extension`.
fn only(dir: &Path, extension: &str) -> Result<PathBuf, Failure> {
    let entries = fs::read_dir(dir)
        .and_then(|entries| entries.collect::<io::Result<Vec<_>>>())
        .map_err(|cause| fail(format!("could not read {}: {cause}", dir.display())))?;
    let found: Vec<_> = entries
        .iter()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(extension))
        .collect();
    match found[..] {
        [entry] => Ok(entry.path()),
        _ => Err(fail(format!(
            "{} holds {} {extension} files, not one",
            dir.display(),
            found.len()
        ))),
    }
}

/// What `path` is called, for a line that names it.
fn name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// The password of the keychain a run makes: 24 bytes nobody can guess, in hex.
fn keychain_password(runner: &dyn Runner) -> Result<String, Failure> {
    let mut bytes = [0; 24];
    runner
        .random(&mut bytes)
        .map_err(|cause| fail(format!("could not draw a password: {cause}")))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// A folder under `tmp` for this run alone, private to this user.
fn scratch_folder(runner: &dyn Runner, tmp: &Path) -> Result<PathBuf, Failure> {
    let prefix = tmp.join("sign-mac-");
    let prefix = prefix
        .to_str()
        .ok_or_else(|| fail(format!("{} is not text", tmp.display())))?;
    file::make_temporary_folder(prefix, || {
        let mut name = [0; 6];
        runner.random(&mut name)?;
        Ok(name)
    })
    .map_err(|cause| fail(cause.to_string()))
}
