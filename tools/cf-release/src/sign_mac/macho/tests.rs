//! Which files are code: told by their first bytes, and found folder by folder.

use std::fs;

use super::*;

fn file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    path
}

/// The eight bytes a file begins with: a magic, then a number (a count, or a version).
fn head(magic: u32, next: u32) -> Vec<u8> {
    [magic.to_be_bytes(), next.to_be_bytes()].concat()
}

#[test]
fn a_file_is_code_by_the_magic_it_begins_with_in_any_of_the_four_thin_forms() {
    let dir = tempfile::tempdir().unwrap();
    for (name, magic) in [
        ("32-bit", 0xfeed_face),
        ("64-bit", 0xfeed_facf),
        ("32-bit, little endian", 0xcefa_edfe),
        ("64-bit, little endian", 0xcffa_edfe),
    ] {
        let path = file(dir.path(), name, &head(magic, 7));
        assert!(is_mach_o(&path).unwrap(), "{name}");
    }
}

#[test]
fn a_universal_binary_is_code_but_a_class_file_that_shares_its_magic_is_not() {
    let dir = tempfile::tempdir().unwrap();
    // Where a binary counts its slices, a class file has its version: 45 at the least.
    for (slices, code) in [
        (1, true),
        (2, true),
        (44, true),
        (45, false),
        (52, false),
        (65, false),
    ] {
        for magic in [0xcafe_babe, 0xcafe_babf] {
            let path = file(dir.path(), "universal", &head(magic, slices));
            assert_eq!(is_mach_o(&path).unwrap(), code, "{magic:x} with {slices}");
        }
    }
}

#[test]
fn what_is_too_short_or_begins_with_anything_else_is_not_code_whatever_its_name() {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in [
        ("empty", Vec::new()),
        (
            "seven bytes of a thin one",
            head(0xfeed_facf, 0)[..7].to_vec(),
        ),
        ("text", b"#!/bin/sh\nexit 0\n".to_vec()),
        ("zeros", vec![0; 64]),
        ("one off", head(0xfeed_fad0, 0)),
    ] {
        let path = file(dir.path(), &format!("{name}.dylib"), &bytes);
        assert!(!is_mach_o(&path).unwrap(), "{name}");
    }
    // And code with no extension is code.
    assert!(is_mach_o(&file(dir.path(), "cf", &head(0xcffa_edfe, 0))).unwrap());
}

#[test]
fn the_code_under_a_folder_is_listed_folder_by_folder_in_the_order_of_their_names() {
    let dir = tempfile::tempdir().unwrap();
    let code = head(0xfeed_facf, 0);
    let root = dir.path();
    let (z, y, x) = (
        file(root, "b/z", &code),
        file(root, "a/y", &code),
        file(root, "a/b/x", &code),
    );
    let c = file(root, "c", &code);
    file(root, "d.txt", b"text");
    file(root, "a/e/Foo.class", &head(0xcafe_babe, 52));
    // Folders and files in one order of names, written in another.
    assert_eq!(mach_os(root).unwrap(), [x, y, z, c]);
}

#[cfg(unix)]
#[test]
fn a_link_is_neither_followed_nor_listed() {
    let dir = tempfile::tempdir().unwrap();
    let code = head(0xfeed_facf, 0);
    let outside = tempfile::tempdir().unwrap();
    let real = file(outside.path(), "tool", &code);
    file(outside.path(), "folder/inner", &code);
    let inside = dir.path().join("app");
    fs::create_dir(&inside).unwrap();
    std::os::unix::fs::symlink(&real, inside.join("link-to-a-file")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("folder"),
        inside.join("link-to-a-folder"),
    )
    .unwrap();
    std::os::unix::fs::symlink(inside.join("nowhere"), inside.join("dangling")).unwrap();
    assert_eq!(mach_os(&inside).unwrap(), Vec::<PathBuf>::new());
}

#[test]
fn a_folder_that_cannot_be_read_is_told_by_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    let told = mach_os(&missing).unwrap_err().to_string();
    assert!(
        told.starts_with(&format!("could not read {}: ", missing.display())),
        "{told}"
    );
}

#[cfg(unix)]
#[test]
fn a_file_that_cannot_be_opened_is_an_error_and_not_a_file_that_is_not_code() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = file(dir.path(), "locked", &head(0xfeed_facf, 0));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    // A user who can read it anyway (root) has nothing to refuse.
    if fs::File::open(&path).is_err() {
        let told = mach_os(dir.path()).unwrap_err().to_string();
        assert!(
            told.starts_with(&format!("could not read {}: ", path.display())),
            "{told}"
        );
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
}
