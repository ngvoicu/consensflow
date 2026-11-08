//! The lints that refuse an HTTPS client (clippy.toml, T-1253) are allowed on an item, never on a
//! whole module or crate, in the crates that build one.
//!
//! The workspace's reqwest is `rustls-no-provider`: a client panics at its build where the process
//! has installed no TLS provider (the Windows update check did, T-1247). clippy.toml refuses every
//! construction of a client outside a crate's one approved constructor, and the approved
//! constructors allow `clippy::disallowed_methods` and `clippy::disallowed_types` on themselves.
//! The app once allowed `clippy::disallowed_methods` for the whole crate, which hid every client a
//! change added anywhere in it. Clippy cannot refuse a blanket allow, so this does, for the one
//! attribute that makes it: an inner attribute (`#![allow(...)]`, `#![expect(...)]`, or either
//! inside `#![cfg_attr(...)]`) that names one of those lints, in the sources of a crate that
//! depends on reqwest.
//!
//! It reads attributes, and judges no client: whether a client has its provider is the
//! approved constructor's, which its crate's tests hold. It does not read the crates' integration
//! tests (`tests/`) or build scripts, which are not product code and keep their own allows.
#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::checkout;

/// The lints a blanket allow would hide a client under.
const LINTS: [&str; 2] = ["disallowed_methods", "disallowed_types"];

/// The inner attributes at the head of `source`, each as written without its `#![` and `]`: the
/// ones a file has before its first item, after any comments.
fn inner_attributes(source: &str) -> Vec<String> {
    let chars: Vec<char> = source.chars().collect();
    let mut found = Vec::new();
    let mut at = 0;
    loop {
        while chars.get(at).is_some_and(|c| c.is_whitespace()) {
            at += 1;
        }
        match (chars.get(at), chars.get(at + 1)) {
            (Some('/'), Some('/')) => {
                while chars.get(at).is_some_and(|c| *c != '\n') {
                    at += 1;
                }
            }
            (Some('/'), Some('*')) => {
                let mut depth = 0;
                while at < chars.len() {
                    match (chars[at], chars.get(at + 1)) {
                        ('/', Some('*')) => {
                            depth += 1;
                            at += 2;
                        }
                        ('*', Some('/')) => {
                            depth -= 1;
                            at += 2;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => at += 1,
                    }
                }
            }
            (Some('#'), Some('!')) if chars.get(at + 2) == Some(&'[') => {
                let from = at + 3;
                let mut depth = 1;
                at += 3;
                while at < chars.len() && depth > 0 {
                    match chars[at] {
                        '[' => depth += 1,
                        ']' => depth -= 1,
                        '"' => {
                            at += 1;
                            while at < chars.len() && chars[at] != '"' {
                                at += if chars[at] == '\\' { 2 } else { 1 };
                            }
                        }
                        _ => {}
                    }
                    at += 1;
                }
                found.push(chars[from..at.saturating_sub(1)].iter().collect());
            }
            _ => return found,
        }
    }
}

/// Whether an inner attribute allows (or expects) one of the lints.
fn allows_a_client_lint(attribute: &str) -> bool {
    let words = attribute.replace(char::is_whitespace, "");
    (words.contains("allow(") || words.contains("expect("))
        && LINTS.iter().any(|lint| words.contains(lint))
}

/// Every Rust file under `folder`.
fn sources(folder: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(folder).expect("a folder of sources") {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// The crates of the workspace that depend on reqwest, as their folders.
fn https_crates() -> Vec<PathBuf> {
    let root = checkout();
    let mut folders = vec![root.join("app/src-tauri")];
    for group in ["crates", "tools"] {
        for entry in fs::read_dir(root.join(group)).expect("the workspace's crates") {
            folders.push(entry.unwrap().path());
        }
    }
    folders
        .into_iter()
        .filter(|folder| {
            fs::read_to_string(folder.join("Cargo.toml")).is_ok_and(|manifest| {
                manifest.lines().any(|line| {
                    let name = line.trim_start();
                    name.starts_with("reqwest")
                })
            })
        })
        .collect()
}

#[test]
fn no_attribute_of_a_whole_module_or_crate_allows_the_lints_that_refuse_a_client() {
    let crates = https_crates();
    let named: Vec<String> = crates
        .iter()
        .map(|folder| folder.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    for expected in ["src-tauri", "cf-daemon"] {
        assert!(
            named.iter().any(|name| name == expected),
            "{expected} builds HTTPS clients (the app's folder is src-tauri): it is among {named:?}"
        );
    }
    let mut blanket = Vec::new();
    for folder in &crates {
        let mut files = Vec::new();
        sources(&folder.join("src"), &mut files);
        for file in files {
            let source = fs::read_to_string(&file).unwrap();
            for attribute in inner_attributes(&source) {
                if allows_a_client_lint(&attribute) {
                    blanket.push(format!("{}: #![{attribute}]", file.display()));
                }
            }
        }
    }
    assert!(
        blanket.is_empty(),
        "these allow the lints that refuse an HTTPS client for a whole module or crate: allow them \
         on the item that needs it, with its reason (clippy.toml):\n{}",
        blanket.join("\n")
    );
}

#[test]
fn a_blanket_allow_is_found_and_a_comment_or_a_string_that_names_one_is_not() {
    let found = |source: &str| {
        inner_attributes(source)
            .iter()
            .any(|a| allows_a_client_lint(a))
    };
    assert!(found("#![allow(clippy::disallowed_methods)]\nfn a() {}"));
    assert!(found(
        "#![allow(\n    clippy::unwrap_used,\n    clippy::disallowed_types,\n)]"
    ));
    assert!(found(
        "#![expect(clippy::disallowed_methods, reason = \"x\")]"
    ));
    assert!(found(
        "#![cfg_attr(not(test), allow(clippy::disallowed_methods))]"
    ));
    // A file's comments come before its attributes: the doc of a module, and a block comment.
    assert!(found(
        "//! The module's docs.\n\n// A word.\n/* And another. */\n#![allow(clippy::disallowed_methods)]\n\nuse std::fs;"
    ));
    // Not a blanket: an item's attribute, a stricter level, other lints, comments and strings.
    assert!(!found("#[allow(clippy::disallowed_methods)]\nfn a() {}"));
    assert!(!found("#![deny(clippy::disallowed_methods)]"));
    assert!(!found("#![allow(clippy::unwrap_used)]"));
    assert!(!found(
        "// #![allow(clippy::disallowed_methods)]\nfn a() {}"
    ));
    assert!(!found(
        "/* #![allow(clippy::disallowed_methods)] */\nfn a() {}"
    ));
    assert!(!found(
        "const A: &str = \"#![allow(clippy::disallowed_methods)]\";"
    ));
}
