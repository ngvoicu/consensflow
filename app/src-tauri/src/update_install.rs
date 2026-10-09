//! Tauri verifies the downloaded archive's signature. On macOS a same-volume
//! atomic exchange additionally keeps the installed bundle intact on failure.
use std::ffi::CString;
use std::fs;
use std::io::Cursor;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};
use std::process::Command;

fn plist_value(app: &Path, field: &str) -> Result<String, String> {
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", field, "raw", "-o", "-"])
        .arg(app.join("Contents/Info.plist"))
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("The app bundle has no valid {field}"));
    }
    String::from_utf8(output.stdout)
        .map(|s| s.trim().to_owned())
        .map_err(|e| e.to_string())
}

/// What a bundle must be to replace the installed app: this app, at the
/// version the feed offered; the `cf` a window runs; and a code signature that
/// verifies. (The archive's own signature was verified before its bytes came
/// here.) Nothing of Node's is asked for, and none is shipped: the releases
/// before this one did, and an app of any of them takes a bundle that ships
/// none by this same check. `cli/package.json` is not read either: the plist
/// names the version.
fn validate_bundle(app: &Path, version: &str) -> Result<(), String> {
    if plist_value(app, "CFBundleIdentifier")? != "dev.ngvoicu.consensflow"
        || plist_value(app, "CFBundleShortVersionString")? != version
        || plist_value(app, "CFBundleVersion")? != version
    {
        return Err(
            "The archive's app identity or version does not match the offered release".into(),
        );
    }
    if !app.join("Contents/Resources/cli/bin/cf").is_file() {
        return Err("The archive must include cf, the command a window runs".into());
    }
    let signature = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app)
        .output()
        .map_err(|e| e.to_string())?;
    if !signature.status.success() {
        return Err("The extracted app failed macOS code-signature verification".into());
    }
    Ok(())
}

fn stage_update(directory: &Path) -> Result<tempfile::TempDir, String> {
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    tempfile::Builder::new()
        .prefix("update-")
        .tempdir_in(directory)
        .map_err(|e| format!("Could not stage the update in ConsensFlow home: {e}"))
}

