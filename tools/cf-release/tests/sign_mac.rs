//! `cf-release sign-mac` as a process: the refusal of a release without its
//! identity on every platform, and, on a Mac, the ad hoc signing of a small
//! bundle with Apple's own tools. The order of the calls and the failures are
//! tried against a script, in the crate (`src/sign_mac/tests`); here `codesign`
//! and `hdiutil` are real, so what the script's tests assume of them is held to.
#![allow(clippy::unwrap_used)]

use std::ffi::{OsStr, OsString};

use cf_base::env::Env;
use cf_release::process::{self, Output};

const BINARY: &str = env!("CARGO_BIN_EXE_cf-release");

/// Runs `cf-release` with `args` and exactly `env`.
fn cf_release(env: &Env, args: &[&OsStr]) -> Output {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    process::capture(OsStr::new(BINARY), &args, env).unwrap()
}

/// This process's environment with `changes` made to it and every `APPLE_`
/// variable it had taken away first, so that a machine with an identity of its own
/// does not give the run one.
fn env_without_an_identity(changes: &[(&str, &OsStr)]) -> Env {
    let kept: Vec<(OsString, OsString)> = Env::from_process()
        .iter()
        .filter(|(name, _)| !name.to_string_lossy().starts_with("APPLE_"))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    let changes = changes
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)));
    Env::from_vars(kept.into_iter().chain(changes))
}

#[test]
fn it_refuses_to_sign_the_release_without_its_identity_naming_what_is_missing() {
    let env = env_without_an_identity(&[("APPLE_API_KEY_ID", OsStr::new("KEY"))]);
    let folder = tempfile::tempdir().unwrap();
    let refused = cf_release(
        &env,
        &[
            OsStr::new("sign-mac"),
            OsStr::new("--bundle"),
            folder.path().as_os_str(),
        ],
    );
    assert_eq!(refused.code, 1);
    assert!(
        refused.stderr.contains(
            "APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD, APPLE_API_KEY, APPLE_API_ISSUER not set"
        ),
        "{refused:?}"
    );
    assert_eq!(refused.stdout, "");
}

#[cfg(target_os = "macos")]
mod apple_tools {
    //! The JavaScript test's case, ported: the bundle as Tauri leaves one, signed
    //! with no identity, then looked at with the tools that sign.

    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;

    const ID: &str = "dev.ngvoicu.consensflow";
    const DMG: &str = "ConsensFlow_3.0.0-alpha.99_aarch64.dmg";
    /// Where the release's app keeps the Mach-Os it carries, by name.
    const CODE: [(&str, &str); 2] = [("app", "MacOS/app"), ("cf", "Resources/cli/bin/cf")];

    /// Runs a program of the system, and answers what it left.
    fn system(program: &str, args: &[&OsStr]) -> Output {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        process::capture(OsStr::new(program), &args, &Env::from_process()).unwrap()
    }

    fn words(program: &str, args: &[&str]) -> Output {
        let args: Vec<_> = args.iter().map(OsStr::new).collect();
        system(program, &args)
    }

    fn succeeds(program: &str, args: &[&OsStr]) {
        let done = system(program, args);
        assert_eq!(done.code, 0, "{program} {args:?}: {done:?}");
    }

    /// A bundle folder as Tauri leaves one: the app, with the release's two
    /// Mach-Os, and a DMG of it. Returns the app.
    fn write_bundle(dir: &Path) -> PathBuf {
        let app = dir.join("macos").join("ConsensFlow.app");
        let contents = app.join("Contents");
        for folder in ["MacOS", "Resources/cli/bin"] {
            fs::create_dir_all(contents.join(folder)).unwrap();
        }
        fs::write(
            contents.join("Info.plist"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
                 <key>CFBundleIdentifier</key><string>{ID}</string>\
                 <key>CFBundleExecutable</key><string>app</string>\
                 <key>CFBundlePackageType</key><string>APPL</string></dict></plist>\n"
            ),
        )
        .unwrap();
        for (_, path) in CODE {
            fs::copy("/usr/bin/true", contents.join(path)).unwrap();
        }
        let volume = dir.join("volume-source");
        fs::create_dir(&volume).unwrap();
        succeeds(
            "ditto",
            &[app.as_os_str(), volume.join("ConsensFlow.app").as_os_str()],
        );
        std::os::unix::fs::symlink("/Applications", volume.join("Applications")).unwrap();
        fs::write(volume.join(".VolumeIcon.icns"), "icon\n").unwrap();
        fs::create_dir(dir.join("dmg")).unwrap();
        let dmg = dir.join("dmg").join(DMG);
        succeeds(
            "hdiutil",
            &[
                OsStr::new("create"),
                OsStr::new("-quiet"),
                OsStr::new("-volname"),
                OsStr::new("ConsensFlow"),
                OsStr::new("-srcfolder"),
                volume.as_os_str(),
                OsStr::new("-fs"),
                OsStr::new("HFS+"),
                OsStr::new("-format"),
                OsStr::new("UDZO"),
                OsStr::new("-size"),
                OsStr::new("20m"),
                dmg.as_os_str(),
            ],
        );
        app
    }

