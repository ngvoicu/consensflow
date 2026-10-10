use super::*;

use crate::updater_smoke::testing::{env, fake_bundle, Fake};

fn said<T: std::fmt::Debug>(result: Result<T>) -> String {
    result.unwrap_err().to_string()
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn takes_what_the_installed_apps_check_takes_the_flips_bundle_with_node_and_the_one_without() {
    let folder = tempfile::tempdir().unwrap();
    let env = env();
    let with_node = inspect_bundle(
        &fake_bundle(&folder.path().join("a"), &Fake::default()),
        "with Node",
    )
    .unwrap();
    assert_eq!(
        (
            with_node.version.as_str(),
            with_node.node,
            with_node.cli_version.as_deref()
        ),
        ("3.0.0-alpha.82", true, Some("3.0.0-alpha.82"))
    );
    let without = inspect_bundle(
        &fake_bundle(
            &folder.path().join("b"),
            &Fake {
                node: false,
                ..Fake::default()
            },
        ),
        "without Node",
    )
    .unwrap();
    assert_eq!(
        (
            without.version.as_str(),
            without.node,
            without.cli_version.as_deref()
        ),
        ("3.0.0-alpha.82", false, None)
    );
    verify_seal(&with_node.app, &env).unwrap();
    verify_seal(&without.app, &env).unwrap();
    assert_ad_hoc(&without.app, "without Node", &env).unwrap();
    // What it names is where it is: the executable, and the window's cf.
    assert_eq!(without.binary, without.app.join("Contents/MacOS/app"));
    assert_eq!(without.cf, cf_of(&without.app));
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn refuses_another_app_two_versions_in_one_plist_a_bundle_with_no_cf_and_half_of_node() {
    let folder = tempfile::tempdir().unwrap();
    let refuse = |options: Fake, words: &str| {
        let parent = tempfile::tempdir_in(folder.path()).unwrap();
        let said = said(inspect_bundle(
            &fake_bundle(parent.path(), &options),
            "the bundle",
        ));
        assert!(said.contains(words), "{said}");
    };
    refuse(
        Fake {
            identity: "dev.example.other",
            ..Fake::default()
        },
        "not dev.ngvoicu.consensflow",
    );
    refuse(
        Fake {
            build_version: Some("3.0.0-alpha.99"),
            ..Fake::default()
        },
        "two versions differ",
    );
    refuse(
        Fake {
            cf: false,
            ..Fake::default()
        },
        "no window's cf",
    );
    let parent = tempfile::tempdir_in(folder.path()).unwrap();
    let half = fake_bundle(parent.path(), &Fake::default());
    fs::remove_file(under(
        &half,
        &["Contents", "Resources", "cli", "bin", "cf.mjs"],
    ))
    .unwrap();
    let said = said(inspect_bundle(&half, "the bundle"));
    assert!(
        said.contains("some of Node's files and not all")
            && said.contains("cf.mjs missing")
            && said.contains("node there"),
        "{said}"
    );
}

#[test]
fn a_bundle_that_is_not_one_says_what_it_lacks() {
    let folder = tempfile::tempdir().unwrap();
    let app = folder.path().join("ConsensFlow.app");
    assert!(said(inspect_bundle(&app, "the app")).contains("the app has no Contents/Info.plist"));
    fs::create_dir_all(app.join("Contents")).unwrap();
    fs::write(app.join("Contents").join("Info.plist"), "not a plist").unwrap();
    assert!(said(plist_value(&app, "CFBundleIdentifier")).contains("could not read"));
    fs::write(
        app.join("Contents").join("Info.plist"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict>\
         <key>CFBundleVersion</key><integer>3</integer></dict></plist>",
    )
    .unwrap();
    assert!(
        said(plist_value(&app, "CFBundleIdentifier")).contains("has no text CFBundleIdentifier")
    );
    assert!(
        said(plist_value(&app, "CFBundleVersion")).contains("has no text CFBundleVersion"),
        "a number is no text"
    );
}

#[test]
fn the_refused_bundles_are_two_in_this_order_each_with_the_words_it_is_refused_in() {
    assert_eq!(
        Refusal::ALL.map(|kind| (kind.name(), kind.words())),
        [
            ("without-cf", "must include cf"),
            ("tampered", "code-signature")
        ]
    );
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn makes_the_refused_bundles_the_way_the_smoke_needs_them_one_the_seal_passes_and_one_it_does_not()
{
    let folder = tempfile::tempdir().unwrap();
    let env = env();
    let update = fake_bundle(&folder.path().join("update"), &Fake::default());
    let refused = folder.path().join("refused");

    let without = refused_bundle(Refusal::WithoutCf, &update, &refused, &env).unwrap();
    verify_seal(&without, &env).unwrap();
    assert!(!cf_of(&without).exists());

    let tampered = refused_bundle(Refusal::Tampered, &update, &refused, &env).unwrap();
    let broken = said(verify_seal(&tampered, &env));
    assert!(
        ["code", "seal", "invalid", "modified"]
            .iter()
            .any(|word| broken.to_lowercase().contains(word)),
        "{broken}"
    );
    assert!(cf_of(&tampered).exists());
    assert_eq!(without, refused.join("without-cf").join("ConsensFlow.app"));
    assert_eq!(tampered, refused.join("tampered").join("ConsensFlow.app"));
    // The update they were made of is as it was.
    verify_seal(&update, &env).unwrap();
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn copies_a_bundle_whole_replacing_what_was_there_or_over_it_with_the_old_files_left() {
    let folder = tempfile::tempdir().unwrap();
    let env = env();
    let update = fake_bundle(&folder.path().join("update"), &Fake::default());
    let place = folder.path().join("Applications").join("ConsensFlow.app");
    copy_bundle(&update, &place, &env).unwrap();
    assert_eq!(
        digest_manifest(&place).unwrap(),
        digest_manifest(&update).unwrap()
    );
    fs::write(place.join("Contents").join("stale"), "an old file").unwrap();
    copy_over(&update, &place, &env).unwrap();
    assert!(
        place.join("Contents").join("stale").exists(),
        "a copy over keeps what the new one lacks"
    );
    copy_bundle(&update, &place, &env).unwrap();
    assert_eq!(
        digest_manifest(&place).unwrap(),
        digest_manifest(&update).unwrap(),
        "a replacement leaves none of it"
    );
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn archives_the_app_as_a_release_does_with_consensflow_app_at_the_root() {
    let folder = tempfile::tempdir().unwrap();
    let env = env();
    let update = fake_bundle(&folder.path().join("update"), &Fake::default());
    let archive = archive_of(&update, &folder.path().join("out").join("a.tar.gz"), &env).unwrap();
    let listed = run("/usr/bin/tar", &args!["-tzf", &archive], &env).unwrap();
    let names: Vec<_> = listed.lines().filter(|name| !name.is_empty()).collect();
    assert!(!names.is_empty());
    assert!(
        names.iter().all(|name| name.starts_with("ConsensFlow.app")),
        "{listed}"
    );
    assert!(
        names.iter().all(|name| !name.contains("/._")),
        "no copy of attributes is in it: {listed}"
    );
    let other = copy_bundle(&update, &folder.path().join("Other.app"), &env).unwrap();
    assert!(
        said(archive_of(&other, &folder.path().join("b.tar.gz"), &env))
            .contains("holds ConsensFlow.app")
    );
}

#[test]
fn a_digest_manifest_names_each_file_by_its_digest_and_mode_and_each_link_by_what_it_names() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    fs::create_dir_all(root.join("b").join("deeper")).unwrap();
    fs::write(root.join("b").join("deeper").join("file"), "bytes").unwrap();
    fs::write(root.join("a"), "").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("b/deeper/file", root.join("link")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.join("a"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let manifest = digest_manifest(root).unwrap();
    // Each directory in the order of its names, and what is in one before what follows it.
    let deeper = Path::new("b").join("deeper").join("file");
    let mut expected = vec!["a".to_string(), deeper.to_string_lossy().into_owned()];
    expected.extend(cfg!(unix).then(|| "link".to_string()));
    let names: Vec<_> = manifest.iter().map(|entry| entry.name.clone()).collect();
    assert_eq!(names, expected);
    let digest = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    assert_eq!(
        manifest[0].kind,
        Kind::File {
            sha256: digest(b""),
            mode: if cfg!(unix) { 0o755 } else { 0 },
        }
    );
    assert!(matches!(&manifest[1].kind, Kind::File { sha256, .. } if *sha256 == digest(b"bytes")));
    #[cfg(unix)]
    assert_eq!(manifest[2].kind, Kind::Link(PathBuf::from("b/deeper/file")));
    // A tree that is the same is the same manifest, and a byte more is not.
    assert_eq!(manifest, digest_manifest(root).unwrap());
    fs::write(root.join("a"), "x").unwrap();
    assert_ne!(manifest, digest_manifest(root).unwrap());
}

#[test]
fn a_file_or_a_tree_is_taken_away_where_it_is_and_nothing_is_done_where_it_is_not() {
    let folder = tempfile::tempdir().unwrap();
    let tree = folder.path().join("tree");
    fs::create_dir_all(tree.join("inside")).unwrap();
    fs::write(tree.join("inside").join("file"), "x").unwrap();
    remove_all(&tree).unwrap();
    assert!(!tree.exists());
    remove_all(&tree).unwrap();
    let file = folder.path().join("file");
    fs::write(&file, "x").unwrap();
    remove_all(&file).unwrap();
    assert!(!file.exists());
}
