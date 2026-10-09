//! The bundle and the sources: what the command refuses of an app that is not
//! what the release says it is, and of a version that is not the sources'.

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

use super::fixture::{Release, Spec, CF, EXECUTABLE, INFO_PLIST, STABLE};

/// A file of the bundle in the folder, by its path inside the archive.
fn at(release: &Release, name: &str) -> PathBuf {
    let inside = name.strip_prefix("ConsensFlow.app/").unwrap();
    inside
        .split('/')
        .fold(release.bundle.clone(), |path, part| path.join(part))
}

/// Gives the bundle's `cf` this script, and the archive's too: the bundle still
/// is the archive, so what is refused is the script and not the difference.
fn set_cf(release: &mut Release, script: &str) {
    fs::write(at(release, CF), script).unwrap();
    release.archived.item(CF).data = script.as_bytes().to_vec();
    release.archive_again();
}

/// An `Info.plist` of an app that names `executable` and `version`.
fn plist(executable: &str, version: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
         <key>CFBundleExecutable</key>{executable}<key>CFBundleShortVersionString</key>{version}\
         </dict></plist>\n"
    )
}

/// Gives the bundle's `Info.plist` this text, and the archive's too.
fn set_plist(release: &mut Release, text: &str) {
    fs::write(at(release, INFO_PLIST), text).unwrap();
    release.archived.item(INFO_PLIST).data = text.as_bytes().to_vec();
    release.archive_again();
}

const NAMED: &str = "<string>ConsensFlow</string>";
const SAYS: &str = "<string>3.0.0-alpha.99</string>";

#[test]
fn rejects_a_bundle_whose_plist_and_bundled_cf_disagree_on_the_version() {
    let release = Release::of(&Spec {
        cf_version: Some("3.0.0-alpha.98"),
        ..Spec::default()
    });
    release
        .run(&[])
        .refused("bundle and its cf versions differ: 3.0.0-alpha.99 != 3.0.0-alpha.98");
}

#[test]
fn rejects_a_bundle_whose_cf_does_not_say_its_version() {
    let mut release = Release::new();
    set_cf(&mut release, "#!/bin/sh\nexit 3\n");
    release
        .run(&[])
        .refused("the bundled cf did not say its version (cf --version): it ended with code 3\n");

    set_cf(
        &mut release,
        "#!/bin/sh\necho 'no such verb: --version' >&2\nexit 2\n",
    );
    release.run(&[]).refused(
        "the bundled cf did not say its version (cf --version): it ended with code 2: no such verb: --version\n",
    );
}

#[test]
fn rejects_a_bundle_whose_cf_cannot_be_run() {
    let release = Release::new();
    let cf = at(&release, CF);
    fs::set_permissions(&cf, fs::Permissions::from_mode(0o644)).unwrap();
    release.run(&[]).refused(&format!(
        "the bundled cf did not say its version (cf --version): could not start {}: ",
        cf.display()
    ));
}

#[test]
fn rejects_a_cf_whose_version_is_not_one_and_takes_the_blank_around_one() {
    let mut release = Release::new();
    set_cf(
        &mut release,
        "#!/bin/sh\necho 'ConsensFlow cf 3.0.0-alpha.99'\n",
    );
    release.run(&[]).refused(
        "bundled cf's version is not a canonical semantic version: ConsensFlow cf 3.0.0-alpha.99\n",
    );

    set_cf(&mut release, "#!/bin/sh\necho 3.0.0-alpha.99+build.1\n");
    release.run(&[]).refused(
        "bundled cf's version is not a canonical semantic version: 3.0.0-alpha.99+build.1\n",
    );

    set_cf(
        &mut release,
        "#!/bin/sh\nprintf '\\n  3.0.0-alpha.99 \\t\\n\\n'\n",
    );
    release.run(&[]).finished();
}

