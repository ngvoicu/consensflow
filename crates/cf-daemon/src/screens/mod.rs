//! The human's screens (`agentsUi`, `src/core/agents-server.js`): the agents
//! (`/`) and the harness diagnostics (`/harnesses`), each a page, and the
//! routes they call. The app opens them with the UI token and puts it on every
//! request; the agents' own tokens open none of this, and the UI token opens
//! none of the agents' API.
//!
//! This is their place, and nothing of them yet: [`Screens::handle`] answers
//! none of them, so a screen's path falls through to the agents' API and is
//! answered as Node answers a path it has no route for (401 from a caller with
//! no window token, else 404) until the screens land. What the landing needs of
//! the daemon is here: the UI token, which [`Screens::token`] holds, and what
//! the routes that change the roster must do once they have (`on_roster_change`,
//! `daemon.js:103-106`).

use std::rc::Rc;

use crate::api::answer::Answer;
use crate::api::request::Request;

/// The screens' own paths, in the order Node lists them (`:69-78`). A path
/// that is none of these is the agents' API's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    /// `GET /` and `GET /harnesses`: a page.
    Page,
    /// `/api/agents` and `/api/agents/<name>`: the roster's agents.
    Agents,
    /// `/api/preferences`.
    Preferences,
    /// `/api/harnesses/check`.
    HarnessCheck,
    /// `/api/harnesses/update`.
    HarnessUpdate,
}

/// The screen a path is, whatever the method; none for any other path.
pub fn recognize(path: &str) -> Option<Screen> {
    match path {
        "/" | "/harnesses" => Some(Screen::Page),
        "/api/agents" => Some(Screen::Agents),
        "/api/preferences" => Some(Screen::Preferences),
        "/api/harnesses/check" => Some(Screen::HarnessCheck),
        "/api/harnesses/update" => Some(Screen::HarnessUpdate),
        _ => {
            let name = path.strip_prefix("/api/agents/")?;
            is_agent_name(name).then_some(Screen::Agents)
        }
    }
}

/// `[a-z][a-z0-9-]*`: what the path of an agent may name.
fn is_agent_name(name: &str) -> bool {
    let mut letters = name.chars();
    letters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && letters
            .all(|letter| letter.is_ascii_lowercase() || letter.is_ascii_digit() || letter == '-')
}

/// The screens, mounted by the API under the UI token.
pub struct Screens {
    /// The app's own token, which opens these screens and nothing else.
    pub token: String,
    /// What a change to the roster does: the members' tiers follow the agents'
    /// again, the page is told if one moved, and the dispatcher is woken. Its
    /// failure is the words of why the tiers could not follow, which fail the
    /// request as Node's throw did, the change itself being made.
    pub on_roster_change: Rc<dyn Fn() -> Result<(), String>>,
}

impl Screens {
    /// Answers a request that is one of the screens', or none when its path is
    /// not theirs. Until they land, it answers none.
    pub async fn handle(&self, _request: &mut Request) -> Option<Answer> {
        None
    }
}

#[cfg(test)]
mod tests;
