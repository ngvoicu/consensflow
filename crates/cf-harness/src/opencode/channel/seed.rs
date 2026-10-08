//! A window's first message, through its own server: OpenCode ignores a prompt
//! on a `--session` launch, so once the server answers, the task is posted to
//! the conversation, once. New and reopened workers occasionally need more than
//! 15 s for the server under load, so the readiness polls and the one post
//! share 60 s, counted from the first request. Once the post starts, neither a
//! transport failure nor a timeout proves non-admission: a visible failure is
//! left, and the task is never retried here.
//!
//! Kept from Node on purpose: the settings of a resumed conversation that
//! are no JSON, or are JSON of null, are refused in sentences of this
//! module's own, where V8 threw its `SyntaxError` and its `TypeError`.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cf_base::js;
use cf_base::json::from_slice_lossy;
use cf_base::text::utf16_len;
use serde_json::{json, Map, Value};

use super::directory::real_path;
use super::{is_session_id, succeeded, url, Channel, Wires, OVERSIZED};
use crate::seams::loopback::{BodyFailed, Method, Request, TERMINATED};
use crate::seams::{arm, Armed};

/// How long the server has to answer, and the task to be posted, from the
/// first request, for a window (`seedSession`'s `timeoutMs`).
pub const LIFETIME_MS: u64 = 60_000;

/// How long one readiness poll, its body included, may take: an early
/// request can stall even after the server is ready.
const POLL_MS: u64 = 500;

/// How long a readiness poll waits before the next.
const RETRY_MS: u64 = 100;

/// The most a poll's body, and the session's settings, may say.
const BODY_LIMIT: usize = 64 * 1024;

/// What the lifetime's timer says when it fires.
const LIFETIME_OVER: &str = "OpenCode task startup timed out";

/// What a resumed conversation's settings are refused in where they name no
/// model and effort of their own.
const NO_MODEL: &str = "OpenCode session has no valid current model and effort";

/// The window's first message, and where it goes.
pub struct Seed<'a> {
    /// The conversation it is for.
    pub session: &'a str,
    /// The folder the window works in.
    pub directory: &'a str,
    pub text: &'a str,
    /// The model and effort of a fresh conversation. A resumed one keeps its
    /// own.
    pub model: Option<&'a str>,
    pub variant: Option<&'a str>,
    pub resume: bool,
    /// How long the server has to answer and the task to be posted:
    /// [`LIFETIME_MS`] for a window.
    pub lifetime_ms: u64,
}

/// Posts the first message to `channel`'s server once it answers: how it
/// failed, in ConsensFlow's sentences, or Node's for a folder that is gone.
pub async fn seed_session(
    wires: Wires<'_>,
    channel: &Channel,
    seed: &Seed<'_>,
) -> Result<(), String> {
    if !is_session_id(seed.session) || seed.text.is_empty() {
        return Err("invalid OpenCode task launch".to_owned());
    }
    if seed
        .variant
        .is_some_and(|variant| variant.is_empty() || utf16_len(variant) > 256)
    {
        return Err("invalid OpenCode reasoning effort".to_owned());
    }
    let mut body = Map::new();
    body.insert(
        "parts".to_owned(),
        json!([{ "type": "text", "text": seed.text }]),
    );
    if let Some(variant) = seed.variant {
        body.insert("variant".to_owned(), json!(variant));
    }
    if let Some(model) = seed.model.filter(|model| !model.is_empty()) {
        match model.split_once('/') {
            Some((provider, id)) if !provider.is_empty() && !id.is_empty() => {
                body.insert(
                    "model".to_owned(),
                    json!({ "providerID": provider, "modelID": id }),
                );
            }
            _ => return Err("OpenCode task model needs provider/model".to_owned()),
        }
    }
    let directory = real_path(seed.directory)?;
    let credentials = STANDARD.encode(format!("opencode:{}", channel.password));
    let asked = Asked {
        wires,
        channel,
        directory: &directory,
        headers: vec![
            ("authorization".to_owned(), format!("Basic {credentials}")),
            ("content-type".to_owned(), "application/json".to_owned()),
        ],
        lifetime: arm(wires.time, seed.lifetime_ms),
    };
    asked.ready().await?;
    if seed.resume {
        asked.adopt_settings(seed.session, &mut body).await?;
    }
    asked.admit(seed.session, body).await
}

/// What the requests of one seed have in common.
struct Asked<'a> {
    wires: Wires<'a>,
    channel: &'a Channel,
    /// The folder the window works in, as the system names it.
    directory: &'a str,
    headers: Vec<(String, String)>,
    /// The one timer of the whole startup, which every request is under.
    lifetime: Armed<'a>,
}

