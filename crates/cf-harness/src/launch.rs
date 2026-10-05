//! The switch: the adapter that launches a harness's windows, built with
//! the engine's services.

use std::rc::Rc;

use cf_proto::agents::Harness;

use crate::claude::ClaudeAdapter;
use crate::codex::CodexAdapter;
use crate::contract::Adapter;
use crate::devin::DevinAdapter;
use crate::opencode::OpenCodeAdapter;
use crate::pi::PiAdapter;
use crate::seams::Services;

/// The adapter of `harness`'s windows.
pub fn adapter(harness: Harness, services: &Services) -> Rc<dyn Adapter> {
    match harness {
        Harness::Claude => Rc::new(ClaudeAdapter::new(services)),
        Harness::Codex => Rc::new(CodexAdapter::new(services)),
        Harness::Devin => Rc::new(DevinAdapter::new(services)),
        Harness::Opencode => Rc::new(OpenCodeAdapter::new(services)),
        Harness::Pi => Rc::new(PiAdapter::new(services)),
    }
}
