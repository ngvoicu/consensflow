//! What an archive holds: what the command refuses of one that is not the
//! bundle, or is not safe to unpack. The archive is read here by `tar` and
//! `flate2`; the script it replaced had Python read it. (The archive as a file is
//! `files.rs`'s.)

use std::fs;
use std::os::unix::fs::PermissionsExt;

use tar::EntryType;

use super::fixture::EXECUTABLE as BIN;
use super::fixture::{differs, unsound, Item, Release, Spec, CF, INFO_PLIST, ROOT};

#[test]
fn rejects_an_archive_whose_packaged_version_differs() {
    let release = Release::of(&Spec {
        archive_version: Some("3.0.0-alpha.98"),
        ..Spec::default()
    });
    release
        .run(&[])
        .refused("archive packaged version does not match the bundle\n");
}

#[test]
fn rejects_an_archive_that_says_a_version_that_is_no_version() {
    let release = Release::of(&Spec {
        archive_version: Some("banana"),
        ..Spec::default()
    });
    release
        .run(&[])
        .refused("archived bundle version is not a canonical semantic version: banana\n");
}

#[test]
fn rejects_an_archive_whose_file_bytes_differ_from_the_supplied_bundle() {
    let mut release = Release::new();
    let bundled = release.archived.item(CF).data.len();
    release.archived.item(CF).data =
        b"#!/bin/sh\necho changed after the bundle was made\n".to_vec();
    let archived = release.archived.item(CF).data.len();
    release.archive_again();
    release.run(&[]).refused(&differs(
        CF,
        &format!("differs: {bundled} bytes in the bundle, {archived} in the archive"),
    ));
}

#[test]
fn rejects_bytes_that_differ_and_are_as_many() {
    let mut release = Release::new();
    release.archived.item(CF).data = b"#!/bin/sh\necho 3.0.0-alpha.98\n".to_vec();
    assert_eq!(release.archived.item(CF).data.len(), 30);
    release.archive_again();
    release
        .run(&[])
        .refused(&differs(CF, "differs: the bytes differ"));
}

#[test]
fn rejects_an_entry_whose_mode_is_not_the_bundles() {
    for (path, mode, how) in [
        (
            BIN,
            0o644,
            "differs: mode 755 in the bundle, 644 in the archive",
        ),
        (
            BIN,
            0o700,
            "differs: mode 755 in the bundle, 700 in the archive",
        ),
        (
            CF,
            0o777,
            "differs: mode 755 in the bundle, 777 in the archive",
        ),
        (
            INFO_PLIST,
            0o755,
            "differs: mode 644 in the bundle, 755 in the archive",
        ),
        (
            INFO_PLIST,
            0o000,
            "differs: mode 644 in the bundle, 0 in the archive",
        ),
        (
            "ConsensFlow.app/Contents",
            0o700,
            "differs: mode 755 in the bundle, 700 in the archive",
        ),
        (
            ROOT,
            0o775,
            "differs: mode 755 in the bundle, 775 in the archive",
        ),
    ] {
        let mut release = Release::new();
        release.archived.item(path).mode = mode;
        release.archive_again();
        release.run(&[]).refused(&differs(path, how));
    }
}

#[test]
fn rejects_a_set_user_id_or_sticky_bit_that_the_bundle_does_not_have() {
    for (mode, written) in [
        (0o4755, "4755"),
        (0o2755, "2755"),
        (0o1755, "1755"),
        (0o7755, "7755"),
    ] {
        let mut release = Release::new();
        release.archived.item(BIN).mode = mode;
        release.archive_again();
        release.run(&[]).refused(&differs(
            BIN,
            &format!("differs: mode 755 in the bundle, {written} in the archive"),
        ));
    }
}

#[test]
fn takes_the_special_bits_the_bundle_has_as_well() {
    let mut release = Release::new();
    let binary = release
        .bundle
        .join("Contents")
        .join("MacOS")
        .join("ConsensFlow");
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o4755)).unwrap();
    release.archived.item(BIN).mode = 0o4755;
    release.archive_again();
    release.run(&[]).finished();
}

#[test]
fn takes_a_mode_that_carries_the_type_of_the_file_as_some_archivers_write_it() {
    let mut release = Release::new();
    for item in &mut release.archived.items {
        let type_bits = if item.kind == EntryType::Directory {
            0o040000
        } else {
            0o100000
        };
        item.mode |= type_bits;
    }
    release.archive_again();
    release.run(&[]).finished();
}

