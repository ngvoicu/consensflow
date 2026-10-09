//! What the publisher is built with, read from Cargo.lock. The release
//! workflow builds this crate with `--locked` in a job that holds no secret and
//! runs the binary in a job that holds the repository's write token, so every
//! crate in its closure is code that runs under that token: the lists below are
//! the whole of it, and a dependency added later fails here until a person has
//! read what it brings.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// The two dependencies, and the only two.
const DEPENDENCIES: [&str; 2] = ["serde_json", "sha2"];

/// Every other crate they bring, by name, as the lockfile resolves them. The
/// lockfile names a crate's dependencies for every platform and for every
/// feature that any crate of the workspace turns on, so this is a superset of
/// what the build compiles (`cargo build -p cf-publish` compiles eighteen
/// crates on a Mac: the two above and sixteen of these). The rest are not built
/// here: `serde`, which `serde_json` lists for a target no build is made for
/// (`cargo tree` shows it only with `--target all`), with its proc-macro
/// (`serde_derive`, `syn`, `quote`, `proc-macro2`, `unicode-ident`), and what
/// another member's features add (`const-oid`, `hybrid-array`, `foldhash`,
/// `autocfg`). One the build needs that is not here fails this test, and so
/// does one here that the lockfile no longer reaches.
const CLOSURE: [&str; 26] = [
    "autocfg",
    "block-buffer",
    "cfg-if",
    "const-oid",
    "cpufeatures",
    "crypto-common",
    "digest",
    "equivalent",
    "foldhash",
    "generic-array",
    "hashbrown",
    "hybrid-array",
    "indexmap",
    "itoa",
    "libc",
    "memchr",
    "proc-macro2",
    "quote",
    "serde",
    "serde_core",
    "serde_derive",
    "syn",
    "typenum",
    "unicode-ident",
    "version_check",
    "zmij",
];

/// Cargo.lock as `package name -> the names it depends on`, every version of a
/// name together. Dev-dependencies are in it too, which is why this crate has none.
fn lock() -> BTreeMap<String, BTreeSet<String>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let text = fs::read_to_string(root.join("Cargo.lock")).unwrap();
    let mut packages: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let (mut name, mut listing) = (String::new(), false);
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("name = \"") {
            name = value.trim_end_matches('"').to_string();
            packages.entry(name.clone()).or_default();
        } else if line == "dependencies = [" {
            listing = true;
        } else if line == "]" || line == "[[package]]" {
            listing = false;
        } else if listing {
            // ` "serde_json",` or ` "indexmap 2.14.0",`: the name is the first word.
            let quoted = line.trim().trim_matches(|c| c == '"' || c == ',');
            let dependency = quoted.split(' ').next().unwrap_or_default();
            packages
                .entry(name.clone())
                .or_default()
                .insert(dependency.to_string());
        }
    }
    packages
}

/// Every package that `root` is built with, itself included.
fn closure(lock: &BTreeMap<String, BTreeSet<String>>, root: &str) -> BTreeSet<String> {
    let (mut seen, mut pending) = (BTreeSet::new(), vec![root.to_string()]);
    while let Some(name) = pending.pop() {
        if seen.insert(name.clone()) {
            pending.extend(lock.get(&name).into_iter().flatten().cloned());
        }
    }
    seen
}

#[test]
fn names_serde_json_and_sha2_and_no_other_dependency() {
    let lock = lock();
    // The self-dependency is the test kit's: `cf-publish` with `test-support`.
    let mut direct: BTreeSet<_> = lock["cf-publish"].iter().map(String::as_str).collect();
    direct.remove("cf-publish");
    assert_eq!(
        direct,
        BTreeSet::from(DEPENDENCIES),
        "cf-publish runs under the write token: a dependency (a dev-dependency too) is added here on purpose, with tests/closure.rs"
    );
}

#[test]
fn is_built_with_nothing_but_what_those_two_bring() {
    let lock = lock();
    let mut built_with = closure(&lock, "cf-publish");
    built_with.remove("cf-publish");
    // The lock is read right: what the two are known to use is there.
    assert!(built_with.contains("serde_json") && built_with.contains("sha2"));
    let listed: BTreeSet<String> = DEPENDENCIES
        .iter()
        .chain(CLOSURE.iter())
        .map(|name| (*name).to_string())
        .collect();
    let added: Vec<_> = built_with.difference(&listed).collect();
    let gone: Vec<_> = listed.difference(&built_with).collect();
    assert!(
        added.is_empty(),
        "cf-publish is now built with {added:?}, which runs under the write token: read it, then list it in CLOSURE"
    );
    assert!(
        gone.is_empty(),
        "CLOSURE lists {gone:?}, which cf-publish is no longer built with: take it out"
    );
}

#[test]
fn builds_with_no_crate_of_the_product_and_no_runtime() {
    let lock = lock();
    let built_with = closure(&lock, "cf-publish");
    for forbidden in ["tokio", "thiserror", "tempfile", "cf-base", "cf-process"] {
        assert!(
            !built_with.contains(forbidden),
            "cf-publish is built with {forbidden}, which runs under the write token"
        );
    }
    assert!(
        !built_with
            .iter()
            .any(|name| name.starts_with("cf-") && name != "cf-publish"),
        "cf-publish is built with a crate of the product: {built_with:?}"
    );
}
