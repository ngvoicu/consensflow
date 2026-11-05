//! The provider is the whole process's, and the tests of this binary that run the updater's
//! plugin install it for the rest, so each test here runs in a process of its own, its binary run
//! again (`testing::in_a_child`): the child starts with none.

use super::*;
use rustls::crypto::CryptoProvider;

/// The child of `the_app_installs_ring_once_for_its_whole_process`.
const INSTALLS: &str = "CONSENSFLOW_TEST_TLS_INSTALLS";

#[test]
fn the_app_installs_ring_once_for_its_whole_process() {
    if std::env::var_os(INSTALLS).is_some() {
        assert!(
            CryptoProvider::get_default().is_none(),
            "the child starts with no TLS provider installed"
        );
        install_ring_as_default();
        assert!(CryptoProvider::get_default().is_some());
        // Again, and from another thread: nothing to install, nothing to fail.
        install_ring_as_default();
        std::thread::spawn(install_ring_as_default)
            .join()
            .expect("a second thread installs nothing more");
        // A client built with no provider of its own now builds, as the plugin's and the feed's do.
        reqwest::Client::builder()
            .build()
            .expect("a client on the process's provider");
        return;
    }
    let ran = crate::testing::in_a_child(
        "tls::tests::the_app_installs_ring_once_for_its_whole_process",
        INSTALLS,
    );
    assert!(ran.status.success(), "{}", crate::testing::said(&ran));
}

/// What the plugin does before its own client, done first: the app's install leaves it be.
#[test]
fn a_provider_installed_before_the_apps_is_left_as_it_is() {
    const FIRST: &str = "CONSENSFLOW_TEST_TLS_FIRST";
    if std::env::var_os(FIRST).is_some() {
        assert!(ring::default_provider().install_default().is_ok());
        install_ring_as_default();
        assert!(CryptoProvider::get_default().is_some());
        return;
    }
    let ran = crate::testing::in_a_child(
        "tls::tests::a_provider_installed_before_the_apps_is_left_as_it_is",
        FIRST,
    );
    assert!(ran.status.success(), "{}", crate::testing::said(&ran));
}

/// The app installs its provider before it builds anything: in `run`, ahead of the Tauri builder
/// (and so of the plugins and every thread the app starts).
#[test]
fn the_app_installs_its_provider_before_it_builds_anything() {
    let source = include_str!("../lib.rs");
    let run = &source[source
        .find("pub fn run()")
        .expect("the app's run in lib.rs")..];
    let installs = run
        .find("tls::install_ring_as_default();")
        .expect("run installs the app's TLS provider");
    let builds = run
        .find("tauri::Builder::default()")
        .expect("run builds the Tauri app");
    assert!(
        installs < builds,
        "the provider is installed after the app is built"
    );
}