#[test]
fn rejects_a_path_of_another_kind_than_the_bundles() {
    let mut release = Release::new();
    release.archived.item(BIN).kind = EntryType::Directory;
    release.archived.item(BIN).data.clear();
    release.archive_again();
    release.run(&[]).refused(&differs(
        BIN,
        "differs: a file in the bundle, a folder in the archive",
    ));

    let mut release = Release::new();
    let folder = "ConsensFlow.app/Contents/Resources/cli/bin";
    release.archived.item(folder).kind = EntryType::Regular;
    release.archive_again();
    release.run(&[]).refused(&differs(
        folder,
        "differs: a folder in the bundle, a file in the archive",
    ));
}

#[test]
fn rejects_a_path_the_archive_has_and_the_bundle_has_not_and_the_other_way() {
    let mut release = Release::new();
    release.archived.push(Item::file(
        "ConsensFlow.app/Contents/extra.txt",
        0o644,
        b"x",
    ));
    release.archive_again();
    release.run(&[]).refused(&unsound(
        "the archive content manifest does not match the supplied bundle: \
         ConsensFlow.app/Contents/extra.txt is in the archive and not in the bundle",
    ));

    let mut release = Release::new();
    release.archived.remove(CF);
    release.archive_again();
    release.run(&[]).refused(&unsound(&format!(
        "the archive content manifest does not match the supplied bundle: \
         {CF} is in the bundle and not in the archive"
    )));
}

#[test]
fn tells_the_first_path_in_the_order_of_the_names_where_they_differ() {
    let mut release = Release::new();
    release.archived.item(INFO_PLIST).mode = 0o600;
    release.archived.item(CF).mode = 0o600;
    release.archived.item(ROOT).mode = 0o700;
    release.archive_again();
    release.run(&[]).refused(&differs(
        ROOT,
        "differs: mode 755 in the bundle, 700 in the archive",
    ));
}

#[test]
fn does_not_mind_the_order_of_the_entries_or_how_a_name_is_written() {
    let mut release = Release::new();
    release.archived.items.reverse();
    release.archive_again();
    release.run(&[]).finished();

    // As `tar -c .` writes them, with slashes doubled, and with a `.` among them.
    let writes: [fn(&str) -> String; 3] = [
        |name| format!("./{name}"),
        |name| name.replace('/', "//"),
        |name| name.replace("Contents/", "Contents/./"),
    ];
    for write in writes {
        let mut release = Release::new();
        for item in &mut release.archived.items {
            item.name = write(&item.name);
        }
        release.archive_again();
        release.run(&[]).finished();
    }
}

#[test]
fn rejects_archives_with_traversal_and_absolute_entries() {
    for name in [
        "../evil.txt",
        "/tmp/cf-evil.txt",
        "ConsensFlow.app/../evil.txt",
        "ConsensFlow.app/Contents/../../evil.txt",
        "Other.app/Contents/x",
        "evil/ConsensFlow.app/x",
        "//ConsensFlow.app/Contents/x",
        "evil.txt",
    ] {
        // Among the entries of a good archive, and as the only one.
        let mut release = Release::new();
        release.archived.push(Item::file(name, 0o644, b"evil"));
        release.archive_again();
        let said = unsound(&format!(
            "the update archive contains an unsafe path: {name}"
        ));
        release.run(&[]).refused(&said);

        release.archived.items = vec![Item::file(name, 0o644, b"evil")];
        release.archive_again();
        release.run(&[]).refused(&said);
    }
}

#[test]
fn rejects_a_path_given_twice_however_it_is_written() {
    for twice in [
        BIN,
        "ConsensFlow.app//Contents/MacOS/ConsensFlow",
        "ConsensFlow.app/./Contents/MacOS/ConsensFlow",
        "./ConsensFlow.app/Contents/MacOS/ConsensFlow",
    ] {
        let mut release = Release::new();
        release.archived.push(Item::file(twice, 0o755, b"binary\n"));
        release.archive_again();
        release.run(&[]).refused(&unsound(&format!(
            "the update archive contains a duplicate path: {BIN}"
        )));
    }
    // A folder twice.
    let mut release = Release::new();
    release.archived.push(Item::dir("ConsensFlow.app/Contents"));
    release.archive_again();
    release.run(&[]).refused(&unsound(
        "the update archive contains a duplicate path: ConsensFlow.app/Contents",
    ));
}