impl Asked<'_> {
    /// The error of work the lifetime cut short, if it has (`throwIfAborted`).
    fn alive(&self) -> Result<(), String> {
        if self.lifetime.fired() {
            Err(LIFETIME_OVER.to_owned())
        } else {
            Ok(())
        }
    }

    fn request(&self, method: Method, url: String, body: Option<Vec<u8>>) -> Request {
        Request {
            method,
            url,
            headers: self.headers.clone(),
            body,
        }
    }

    /// Polls the server's health until it says it is up: a poll that has no
    /// answer in its time, or an answer that is not yet one, is tried again
    /// after a pause; one that says it is not allowed in is the end of it.
    async fn ready(&self) -> Result<(), String> {
        loop {
            self.alive()?;
            match self.poll().await? {
                Some(401 | 403) => return Err("OpenCode task server unauthorized".to_owned()),
                Some(status) if succeeded(status) => return Ok(()),
                _ => self.wires.sleep(RETRY_MS).await,
            }
        }
    }

    /// One health request and its body, both under the poll's own time and
    /// the lifetime: the status it answered, or none where it did not in
    /// time or at all. A body of the wrong size, or one the connection cut,
    /// is thrown.
    async fn poll(&self) -> Result<Option<u16>, String> {
        let poll = arm(self.wires.time, POLL_MS);
        let url = format!("{}/global/health", self.channel.endpoint);
        let request = self.request(Method::Get, url, None);
        let sent = self
            .lifetime
            .bound(poll.bound(self.wires.loopback.send(request)))
            .await
            .flatten();
        let Some(Ok(mut reply)) = sent else {
            self.alive()?;
            return Ok(None);
        };
        let status = reply.status();
        match self
            .lifetime
            .bound(poll.bound(reply.body(BODY_LIMIT)))
            .await
            .flatten()
        {
            Some(Ok(_)) => Ok(Some(status)),
            None => self.alive().map(|()| None),
            Some(Err(failed)) => {
                self.alive()?;
                Err(body_failure(failed))
            }
        }
    }

    /// Reads a resumed conversation's own model and effort, and puts them in
    /// the task where the roster's would go: the conversation keeps what it
    /// ran on. Omitting the variant would inherit the configured agent's,
    /// not the native default, so the default is said.
    async fn adopt_settings(
        &self,
        session: &str,
        body: &mut Map<String, Value>,
    ) -> Result<(), String> {
        self.alive()?;
        let url = url::session(&self.channel.endpoint, session, self.directory);
        let request = self.request(Method::Get, url, None);
        let mut reply = match self.lifetime.bound(self.wires.loopback.send(request)).await {
            None => return Err(LIFETIME_OVER.to_owned()),
            Some(sent) => sent?,
        };
        if !succeeded(reply.status()) {
            return Err("Could not read the current OpenCode session settings".to_owned());
        }
        let bytes = match self.lifetime.bound(reply.body(BODY_LIMIT)).await {
            None => return Err(LIFETIME_OVER.to_owned()),
            Some(read) => read.map_err(body_failure)?,
        };
        let native = from_slice_lossy(&bytes)
            .map_err(|_| "the current OpenCode session settings are no JSON".to_owned())?;
        let field = |name: &str| native.get("model").and_then(|model| model.get(name));
        let (variant, agent) = (field("variant"), native.get("agent"));
        let named = native.get("id").and_then(Value::as_str) == Some(session);
        // A variant or an agent that is there at all must be valid.
        let (Some(provider), Some(id), true) =
            (valid(field("providerID")), valid(field("id")), named)
        else {
            return Err(NO_MODEL.to_owned());
        };
        if variant.is_some_and(|variant| valid(Some(variant)).is_none())
            || agent.is_some_and(|agent| valid(Some(agent)).is_none())
        {
            return Err(NO_MODEL.to_owned());
        }
        body.insert(
            "model".to_owned(),
            json!({ "providerID": provider, "modelID": id }),
        );
        let variant = variant.and_then(Value::as_str).unwrap_or("default");
        body.insert("variant".to_owned(), json!(variant));
        if let Some(agent) = agent {
            body.insert("agent".to_owned(), agent.clone());
        }
        Ok(())
    }

    /// Posts the task, once: only the lifetime bounds it, and any failure
    /// after it started leaves its admission uncertain.
    async fn admit(&self, session: &str, body: Map<String, Value>) -> Result<(), String> {
        self.alive()?;
        let url = url::prompt(&self.channel.endpoint, session, self.directory);
        let text = js::stringify(&Value::Object(body));
        let request = self.request(Method::Post, url, Some(text.into_bytes()));
        let reply = match self.lifetime.bound(self.wires.loopback.send(request)).await {
            Some(Ok(reply)) => reply,
            Some(Err(_)) | None => {
                return Err("OpenCode task admission is uncertain; task was not retried".to_owned());
            }
        };
        match reply.status() {
            204 => Ok(()),
            status => Err(format!(
                "OpenCode task admission is uncertain (HTTP {status}); task was not retried"
            )),
        }
    }
}

/// A name the settings give a model, an effort or an agent: text of 1 to 256
/// UTF-16 units.
fn valid(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty() && utf16_len(text) <= 256)
}

/// What a body that could not be read whole is thrown as.
fn body_failure(failed: BodyFailed) -> String {
    match failed {
        BodyFailed::TooLarge => OVERSIZED.to_owned(),
        BodyFailed::Cut => TERMINATED.to_owned(),
    }
}