pub(crate) fn install_archive(
    app: &Path,
    bytes: &[u8],
    version: &str,
    directory: &Path,
) -> Result<(), String> {
    if app.extension().and_then(|s| s.to_str()) != Some("app")
        || fs::symlink_metadata(app)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        || plist_value(app, "CFBundleIdentifier")? != "dev.ngvoicu.consensflow"
    {
        return Err(
            "Install a packaged ConsensFlow app in a writable folder before updating".into(),
        );
    }
    let installed = semver::Version::parse(&plist_value(app, "CFBundleShortVersionString")?)
        .map_err(|e| e.to_string())?;
    if semver::Version::parse(version).map_err(|e| e.to_string())? <= installed {
        return Err("The update must be newer than the installed app".into());
    }
    let staging = stage_update(directory)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(Cursor::new(bytes)));
    let mut expanded = 0_u64;
    for (index, entry) in archive.entries().map_err(|e| e.to_string())?.enumerate() {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?;
        let mut components = path.components();
        if components.next() != Some(Component::Normal(std::ffi::OsStr::new("ConsensFlow.app")))
            || components.any(|p| !matches!(p, Component::Normal(_)))
            || !(entry.header().entry_type().is_file() || entry.header().entry_type().is_dir())
        {
            return Err("The update archive contains an unsafe path, link or special file".into());
        }
        expanded = expanded
            .checked_add(entry.size())
            .ok_or("Update archive size overflow")?;
        if index >= 20_000 || expanded > 1024 * 1024 * 1024 {
            return Err("The update archive exceeds the bundle size limit".into());
        }
        if !entry.unpack_in(staging.path()).map_err(|e| e.to_string())? {
            return Err("The update archive contains a path outside the app".into());
        }
    }
    let candidate = staging.path().join("ConsensFlow.app");
    validate_bundle(&candidate, version)?;
    let old = CString::new(app.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let new = CString::new(candidate.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    // RENAME_SWAP is atomic on the same filesystem; cross-volume installs
    // fail without changing either path or staging outside ConsensFlow home.
    if unsafe {
        libc::renameatx_np(
            libc::AT_FDCWD,
            old.as_ptr(),
            libc::AT_FDCWD,
            new.as_ptr(),
            libc::RENAME_SWAP,
        )
    } != 0
    {
        return Err(format!(
            "Could not replace the app; the installed copy is unchanged: {}",
            std::io::Error::last_os_error()
        ));
    }
    // Report a filesystem cleanup failure explicitly. The exchange has already
    // succeeded, so it must never be reported as a failed installation.
    let staging_path = staging.path().to_owned();
    if let Err(error) = staging.close() {
        eprintln!(
            "ConsensFlow update installed, but cleanup of {} failed: {error}",
            staging_path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    fn install_fixture(app: &Path, bytes: &[u8], version: &str) -> Result<(), String> {
        let home = tempfile::tempdir().unwrap();
        install_archive(app, bytes, version, &home.path().join("updates"))
    }

    /// What a bundle of the releases before this one ships besides the app and
    /// a window's `cf`: Node, the CLI's sources and their `package.json`,
    /// relative to the bundle. A bundle of this release has none of them.
    const OLDER_LAYOUT: [&str; 5] = [
        "Contents/MacOS/node",
        "Contents/Resources/cli/package.json",
        "Contents/Resources/cli/bin/cf.mjs",
        "Contents/Resources/cli/hosts/adapter.js",
        "Contents/Resources/cli/src/ui.js",
    ];

    fn sign(app: &Path) {
        assert!(Command::new("/usr/bin/codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(app)
            .output()
            .unwrap()
            .status
            .success());
    }

    fn bundle(parent: &Path, version: &str) -> std::path::PathBuf {
        bundle_of(parent, version, "dev.ngvoicu.consensflow", false)
    }

    /// A bundle laid out as the releases before this one were: with Node, the
    /// CLI's sources and their `package.json` beside the app and its `cf`.
    fn older_bundle(parent: &Path, version: &str) -> std::path::PathBuf {
        bundle_of(parent, version, "dev.ngvoicu.consensflow", true)
    }

    fn bundle_of(
        parent: &Path,
        version: &str,
        identity: &str,
        older_layout: bool,
    ) -> std::path::PathBuf {
        let app = parent.join("ConsensFlow.app");
        for dir in ["Contents/MacOS", "Contents/Resources/cli/bin"] {
            fs::create_dir_all(app.join(dir)).unwrap();
        }
        fs::copy("/bin/echo", app.join("Contents/MacOS/app")).unwrap();
        fs::write(app.join("Contents/Info.plist"), format!(r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>{identity}</string><key>CFBundleExecutable</key><string>app</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>{version}</string><key>CFBundleShortVersionString</key><string>{version}</string></dict></plist>"#)).unwrap();
        // A window's cf: native code under Resources, signed ad hoc before the
        // bundle around it is, as `cargo xtask build-cf` signs it.
        let cf = app.join("Contents/Resources/cli/bin/cf");
        fs::copy("/bin/echo", &cf).unwrap();
        assert!(Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(&cf)
            .status()
            .unwrap()
            .success());
        if older_layout {
            fs::copy("/bin/echo", app.join(OLDER_LAYOUT[0])).unwrap();
            for dir in ["Contents/Resources/cli/hosts", "Contents/Resources/cli/src"] {
                fs::create_dir_all(app.join(dir)).unwrap();
            }
            fs::write(
                app.join(OLDER_LAYOUT[1]),
                format!(r#"{{"name":"consensflow","version":"{version}"}}"#),
            )
            .unwrap();
            for file in &OLDER_LAYOUT[2..] {
                fs::write(app.join(file), "test file of the older layout").unwrap();
            }
        }
        sign(&app);
        app
    }

    fn archive(app: &Path) -> Vec<u8> {
        let target = app.parent().unwrap().join("update.tar.gz");
        assert!(Command::new("/usr/bin/tar")
            .args(["-czf"])
            .arg(&target)
            .arg("-C")
            .arg(app.parent().unwrap())
            .arg(app.file_name().unwrap())
            .env("COPYFILE_DISABLE", "1")
            .status()
            .unwrap()
            .success());
        fs::read(target).unwrap()
    }

    #[test]
    fn update_staging_stays_inside_the_private_directory() {
        let home = tempfile::tempdir().unwrap();
        let directory = home.path().join(".consensflow/app/updates");
        let staging = stage_update(&directory).unwrap();
        assert_eq!(staging.path().parent(), Some(directory.as_path()));
        assert!(staging.path().is_dir());
        staging.close().unwrap();
        assert_eq!(fs::read_dir(directory).unwrap().count(), 0);
    }

    /// The bundle replaces the whole app, whichever layout the installed one
    /// had: of this release's, or of the releases before it, which shipped
    /// Node, the CLI's sources and their `package.json` and are replaced by
    /// a bundle that ships none of them, leaving nothing of them behind.
    #[test]
    fn a_signed_bundle_replaces_the_whole_app_and_leaves_nothing_of_an_older_layout() {
        for installed in ["this layout", "an older layout"] {
            let old = tempfile::tempdir().unwrap();
            let new = tempfile::tempdir().unwrap();
            let app = if installed == "this layout" {
                bundle(old.path(), "3.0.0-alpha.35")
            } else {
                older_bundle(old.path(), "3.0.0-alpha.35")
            };
            let next = bundle(new.path(), "3.0.0-alpha.36");
            install_fixture(&app, &archive(&next), "3.0.0-alpha.36")
                .unwrap_or_else(|error| panic!("{installed}: {error}"));
            assert_eq!(
                plist_value(&app, "CFBundleShortVersionString").unwrap(),
                "3.0.0-alpha.36",
                "{installed}"
            );
            for file in OLDER_LAYOUT {
                assert!(!app.join(file).exists(), "{file} is left: {installed}");
            }
            assert!(
                Command::new("/usr/bin/codesign")
                    .args(["--verify", "--deep", "--strict"])
                    .arg(&app)
                    .status()
                    .unwrap()
                    .success(),
                "{installed}"
            );
            assert_eq!(
                fs::read_dir(old.path()).unwrap().count(),
                1,
                "transactional staging is removed: {installed}"
            );
        }
    }

    #[test]
    fn invalid_or_wrong_version_archives_leave_original_app_intact() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let app = bundle(old.path(), "3.0.0-alpha.35");
        let before = fs::read(app.join("Contents/Info.plist")).unwrap();
        let next = bundle(new.path(), "3.0.0-alpha.34");
        for bytes in [b"not a tar".to_vec(), archive(&next)] {
            assert!(install_fixture(&app, &bytes, "3.0.0-alpha.36").is_err());
            assert_eq!(fs::read(app.join("Contents/Info.plist")).unwrap(), before);
            assert_eq!(fs::read_dir(old.path()).unwrap().count(), 1);
        }
        fs::write(
            next.join("Contents/Resources/cli/bin/cf"),
            "changed after signing",
        )
        .unwrap();
        assert!(install_fixture(&app, &archive(&next), "3.0.0-alpha.34").is_err());
        assert_eq!(fs::read(app.join("Contents/Info.plist")).unwrap(), before);
    }

    /// A bundle signed as it should be, but another app's, is refused for its
    /// identity before anything of it is looked at.
    #[test]
    fn a_signed_bundle_of_another_app_is_refused_for_its_identity() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let app = bundle(old.path(), "3.0.0-alpha.35");
        let next = bundle_of(new.path(), "3.0.0-alpha.36", "dev.example.other", false);
        let error = install_fixture(&app, &archive(&next), "3.0.0-alpha.36").unwrap_err();
        assert!(error.contains("identity or version"), "{error}");
        assert_eq!(
            plist_value(&app, "CFBundleShortVersionString").unwrap(),
            "3.0.0-alpha.35"
        );
    }

    #[test]
    fn archive_links_are_refused_before_any_swap() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let app = bundle(old.path(), "3.0.0-alpha.35");
        let next = bundle(new.path(), "3.0.0-alpha.36");
        std::os::unix::fs::symlink("/tmp", next.join("Contents/escape")).unwrap();
        assert!(install_fixture(&app, &archive(&next), "3.0.0-alpha.36").is_err());
        assert!(!app.join("Contents/escape").exists());
        assert_eq!(fs::read_dir(old.path()).unwrap().count(), 1);
    }

    /// What a window runs is `cf`: a valid signature does not make up for a
    /// bundle that has none.
    #[test]
    fn a_valid_signature_does_not_replace_a_bundle_without_cf() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let app = bundle(old.path(), "3.0.0-alpha.35");
        let next = bundle(new.path(), "3.0.0-alpha.36");
        fs::remove_file(next.join("Contents/Resources/cli/bin/cf")).unwrap();
        sign(&next);
        let error = install_fixture(&app, &archive(&next), "3.0.0-alpha.36").unwrap_err();
        assert!(error.contains("must include cf"), "{error}");
        assert_eq!(
            plist_value(&app, "CFBundleShortVersionString").unwrap(),
            "3.0.0-alpha.35"
        );
    }

    /// The bundle's seal covers every file of it: `cf` changed after the
    /// signing refuses the bundle.
    #[test]
    fn a_newer_bundle_modified_after_signing_leaves_the_old_app_intact() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let app = bundle(old.path(), "3.0.0-alpha.35");
        let next = bundle(new.path(), "3.0.0-alpha.36");
        fs::write(next.join("Contents/Resources/cli/bin/cf"), "tampered").unwrap();
        let error = install_fixture(&app, &archive(&next), "3.0.0-alpha.36").unwrap_err();
        assert!(error.contains("code-signature"), "{error}");
        assert_eq!(
            plist_value(&app, "CFBundleShortVersionString").unwrap(),
            "3.0.0-alpha.35"
        );
    }
}