    /// What `codesign --display` says of `path`: it says it on its error stream.
    fn shown(path: &Path) -> String {
        words("codesign", &["-dvvv", &path.to_string_lossy()]).stderr
    }

    fn cdhash(shown: &str) -> &str {
        shown
            .lines()
            .find_map(|line| line.strip_prefix("CDHash="))
            .unwrap()
    }

    /// A DMG attached, and detached again whatever happens.
    struct Attached<'a>(&'a Path);

    impl Drop for Attached<'_> {
        fn drop(&mut self) {
            let _ = words("hdiutil", &["detach", &self.0.to_string_lossy(), "-force"]);
        }
    }

    #[test]
    fn it_signs_every_mach_o_of_the_app_from_the_inside_out_then_makes_the_dmg_again_around_it() {
        let dir = tempfile::tempdir().unwrap();
        let scratch_in = tempfile::tempdir().unwrap();
        let app = write_bundle(dir.path());

        // The run makes its own folder under TMPDIR: it must be gone once the run is.
        let env = env_without_an_identity(&[("TMPDIR", scratch_in.path().as_os_str())]);
        let signed = cf_release(
            &env,
            &[
                OsStr::new("sign-mac"),
                OsStr::new("--bundle"),
                dir.path().as_os_str(),
                OsStr::new("--adhoc"),
            ],
        );
        assert_eq!(signed.code, 0, "{signed:?}");
        assert_eq!(fs::read_dir(scratch_in.path()).unwrap().count(), 0);

        for (name, path) in CODE {
            let file = app.join("Contents").join(path);
            let said = shown(&file);
            assert!(
                said.contains("flags=0x10002(adhoc,runtime)"),
                "{name} runs hardened: {said}"
            );
            // The main executable is the bundle's own; the other has an identifier of its own.
            let identifier = if name == "app" {
                ID.to_string()
            } else {
                format!("{ID}.{name}")
            };
            assert!(
                said.lines()
                    .any(|line| line == format!("Identifier={identifier}")),
                "{name}: {said}"
            );
            // None needs an entitlement: the JIT ones were the bundled Node's V8's.
            let entitled = words(
                "codesign",
                &[
                    "-d",
                    "--entitlements",
                    "-",
                    "--xml",
                    &file.to_string_lossy(),
                ],
            );
            assert_eq!(entitled.stdout, "", "{name}");
        }
        succeeds(
            "codesign",
            &[
                OsStr::new("--verify"),
                OsStr::new("--deep"),
                OsStr::new("--strict"),
                app.as_os_str(),
            ],
        );

        let dmg = dir.path().join("dmg").join(DMG);
        assert!(shown(&dmg).lines().any(|line| line == "Format=disk image"));
        let volume = dir.path().join("volume");
        fs::create_dir(&volume).unwrap();
        succeeds(
            "hdiutil",
            &[
                OsStr::new("attach"),
                dmg.as_os_str(),
                OsStr::new("-readonly"),
                OsStr::new("-noverify"),
                OsStr::new("-noautoopen"),
                OsStr::new("-nobrowse"),
                OsStr::new("-mountpoint"),
                volume.as_os_str(),
            ],
        );
        let _attached = Attached(&volume);
        let mut held: Vec<_> = fs::read_dir(&volume)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        held.sort();
        assert_eq!(
            held,
            [".VolumeIcon.icns", "Applications", "ConsensFlow.app"]
        );
        // The app in the DMG is the signed one, to the byte of its seal.
        let (inside, outside) = (shown(&volume.join("ConsensFlow.app")), shown(&app));
        assert_eq!(cdhash(&inside), cdhash(&outside));
    }
}
