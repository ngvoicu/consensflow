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

fn validate_bundle(app: &Path, version: &str) -> Result<(), String> {
    if plist_value(app, "CFBundleIdentifier")? != "dev.ngvoicu.consensflow"
        || plist_value(app, "CFBundleShortVersionString")? != version
        || plist_value(app, "CFBundleVersion")? != version
    {
        return Err(
            "The archive's app identity or version does not match the offered release".into(),
        );
    }
    let cli = app.join("Contents/Resources/cli");
    let package: serde_json::Value =
        serde_json::from_slice(&fs::read(cli.join("package.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if !app.join("Contents/MacOS/node").is_file()
        || package["name"] != "consensflow"
        || package["version"] != version
        || !cli.join("bin/cf.mjs").is_file()
        || !cli.join("hosts").is_dir()
        || !cli.join("src").is_dir()
    {
        return Err("The archive must include cf and its adapters at the same app version".into());
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

    fn bundle(parent: &Path, version: &str) -> std::path::PathBuf {
        let app = parent.join("ConsensFlow.app");
        for dir in [
            "Contents/MacOS",
            "Contents/Resources/cli/bin",
            "Contents/Resources/cli/hosts",
            "Contents/Resources/cli/src",
        ] {
            fs::create_dir_all(app.join(dir)).unwrap();
        }
        fs::copy("/bin/echo", app.join("Contents/MacOS/app")).unwrap();
        fs::copy("/bin/echo", app.join("Contents/MacOS/node")).unwrap();
        fs::write(app.join("Contents/Info.plist"), format!(r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>dev.ngvoicu.consensflow</string><key>CFBundleExecutable</key><string>app</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>{version}</string><key>CFBundleShortVersionString</key><string>{version}</string></dict></plist>"#)).unwrap();
        fs::write(
            app.join("Contents/Resources/cli/package.json"),
            format!(r#"{{"name":"consensflow","version":"{version}"}}"#),
        )
        .unwrap();
        fs::write(app.join("Contents/Resources/cli/bin/cf.mjs"), "test CLI").unwrap();
        fs::write(
            app.join("Contents/Resources/cli/hosts/adapter.js"),
            "test adapter",
        )
        .unwrap();
        fs::write(app.join("Contents/Resources/cli/src/ui.js"), "test runtime").unwrap();
        assert!(Command::new("/usr/bin/codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(&app)
            .output()
            .unwrap()
            .status
            .success());
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

    #[test]
    fn signed_bundle_replaces_whole_app_and_leaves_no_retained_copy() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let app = bundle(old.path(), "3.0.0-alpha.35");
        let next = bundle(new.path(), "3.0.0-alpha.36");
        install_fixture(&app, &archive(&next), "3.0.0-alpha.36").unwrap();
        assert!(
            fs::read_to_string(app.join("Contents/Resources/cli/package.json"))
                .unwrap()
                .contains("alpha.36")
        );
        assert!(Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&app)
            .status()
            .unwrap()
            .success());
        assert_eq!(
            fs::read_dir(old.path()).unwrap().count(),
            1,
            "transactional staging is removed"
        );
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
            next.join("Contents/Resources/cli/bin/cf.mjs"),
            "changed after signing",
        )
        .unwrap();
        assert!(install_fixture(&app, &archive(&next), "3.0.0-alpha.34").is_err());
        assert_eq!(fs::read(app.join("Contents/Info.plist")).unwrap(), before);
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

    #[test]
    fn a_valid_signature_does_not_replace_missing_bundled_runtime() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let app = bundle(old.path(), "3.0.0-alpha.35");
        let next = bundle(new.path(), "3.0.0-alpha.36");
        let node = next.join("Contents/MacOS/node");
        fs::remove_file(node).unwrap();
        assert!(Command::new("/usr/bin/codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(&next)
            .output()
            .unwrap()
            .status
            .success());
        assert!(install_fixture(&app, &archive(&next), "3.0.0-alpha.36").is_err());
        assert_eq!(
            plist_value(&app, "CFBundleShortVersionString").unwrap(),
            "3.0.0-alpha.35"
        );
    }

    #[test]
    fn a_newer_bundle_modified_after_signing_leaves_the_old_app_intact() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let app = bundle(old.path(), "3.0.0-alpha.35");
        let next = bundle(new.path(), "3.0.0-alpha.36");
        fs::write(next.join("Contents/Resources/cli/bin/cf.mjs"), "tampered").unwrap();
        let error = install_fixture(&app, &archive(&next), "3.0.0-alpha.36").unwrap_err();
        assert!(error.contains("code-signature"), "{error}");
        assert_eq!(
            plist_value(&app, "CFBundleShortVersionString").unwrap(),
            "3.0.0-alpha.35"
        );
    }
}
