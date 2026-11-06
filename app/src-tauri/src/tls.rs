//! The TLS provider of the app's process.
//!
//! The app's HTTPS is reqwest on rustls, and reqwest is built here with
//! `rustls-no-provider` (the workspace's `reqwest`, and the updater's plugin asks for it too):
//! a client builds on the provider the process has installed, and panics at its build where it
//! has none. The updater's plugin installs ring before it builds its own client, so where the
//! app installs an update itself (the Mac) its check works; where it does not (Windows), the
//! feed is read by `update_page::read_feed`, which no plugin precedes, and the build of its
//! client panicked at every check (T-1247).
//!
//! The provider is ring, installed once for the whole process: the one provider the build
//! holds (`Cargo.lock` has no aws-lc-rs, which is what a second one would be), the one the
//! plugin installs, and so the one the plugin's own downloads and any client built later find.
//! The daemon, a process of its own, gives its client the same ring explicitly and installs
//! nothing (`cf-daemon`'s `screens::network`).

use rustls::crypto::ring;

/// Makes ring the process's TLS provider, where it has none. It may be called again, and from
/// any thread.
pub(crate) fn install_ring_as_default() {
    // The install fails only where a provider is installed already, ring's or the plugin's own
    // (also ring): the process has the one it needs either way.
    let _ = ring::default_provider().install_default();
}

#[cfg(test)]
mod tests;
