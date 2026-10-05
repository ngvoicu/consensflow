//! The human's screens (`agentsUi`, `src/core/agents-server.js`): the agents
//! (`/`) and the harness diagnostics (`/harnesses`), each a page, and the
//! routes they call. The app opens them with the UI token and puts it on every
//! request; the agents' own tokens open none of this, and the UI token opens
//! none of the agents' API.
//!
//! [`Screens::handle`] is Node's `handle`, in its order:
//!
//! 1. a path that is none of the screens' is the agents' API's: none is
//!    answered ([`recognize`]);
//! 2. the UI token, as the bearer or, where the bearer says nothing, in the
//!    query; without it, 401 `{error: "unauthorized"}`, a bare one (the API's
//!    own 401 says why);
//! 3. a GET of a page or of the agents is answered, and no body is read;
//! 4. any other method has its body read, as the one JSON value it holds (`{}`
//!    where it has none), before any route is looked for: a body that is no
//!    JSON fails a request that has no route as it fails one that has;
//! 5. the route the method and the path name, else 404 `{error: "not found"}`.
//!
//! Anything that throws is 400 `{error: <its words>}`: a refusal of the
//! roster's, a body too large, a harness the admin will not update. Every
//! write to the roster is followed by `on_roster_change`.
//!
//! - [`pages`] holds the two pages as the text Node served,
//! - [`agents`] the roster's routes,
//! - [`harnesses`] the diagnostics',
//! - [`body`] what a route reads of its body, and
//! - [`network`] where the harness feeds are asked.

mod agents;
mod body;
mod harnesses;
pub mod network;
mod pages;

use std::rc::Rc;

use cf_base::env::Env;
use cf_harness::admin::HarnessAdmin;
use hyper::Method;
use serde_json::{json, Map, Value};

use crate::api::answer::Answer;
use crate::api::credentials::token_matches;
use crate::api::request::Request;
use crate::roster::Agents;

pub use agents::offerable;

/// The screens' own paths, in the order Node lists them (`:69-78`). A path that
/// is none of these is the agents' API's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    /// `/` and `/harnesses`: a page.
    Page,
    /// `/api/agents`: the roster's agents.
    Agents,
    /// `/api/agents/<name>`: one agent of the human's own.
    Agent(String),
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
            is_agent_name(name).then(|| Screen::Agent(name.to_owned()))
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
    /// The environment the daemon runs in, which says what is installed here.
    pub env: Env,
    /// The saved agents, listed and changed by the routes.
    pub agents: Rc<Agents>,
    /// What is known of each harness's CLI, and its update.
    pub admin: HarnessAdmin,
}

impl Screens {
    /// Answers a request that is one of the screens', or none when its path is
    /// not theirs.
    pub async fn handle(&self, request: &mut Request) -> Option<Answer> {
        let screen = recognize(&request.path)?;
        if !self.opens(request) {
            return Some(Answer::json(401, json!({ "error": "unauthorized" })));
        }
        Some(
            self.answer(screen, request)
                .await
                .unwrap_or_else(|words| Answer::json(400, json!({ "error": words }))),
        )
    }

    /// Whether `request` carries the UI token: as the bearer, which is read as
    /// none when it is empty, else as the query's first `token`.
    fn opens(&self, request: &Request) -> bool {
        let presented = request
            .bearer()
            .filter(|bearer| !bearer.is_empty())
            .or_else(|| request.param("token"))
            .unwrap_or_default();
        !presented.is_empty() && token_matches(presented, &self.token)
    }

    /// The route's answer, or the words of why it threw.
    async fn answer(&self, screen: Screen, request: &mut Request) -> Result<Answer, String> {
        let get = request.method == Method::GET;
        if get {
            match screen {
                Screen::Page => return Ok(Answer::html(self.page(&request.path))),
                Screen::Agents => return self.list_agents(),
                _ => {}
            }
        }
        let body = if get {
            Value::Object(Map::new())
        } else {
            body::read(request).await?
        };
        match (&request.method, screen) {
            (&Method::POST, Screen::Agents) => self.add_agent(&body),
            (&Method::POST, Screen::Preferences) => self.set_preferences(&body),
            (&Method::POST, Screen::HarnessUpdate) => self.update_harness(&body).await,
            (&Method::POST, Screen::HarnessCheck) => self.check_harnesses(&body).await,
            (&Method::PATCH, Screen::Agent(name)) => self.edit_agent(&name, &body),
            (&Method::DELETE, Screen::Agent(name)) => self.remove_agent(&name),
            _ => Ok(Answer::json(404, json!({ "error": "not found" }))),
        }
    }

    /// The page at `path`, `/` or `/harnesses`, made for the UI token.
    fn page(&self, path: &str) -> String {
        if path == "/" {
            pages::agents(&self.token)
        } else {
            pages::harnesses(&self.token)
        }
    }
}

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
