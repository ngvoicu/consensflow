//! The inputs of a run: both apps held to what the update needs of them.

use super::*;

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn takes_two_apps_that_are_the_installed_ones_and_the_updates_checked_once() {
    let folder = tempfile::tempdir().unwrap();
    let Some((root, key)) = run_key(folder.path()) else {
        return;
    };
    let made = apps(folder.path(), &Fake::default(), &update());
    let inputs = load(folder.path(), &root, &key, &made.from, &made.to).unwrap();
    assert_eq!(
        (inputs.from.version.as_str(), inputs.to.version.as_str()),
        ("3.0.0-alpha.82", "3.0.0-alpha.83")
    );
    assert_eq!(
        (inputs.from.label.as_str(), inputs.to.label.as_str()),
        ("FROM_APP", "TO_APP")
    );
    assert_eq!(inputs.from_manifest, digest_manifest(&made.from).unwrap());
    assert_eq!(inputs.to_manifest, digest_manifest(&made.to).unwrap());
    assert_eq!(inputs.release, Release::Flip);
    assert!(
        inputs.from.node && !inputs.to.node,
        "the flip's ships Node, the update's does not"
    );
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn refuses_inputs_that_are_not_what_the_update_needs() {
    let folder = tempfile::tempdir().unwrap();
    let Some((root, key)) = run_key(folder.path()) else {
        return;
    };
    let made = apps(folder.path(), &Fake::default(), &update());
    let load = |from: &Path, to: &Path| refusal(load(folder.path(), &root, &key, from, to));
    assert!(load(&made.from, &made.from)
        .contains("FROM_APP and TO_APP must be distinct source bundles"));
    // Newer is newer: the same version, and an older one, are no update.
    let same = fake_bundle(&folder.path().join("same"), &Fake::default());
    let said = load(&made.from, &same);
    assert!(
        said.contains(
            "the update (3.0.0-alpha.82) must be newer than the installed app (3.0.0-alpha.82)"
        ),
        "{said}"
    );
    // Not a bundle of ours, not a bundle at all, and not a built app.
    let other = fake_bundle(
        &folder.path().join("other"),
        &Fake {
            identity: "dev.example.other",
            ..update()
        },
    );
    assert!(load(&made.from, &other)
        .contains("TO_APP is dev.example.other, not dev.ngvoicu.consensflow"));
    assert!(load(&made.from, &folder.path().join("nowhere.app")).starts_with("could not find "));
    assert!(load(&made.from, folder.path()).contains("--to-app must point to a .app bundle"));
    // A CLI of another version than the app's own.
    let odd = fake_bundle(&folder.path().join("odd"), &Fake::default());
    fs::write(
        odd.join("Contents/Resources/cli/package.json"),
        "{\"version\":\"1.0.0\"}",
    )
    .unwrap();
    let said = load(&odd, &made.to);
    assert!(
        said.contains("FROM_APP: its CLI's version is not its own"),
        "{said}"
    );
    // A seal that does not verify.
    let broken = fake_bundle(&folder.path().join("broken"), &update());
    fs::write(
        broken.join("Contents/Resources/cli/bin/cf"),
        "changed after signing",
    )
    .unwrap();
    assert!(load(&made.from, &broken).contains("codesign"));
}

#[test]
fn what_a_built_app_is_a_bundle_that_is_there_where_it_really_is() {
    // A folder that is no app is none, and what is not there is said.
    let folder = tempfile::tempdir().unwrap();
    let said = refusal(built_path(folder.path(), "--from-app"));
    assert!(
        said.contains("--from-app must point to a .app bundle"),
        "{said}"
    );
    let said = refusal(built_path(&folder.path().join("no.app"), "--from-app"));
    assert!(said.starts_with("could not find "), "{said}");
    let app = folder.path().join("ConsensFlow.app");
    fs::create_dir_all(&app).unwrap();
    assert_eq!(
        built_path(&app, "--to-app").unwrap(),
        fs::canonicalize(&app).unwrap()
    );
    // Where the system keeps a folder behind a link, the app is where it really is.
    #[cfg(unix)]
    {
        let link = folder.path().join("Linked.app");
        std::os::unix::fs::symlink(&app, &link).unwrap();
        assert_eq!(
            built_path(&link, "--to-app").unwrap(),
            fs::canonicalize(&app).unwrap()
        );
    }
}

#[test]
fn the_installed_app_in_applications_is_not_a_built_app() {
    // What is under /Applications is the user's app, which the smoke never touches.
    for path in [
        "/Applications/ConsensFlow.app",
        "/Applications/Utilities/Some.app",
        "/Applications",
    ] {
        assert!(is_installed(Path::new(path)), "{path}");
    }
    for path in [
        "/ApplicationsOther/ConsensFlow.app",
        "/Users/someone/Applications/ConsensFlow.app",
        "/tmp/Applications/ConsensFlow.app",
        "ConsensFlow.app",
    ] {
        assert!(!is_installed(Path::new(path)), "{path}");
    }
}
