//! ConsensFlow's integration with each coding-agent harness: one module per
//! harness, each importing only what all of them share (`shared`), never a
//! sibling. It holds the hooks a harness runs through `cf hook`, the
//! readers of what each harness's own record of a conversation says
//! ([`records`]), where each harness's CLI is ([`detect`]), how the engine
//! launches a harness's window and works with it ([`contract`]), what it is
//! given to do so ([`seams`]), which adapter launches which harness
//! ([`launch::adapter`]), the files a launch leaves ([`forget_launch`],
//! [`sweep_launches`]), and what opening the app prepares: the terminal
//! command beside the Pi extension ([`prepare`], which, like
//! the adapters' table, sits above the harnesses' modules).

#![cfg_attr(not(test), forbid(unsafe_code))]
#![cfg_attr(test, deny(unsafe_code))]

#[cfg(test)]
#[allow(unsafe_code)]
mod standin_boot {
    ctor::declarative::ctor! {
        #[ctor(unsafe)]
        fn begin_stand_in_process() {
            cf_e2e::standin_process::start(false);
        }
    }
}

pub mod admin;
pub mod background;
pub mod claude;
pub mod codex;
pub mod contract;
pub mod detect;
pub mod devin;
pub mod launch;
pub mod muse;
#[cfg(test)]
mod page_listings;
pub mod pi;
pub mod prepare;
pub mod records;
pub mod seams;
mod shared;

pub use devin::forget_kept;
pub use shared::admission::Sent;
pub use shared::pane::{enter, look};
pub use shared::look::{
    after_wait, licence_enter, licence_paste, AfterWait, At, BoxReading, BoxState, EnterFor,
    EnterLicence, Look, NoLook, PasteLicence, NO_LOOK, SCREEN_QUIET_FOR_ENTER_MS,
};
pub use shared::launch_files::{forget_launch, sweep_launches, NotRemoved};
pub use shared::specmint::bundled_specmint;

// Fakes other crates' tests compile too: a failure in one is the test's.
#[cfg(any(test, feature = "test-support"))]
#[allow(clippy::expect_used, clippy::unwrap_used)]
pub mod testing;

// The answers the live tools put to the harness code, through
// the test binary `harness-ask`: they ship with nothing.
#[cfg(any(test, feature = "test-support"))]
pub mod tooling;
