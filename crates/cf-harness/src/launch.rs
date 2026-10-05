//! The switch: the adapter that launches a harness's windows, built with
//! the engine's services. A harness whose launch is not yet in Rust has
//! none, and its windows stay with Node's adapters until it is.

use std::rc::Rc;

use cf_proto::agents::Harness;

use crate::claude::ClaudeAdapter;
use crate::contract::Adapter;
use crate::pi::PiAdapter;
use crate::seams::Services;

/// The adapter of `harness`'s windows, or none while its launch is Node's.
pub fn adapter(harness: Harness, services: &Services) -> Option<Rc<dyn Adapter>> {
    match harness {
        Harness::Claude => Some(Rc::new(ClaudeAdapter::new(services))),
        Harness::Pi => Some(Rc::new(PiAdapter::new(services))),
        Harness::Codex | Harness::Opencode | Harness::Devin => None,
    }
}
