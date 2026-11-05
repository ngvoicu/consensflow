//! Every HTTPS client and TLS config the product builds says where its provider comes from
//! (T-1247, 3.0.7-alpha.2). The workspace's reqwest is `rustls-no-provider`: it brings none, and
//! the build of a client panics where the process has none installed ("No rustls crypto provider is
//! configured"). The app's update check on Windows panicked so at every start, since
//! `update_page::read_feed` was written: the updater's plugin installs ring before it builds its
//! own client, and on Windows no check goes through the plugin. This holds the rule over the whole
//! workspace, from the sources:
//!
//! - a file that builds a reqwest client is named in [`CLIENTS`] with what shows, in its code, where
//!   the client gets its provider: the app's `update_page.rs` installs ring for the process
//!   (`tls.rs`, which `run` calls at the start too), and `cf-daemon`'s `screens/network.rs` hands
//!   the client a rustls config built on ring, installing nothing. A new client is a new line there,
//!   with its proof, and a line whose file builds no client any more is taken out;
//! - no rustls config is built on the process's default provider (`ClientConfig::builder()`,
//!   `ServerConfig::builder()`): `builder_with_provider(ring)` names it, in a process that has none
//!   installed and in one that has;
//! - the build holds ring alone: a second provider (aws-lc-rs, which reqwest's own `rustls` feature
//!   builds in) is a second one to choose from where none is installed, and `Cargo.lock` has none.
#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::checkout;

/// The files that build a reqwest client, and what each holds of its provider: pieces of its code
/// without comments or whitespace (see [`squeezed`]), all of which must be there.
const CLIENTS: &[(&str, &[&str])] = &[
    // The feed of a release, read where the app installs no update itself (Windows).
    (
        "app/src-tauri/src/update_page.rs",
        &["crate::tls::install_ring_as_default()"],
    ),
    // The test of that install, which builds a client on the provider the install made.
    (
        "app/src-tauri/src/tls/tests.rs",
        &["install_ring_as_default()"],
    ),
    // The harness feeds, asked by the daemon.
    (
        "crates/cf-daemon/src/screens/network.rs",
        &[
            "tls_backend_preconfigured(",
            "ClientConfig::builder_with_provider(",
            "rustls::crypto::ring::default_provider()",
        ],
    ),
];

/// This file spells what it looks for.
const THIS: &str = "tools/xtask/tests/tls_provider.rs";

/// How a reqwest client is made, as the code reads once squeezed.
const BUILDS_A_CLIENT: &[&str] = &[
    "Client::builder(",
    "ClientBuilder::new(",
    "Client::new(",
    "Client::default(",
    "reqwest::get(",
];

/// How a rustls config is made on the process's default provider.
const ON_THE_DEFAULT: &[&str] = &["ClientConfig::builder(", "ServerConfig::builder("];

/// Every Rust file under `folder`, but for build folders.
fn sources(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if !["target", "node_modules", ".git"].contains(&name.as_str()) {
                sources(&path, found);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// The Rust sources of the workspace: its crates, its tools and the app's.
fn workspace() -> Vec<PathBuf> {
    let root = checkout();
    let mut found = Vec::new();
    for folder in [
        "crates",
        "tools",
        "app/src-tauri/src",
        "app/src-tauri/tests",
    ] {
        sources(&root.join(folder), &mut found);
    }
    found
}

/// The path from the checkout's root, with `/` between folders.
fn relative(path: &Path) -> String {
    path.strip_prefix(checkout())
        .unwrap()
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// A file's code without its comments and its whitespace: rustfmt may break a call anywhere, and
/// what a comment says of a client builds none.
fn squeezed(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| line.split_once("//").map_or(line, |(code, _)| code))
        .flat_map(str::chars)
        .filter(|each| !each.is_whitespace())
        .collect()
}

/// The workspace's files, relative and squeezed, but for this one.
fn code() -> Vec<(String, String)> {
    workspace()
        .iter()
        .map(|path| (relative(path), squeezed(path)))
        .filter(|(shown, _)| shown != THIS)
        .collect()
}

#[test]
fn every_client_the_product_builds_says_where_its_provider_comes_from() {
    let mut found = Vec::new();
    let mut building = Vec::new();
    for (shown, text) in code() {
        if !text.contains("reqwest") || !BUILDS_A_CLIENT.iter().any(|build| text.contains(build)) {
            continue;
        }
        building.push(shown.clone());
        match CLIENTS.iter().find(|(file, _)| *file == shown) {
            None => found.push(format!(
                "{shown}: builds a reqwest client and is not in CLIENTS, with what shows where its provider comes from"
            )),
            Some((_, proofs)) => {
                for proof in *proofs {
                    if !text.contains(proof) {
                        found.push(format!("{shown}: builds a client and lacks `{proof}`"));
                    }
                }
            }
        }
    }
    for (file, _) in CLIENTS {
        if !building.iter().any(|shown| shown == file) {
            found.push(format!(
                "{file}: is in CLIENTS and builds no reqwest client"
            ));
        }
    }
    assert!(
        found.is_empty(),
        "reqwest here brings no TLS provider (rustls-no-provider): a client panics at its build where the process has none:\n{}",
        found.join("\n")
    );
}

#[test]
fn no_tls_config_is_built_on_the_default_provider() {
    let found: Vec<String> = code()
        .into_iter()
        .filter_map(|(shown, text)| {
            ON_THE_DEFAULT
                .iter()
                .find(|build| text.contains(*build))
                .map(|build| format!("{shown}: `{build}…` takes the process's default provider"))
        })
        .collect();
    assert!(
        found.is_empty(),
        "name the provider, builder_with_provider(Arc::new(rustls::crypto::ring::default_provider())):\n{}",
        found.join("\n")
    );
}

#[test]
fn the_build_holds_ring_as_its_only_tls_provider() {
    let lock = fs::read_to_string(checkout().join("Cargo.lock")).expect("Cargo.lock");
    for name in ["aws-lc-rs", "aws-lc-sys"] {
        assert!(
            !lock.contains(&format!("name = \"{name}\"")),
            "Cargo.lock holds {name}: ring is the one TLS provider the app and the daemon build on"
        );
    }
    assert!(
        lock.contains("name = \"ring\""),
        "Cargo.lock holds no ring: the provider the app installs and the daemon hands its client"
    );
}