#[test]
fn rejects_a_bundle_without_the_cf_a_window_runs() {
    let release = Release::new();
    let cf = at(&release, CF);
    fs::remove_file(&cf).unwrap();
    release.run(&[]).refused(&format!(
        "bundle is missing a window's cf: {}\n",
        cf.display()
    ));

    // A folder where it should be is no cf either.
    fs::create_dir(&cf).unwrap();
    release.run(&[]).refused(&format!(
        "bundle is missing a window's cf: {}\n",
        cf.display()
    ));
}

#[test]
fn rejects_a_bundle_without_its_native_executable_before_it_asks_about_the_cf() {
    let release = Release::new();
    let binary = at(&release, EXECUTABLE);
    fs::remove_file(&binary).unwrap();
    fs::remove_file(at(&release, CF)).unwrap();
    release.run(&[]).refused(&format!(
        "bundle is missing native executable: {}\n",
        binary.display()
    ));
}

#[test]
fn rejects_an_executable_name_that_leaves_the_folder_it_is_in() {
    for name in [
        "../ConsensFlow",
        "a/b",
        "Contents/MacOS/ConsensFlow",
        "a\\b",
        "/bin/sh",
    ] {
        let mut release = Release::new();
        set_plist(
            &mut release,
            &plist(&format!("<string>{name}</string>"), SAYS),
        );
        release
            .run(&[])
            .refused("bundle executable name is unsafe\n");
    }
}

#[test]
fn rejects_an_info_plist_that_does_not_name_what_the_release_needs() {
    let executable = "bundle has no readable CFBundleExecutable in Info.plist\n";
    let version = "bundle has no readable CFBundleShortVersionString in Info.plist\n";
    for (text, said) in [
        (String::new(), executable),
        ("not a plist at all".to_string(), executable),
        ("<plist version=\"1.0\"><array/></plist>".to_string(), executable),
        (
            "<plist version=\"1.0\"><dict><key>CFBundleShortVersionString</key><string>3.0.0</string></dict></plist>"
                .to_string(),
            executable,
        ),
        (
            "<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><integer>1</integer></dict></plist>"
                .to_string(),
            executable,
        ),
        (
            "<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>ConsensFlow</string></dict></plist>"
                .to_string(),
            version,
        ),
        (plist(NAMED, "<integer>3</integer>"), version),
        (plist(NAMED, "<array><string>3.0.0</string></array>"), version),
    ] {
        let mut release = Release::new();
        set_plist(&mut release, &text);
        release.run(&[]).refused(said);
    }
    // No file at all.
    let release = Release::new();
    fs::remove_file(at(&release, INFO_PLIST)).unwrap();
    release.run(&[]).refused(executable);
}

#[test]
fn reads_the_info_plist_a_binary_one_as_well() {
    let mut release = Release::new();
    let value = plist::Value::from_reader(std::io::Cursor::new(plist(NAMED, SAYS))).unwrap();
    let mut binary = Vec::new();
    value.to_writer_binary(&mut binary).unwrap();
    assert!(binary.starts_with(b"bplist00"));
    fs::write(at(&release, INFO_PLIST), &binary).unwrap();
    release.archived.item(INFO_PLIST).data = binary;
    release.archive_again();
    release.run(&[]).finished();
}

#[test]
fn rejects_what_is_not_an_app_folder() {
    let release = Release::new();
    let refused = |bundle: &PathBuf| {
        release
            .run(&[("bundle", bundle.to_str().unwrap())])
            .refused(&format!(
                "bundle is not a .app directory: {}\n",
                bundle.display()
            ));
    };
    // A folder not named for an app, a file that is, and nothing at all.
    refused(&release.repo);
    let file = release.dir.path().join("x.app");
    fs::write(&file, "").unwrap();
    refused(&file);
    refused(&release.dir.path().join("Gone.app"));
    // A trailing slash is only a way to write the folder.
    let slashed = format!("{}/", release.bundle.display());
    release.run(&[("bundle", &slashed)]).finished();
}

