//! The build holds one TLS provider, ring (T-1247, 3.0.7-alpha.2). The workspace's reqwest is
//! `rustls-no-provider`: a client builds on the provider the process has installed. The app installs
//! ring (`app/src-tauri/src/tls.rs`) and the daemon hands its client a config built on ring; a
//! second provider in the build (aws-lc-rs, which reqwest's own `rustls` feature builds in) would be
//! a second to choose from where none is installed. `Cargo.lock` holds none, and holds ring.
#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::fs;

use common::checkout;

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