#[test]
fn rejects_a_link_of_either_kind_wherever_it_points() {
    for (kind, target) in [
        (EntryType::Symlink, "Info.plist"),
        (EntryType::Symlink, "/etc/passwd"),
        (EntryType::Symlink, "../../../.."),
        (EntryType::Link, "ConsensFlow.app/Contents/Info.plist"),
        (EntryType::Link, "/etc/passwd"),
    ] {
        let mut release = Release::new();
        release
            .archived
            .push(Item::link("ConsensFlow.app/Contents/link", kind, target));
        release.archive_again();
        release.run(&[]).refused(&unsound(
            "the update archive contains a symlink or hard link: ConsensFlow.app/Contents/link",
        ));
    }
}

#[test]
fn rejects_an_entry_that_is_a_pipe_or_a_device() {
    for kind in [
        EntryType::Fifo,
        EntryType::Char,
        EntryType::Block,
        EntryType::Continuous,
    ] {
        let mut release = Release::new();
        release
            .archived
            .push(Item::special("ConsensFlow.app/Contents/odd", kind));
        release.archive_again();
        release.run(&[]).refused(&unsound(
            "the update archive contains a special entry: ConsensFlow.app/Contents/odd",
        ));
    }
}

#[test]
fn judges_a_path_before_what_kind_of_entry_it_is() {
    let mut release = Release::new();
    release
        .archived
        .push(Item::link("../escape", EntryType::Symlink, "/etc"));
    release.archive_again();
    release.run(&[]).refused(&unsound(
        "the update archive contains an unsafe path: ../escape",
    ));
}

#[test]
fn rejects_an_archive_with_no_root_or_no_info_plist() {
    let mut release = Release::new();
    release.archived.remove(ROOT);
    release.archive_again();
    release
        .run(&[])
        .refused(&unsound("the update archive has no ConsensFlow.app root"));

    let mut release = Release::new();
    release.archived.items.truncate(1);
    release.archive_again();
    release.run(&[]).refused(&unsound(
        "the update archive has no readable ConsensFlow.app/Contents/Info.plist",
    ));
}

#[test]
fn rejects_an_info_plist_in_the_archive_that_says_no_version() {
    let none = unsound("the update archive has no readable ConsensFlow.app/Contents/Info.plist");
    let no_version = unsound("the archive has no readable packaged version");
    for (plist, said) in [
        ("not a plist".to_string(), &none),
        (String::new(), &none),
        ("<plist version=\"1.0\"><array/></plist>".to_string(), &none),
        (
            "<plist version=\"1.0\"><dict/></plist>".to_string(),
            &no_version,
        ),
        (
            "<plist version=\"1.0\"><dict><key>CFBundleShortVersionString</key><integer>3</integer></dict></plist>"
                .to_string(),
            &no_version,
        ),
    ] {
        let mut release = Release::new();
        release.archived.item(INFO_PLIST).data = plist.into_bytes();
        release.archive_again();
        release.run(&[]).refused(said);
    }
    // One that is a folder, or is not there at all.
    let mut release = Release::new();
    release.archived.item(INFO_PLIST).kind = EntryType::Directory;
    release.archived.item(INFO_PLIST).data.clear();
    release.archive_again();
    release.run(&[]).refused(&none);
}

#[test]
fn reads_an_info_plist_as_large_as_an_app_has_one_and_no_larger() {
    // The bundle and the archive hold the same Info.plist, padded to a size.
    for (kilobytes, taken) in [(900, true), (2048, false)] {
        let mut release = Release::new();
        let plain = String::from_utf8(release.archived.item(INFO_PLIST).data.clone()).unwrap();
        let padding = format!(
            "<key>Padding</key><string>{}</string>",
            "x".repeat(kilobytes * 1024)
        );
        let padded = plain.replace("</dict>", &format!("{padding}</dict>"));
        fs::write(release.bundle.join("Contents").join("Info.plist"), &padded).unwrap();
        release.archived.item(INFO_PLIST).data = padded.into_bytes();
        release.archive_again();
        let ran = release.run(&[]);
        if taken {
            ran.finished();
        } else {
            ran.refused(&unsound(
                "the update archive has no readable ConsensFlow.app/Contents/Info.plist",
            ));
        }
    }
}