#[test]
fn rejects_a_version_that_is_not_a_canonical_semantic_one_in_the_bundle() {
    for (version, said) in [
        (
            "3.0.0-alpha.99+build.1",
            "bundle version is not a canonical semantic version: 3.0.0-alpha.99+build.1",
        ),
        (
            "3.0.0+build.1",
            "bundle version is not a canonical semantic version: 3.0.0+build.1",
        ),
        (
            "03.0.0",
            "bundle version is not a canonical semantic version: 03.0.0",
        ),
        (
            "3.0",
            "bundle version is not a canonical semantic version: 3.0",
        ),
        (
            "v3.0.0",
            "bundle version is not a canonical semantic version: v3.0.0",
        ),
        (
            "3.0.0-alpha.01",
            "bundle version has a leading-zero prerelease identifier",
        ),
    ] {
        let release = Release::of(&Spec {
            version: Some(version),
            ..Spec::default()
        });
        release.run(&[]).refused(said);
    }
}

#[test]
fn rejects_non_alpha_prereleases_on_alpha() {
    for version in ["3.0.0-beta.1", "3.0.0-rc.1", "3.0.0-alpha1", "3.0.0-0"] {
        let release = Release::of(&Spec {
            version: Some(version),
            ..Spec::default()
        });
        release
            .run(&[])
            .refused("alpha channel accepts only alpha prereleases or stable releases");
        // Nor does the stable channel take it.
        release
            .run(&[("channel", "stable")])
            .refused("stable channel requires a stable release");
    }
    let release = Release::of(&Spec {
        version: Some(STABLE),
        ..Spec::default()
    });
    release.run(&[]).finished();
}

#[test]
fn rejects_a_bundle_that_disagrees_with_the_source_versions() {
    let release = Release::of(&Spec {
        repo_version: Some("3.0.0-alpha.98"),
        ..Spec::default()
    });
    release
        .run(&[])
        .refused("source version 3.0.0-alpha.98 does not match bundle version 3.0.0-alpha.99\n");
}

#[test]
fn rejects_sources_that_disagree_among_themselves_even_if_one_is_the_bundles() {
    let release = Release::new();
    fs::write(
        release.repo.join("package.json"),
        r#"{"version":"3.0.0-alpha.98"}"#,
    )
    .unwrap();
    release.run(&[]).refused(
        "source package, Cargo and Tauri versions do not match: \
         package.json 3.0.0-alpha.98, Cargo.toml 3.0.0-alpha.99, tauri.conf.json 3.0.0-alpha.99\n",
    );

    fs::write(
        release.repo.join("package.json"),
        r#"{"version":"3.0.0-alpha.99+x"}"#,
    )
    .unwrap();
    release.run(&[]).refused(
        "source package.json version is not a canonical semantic version: 3.0.0-alpha.99+x\n",
    );

    fs::remove_file(release.repo.join("package.json")).unwrap();
    release
        .run(&[])
        .refused("could not read source package.json: ");
}

#[test]
fn asks_the_folder_it_is_run_in_for_the_sources_unless_told() {
    // The tests run in this crate's folder, which holds no package.json.
    let release = Release::new();
    release
        .run_without("repo")
        .refused("could not read source package.json: ");
}

#[test]
fn rejects_a_bundle_with_a_symlink_or_a_special_file_in_it() {
    let release = Release::new();
    let link = at(&release, "ConsensFlow.app/Contents/link");
    symlink("Info.plist", &link).unwrap();
    release.run(&[]).refused(
        "archive safety/content check failed: the app bundle contains a symlink: ConsensFlow.app/Contents/link\n",
    );
    fs::remove_file(&link).unwrap();
    release.run(&[]).finished();

    let _socket = UnixListener::bind(at(&release, "ConsensFlow.app/Contents/socket")).unwrap();
    release.run(&[]).refused(
        "archive safety/content check failed: the app bundle contains a special file: ConsensFlow.app/Contents/socket\n",
    );
}

#[test]
fn asks_the_bundle_before_it_asks_the_sources() {
    let release = Release::of(&Spec {
        repo_version: Some("3.0.0-alpha.98"),
        ..Spec::default()
    });
    fs::remove_file(at(&release, CF)).unwrap();
    release
        .run(&[])
        .refused("bundle is missing a window's cf: ");
}
