//! What the engine asks of a harness (`src/adapters/`, as
//! `src/core/windows.js` and `deliveries.js` ask it): prepare a launch,
//! and of the window opened on it, whether it is ready, a delivery into it,
//! and a look at it.
//!
//! The engine runs on one thread (step 3.5: tokio's `current_thread`), so
//! what may wait (on the pane host, a harness's server, a child, a timer) is
//! a future that need not be `Send`. A window keeps what JavaScript kept in
//! its `launch` bag, in cells never borrowed across an await, behind `&self`:
//! the engine may close a window while a delivery into it still waits.

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use cf_proto::agents::Harness;
use serde_json::Value;

use crate::records::{Item, Options, Quota, Reading};

/// What waits, on the engine's one thread.
pub type Work<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// A harness's side of the engine: how its windows are launched.
pub trait Adapter {
    /// Prepares a window on `launch`: writes the files it runs with and
    /// says how it opens, or why it cannot (the launch then fails with that
    /// sentence).
    fn prepare<'a>(&'a self, launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>>;
}

/// A window to launch (`adapter.prepare`'s argument).
#[derive(Debug, Clone)]
pub struct Launch<'a> {
    pub id: &'a LaunchId,
    /// The participant's project and handle, which Pi names its window by.
    pub project: i64,
    pub handle: &'a str,
    /// `chief`, `worker`, `advisor`, `reviewer` or `designer`.
    pub role: &'a str,
    /// The project's folder, where the window opens.
    pub directory: &'a str,
    /// The conversation to open again, when the participant has one.
    pub resume: Option<&'a str>,
    /// The window's first message, as the engine wrote it.
    pub message: Option<&'a str>,
    /// The agent it runs on: none for a chief from before every chief had one.
    pub agent: Option<Agent<'a>>,
    /// The role's text.
    pub instructions: &'a str,
}

/// The model a window runs on and the levels its harness reads; an empty
/// one is none, as JavaScript's `if (model)` read them.
#[derive(Debug, Clone, Copy, Default)]
pub struct Agent<'a> {
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub thinking: Option<&'a str>,
}

/// A launch's id: a uuid in lowercase (`core/launch-files.js`), the name of
/// the folders its files are written in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LaunchId(String);

impl LaunchId {
    /// The launch id `text` is, if it is one.
    pub fn new(text: &str) -> Option<Self> {
        let groups: Vec<&str> = text.split('-').collect();
        let shaped = groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
            && text
                .bytes()
                .all(|byte| byte == b'-' || byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        shaped.then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// How a prepared window opens.
pub struct Prepared {
    pub argv: Vec<String>,
    /// What its environment adds, in the order its harness built it.
    pub env: Vec<(String, String)>,
    /// The keys it must not inherit.
    pub drop_env: Vec<String>,
    /// The conversation it opens on, where ConsensFlow names it.
    pub native_session: Option<String>,
    pub window: Rc<dyn Window>,
}

/// A window open on its harness.
pub trait Window {
    /// Its pane opened, the window's own process `pid` when the host knew it.
    fn opened(&self, pid: Option<u32>);
    /// The window shows `session` now (a `/clear` or `/resume` in it): later
    /// looks follow it there.
    fn follow(&self, session: &str);
    /// The conversation the window started on, where its harness says it
    /// itself; none where ConsensFlow named it.
    fn started(&self) -> Work<'_, Result<Option<String>, String>>;
    /// Whether a delivery may go in now.
    fn ready<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
    ) -> Work<'a, Result<Readiness, HostError>>;
    /// Hands `text` to the window's harness.
    fn deliver<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
        text: &'a str,
    ) -> Work<'a, Admission>;
    /// What the window's harness says of it now.
    fn observe(&self) -> Work<'_, Result<Observed, String>>;
}

/// A window's pane, by the pane host's id and the generation it opened in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    pub id: String,
    pub generation: u64,
}

/// The pane host, as an adapter asks it: its harness's own requests
/// (`pane.snapshot`, `pane.write_paste`, `pane.claim`), passed straight
/// through (`core/pane-host.js`). Opening and ending panes are the engine's.
pub trait PaneHost {
    fn request<'a>(&'a self, op: &'a str, body: Value) -> Work<'a, Result<Value, HostError>>;
}

/// A request the pane host never answered: the bridge ended, or its deadline
/// passed, `error` its word for which, when it said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostError {
    pub error: Option<String>,
    pub message: String,
}

/// Whether a window may take a delivery now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    Held(Held),
}

/// Why a delivery waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Held {
    /// The human typed into the window and has not sent it: the board says so.
    Unsent,
    /// The window shows another conversation than its launch's.
    ShowsAnother,
    /// A harness that said no more than "not yet" (Devin's `false`).
    Unsaid,
    Because(String),
}

impl Held {
    /// The sentence the engine says it in.
    pub fn sentence(&self) -> &str {
        match self {
            Held::Unsent => "you have typed in this window and not sent it",
            Held::ShowsAnother => "the window shows another conversation",
            Held::Unsaid => "a paste is on its way",
            Held::Because(reason) => reason,
        }
    }
}

/// What became of a delivery: only one refused before the harness could
/// take it is known not to have reached it; any other failure may have, and
/// the harness's own record decides (`admission`, `adapters/shared.js`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// Handed over; `queued` where the harness's own queue took it.
    Admitted {
        queued: bool,
    },
    Refused {
        reason: String,
    },
    Uncertain {
        reason: String,
    },
}

/// What a look at a window found.
#[derive(Debug, Clone, PartialEq)]
pub struct Observed {
    /// The harness's record of the conversation.
    pub reading: Arc<Reading>,
    pub settled: bool,
    pub waiting: Option<Waiting>,
    pub failed: bool,
    pub quota: Option<Arc<Quota>>,
    /// The window shows another conversation, this one: the look is the
    /// old conversation's last, and nothing in it settles.
    pub switched: Option<String>,
    /// The window has not said which conversation it shows (it is
    /// starting, switching conversations or reconnecting).
    pub unnamed: bool,
}

impl Observed {
    /// The conversation's items: none where its record could not be read.
    pub fn items(&self) -> &[Item] {
        match &*self.reading {
            Reading::Known(record) => &record.items,
            Reading::Unknown(_) => &[],
        }
    }
}

/// A window that waits on something its own dialog holds, and why where its
/// harness said: nothing is pasted into it meanwhile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiting {
    pub reason: Option<String>,
}

/// A harness's own record of a conversation, as the engine serves it to
/// the windows: a worker that owns the readers reads it off the engine's
/// thread, where a first look at a long transcript takes a second.
pub trait Records {
    /// What `harness`'s record of `session` says now (`cachedAnswers`).
    fn look<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
        options: &'a Options,
    ) -> Work<'a, Arc<Reading>>;
    /// Whether `harness` kept a transcript of `session` at all
    /// (`hasTranscript`), or why that cannot be told.
    fn has_transcript<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
    ) -> Work<'a, Result<bool, String>>;
}
