//! What the two tools are built with. Neither may build with the app or the
//! daemon's crates, which want the resources and the toolchain that a checkout
//! may not have (`cargo xtask stage` is what makes the first), and `cf-release`
//! may not build with xtask. Read from Cargo.lock, which is what a build
//! resolves, dev-dependencies included.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// The crates of the product that the tools run as processes (`cargo build -p …`) and never link.
const NOT_FOR_TOOLS: [&str; 5] = ["app", "cf-daemon", "cf-engine", "cf-ledger", "cf-harness"];

/// Cargo.lock as `package name -> the names it depends on`, every version of a name together.
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
            // ` "serde_json",` or ` "thiserror 2.0.20",`: the name is the first word.
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
fn neither_tool_is_built_with_the_app_or_the_daemons_crates() {
    let lock = lock();
    for tool in ["xtask", "cf-release"] {
        let built_with = closure(&lock, tool);
        // The lock is read right: what the tools are known to use is there.
        assert!(built_with.contains("cf-base"), "{tool}: {built_with:?}");
        for forbidden in NOT_FOR_TOOLS {
            assert!(
                !built_with.contains(forbidden),
                "{tool} is built with {forbidden}: the tools run it as a process, `cargo build -p {forbidden}`"
            );
        }
    }
}

#[test]
fn the_release_tool_is_not_built_with_xtask_but_xtask_may_be_built_with_it() {
    let lock = lock();
    assert!(!closure(&lock, "cf-release").contains("xtask"));
    assert!(closure(&lock, "xtask").contains("cf-release"));
}
