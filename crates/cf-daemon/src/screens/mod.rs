//! The human's screen: the harness diagnostics (`/harnesses`), a page, and the
//! routes it calls (the human's saved agents and their screen are no more: a
//! member is a harness, a model, an effort and a tier, spec harness-models). The app opens them
//! with the UI token and puts it on every request; the agents' own tokens open
//! none of this, and the UI token opens none of the agents' API.
//!
//! [`Screens::handle`] is Node's `handle`, in its order:
//!
//! 1. a path that is none of the screens' is the agents' API's: none is
//!    answered ([`recognize`]);
//! 2. the UI token, as the bearer or, where the bearer says nothing, in the
//!    query; without it, 401 `{error: "unauthorized"}`, a bare one (the API's
//!    own 401 says why);
//! 3. a GET of the page is answered, and no body is read;
//! 4. any other method has its body read, as the one JSON value it holds (`{}`
//!    where it has none), before any route is looked for: a body that is no
//!    JSON fails a request that has no route as it fails one that has;
//! 5. the route the method and the path name, else 404 `{error: "not found"}`.
//!
//! Anything that throws is 400 `{error: <its words>}`: a body too large, a
//! harness the admin will not update.
//!
//! - [`pages`] holds the page as the text Node served,
//! - [`harnesses`] the diagnostics' routes,
//! - [`body`] what a route reads of its body, and
//! - [`network`] where the harness feeds are asked.

mod body;
mod harnesses;
pub mod network;
mod pages;

use cf_base::env::Env;
use cf_harness::admin::HarnessAdmin;
use hyper::Method;
use serde_json::{json, Map, Value};

use crate::api::answer::Answer;
use crate::api::credentials::token_matches;
use crate::api::request::Request;

/// The screens' own paths, in the order Node lists them (`:69-78`). A path that
/// is none of these is the agents' API's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    /// `/harnesses`: the page.
    Page,
    /// `/api/harnesses/check`.
    HarnessCheck,
    /// `/api/harnesses/update`.
    HarnessUpdate,
}

/// The screen a path is, whatever the method; none for any other path.
pub fn recognize(path: &str) -> Option<Screen> {
    match path {
        "/harnesses" => Some(Screen::Page),
        "/api/harnesses/check" => Some(Screen::HarnessCheck),
        "/api/harnesses/update" => Some(Screen::HarnessUpdate),
        _ => None,
    }
}

/// The screens, mounted by the API under the UI token.
pub struct Screens {
    /// The app's own token, which opens these screens and nothing else.
    pub token: String,
    /// The environment the daemon runs in, which says what is installed here.
    pub env: Env,
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
        if get && screen == Screen::Page {
            return Ok(Answer::html(pages::harnesses(&self.token)));
        }
        let body = if get {
            Value::Object(Map::new())
        } else {
            body::read(request).await?
        };
        match (&request.method, screen) {
            (&Method::POST, Screen::HarnessUpdate) => self.update_harness(&body).await,
            (&Method::POST, Screen::HarnessCheck) => self.check_harnesses(&body).await,
            _ => Ok(Answer::json(404, json!({ "error": "not found" }))),
        }
    }
}

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
