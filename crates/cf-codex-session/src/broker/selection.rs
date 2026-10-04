//! Which thread a window shows, and whether a delivery may go to it: the
//! broker's state, changed only by what passes through it and by what the
//! daemon asks. It does no I/O, so each transition is a function call.
//!
//! The TUI starts, resumes or forks the thread it shows with a request, and
//! Codex's answer to that request names the thread. A request that may switch
//! the main thread is remembered by its id; its answer applies only when it is
//! the latest switch and its connection still owns the window. The server
//! numbers the requests it sends the TUI on its own, so only a response (a
//! message with no `method`) ever answers a request of the TUI's.

use std::collections::HashMap;

use cf_base::js;
use cf_base::json::js_order;
use cf_proto::codex::Refusal;
use serde_json::{Map, Value};

/// One TUI connection, as the broker names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ClientId(pub(crate) u64);

/// The id a request carries: any JSON value, or none. Ids are compared as
/// JSON values, so `1` and `"1"` are two ids.
type RequestId = Option<Value>;

/// What a TUI's request that was not answered yet may change.
#[derive(Debug, Clone, PartialEq)]
enum Pending {
    /// It may switch the main thread. `prior` is what the window showed
    /// before, to go back to if Codex refuses.
    Switch {
        revision: u64,
        prior: Option<String>,
        prior_empty: bool,
        main_resume: bool,
    },
    /// It names the permissions of the shown thread.
    PermissionChange,
}

/// The requests one TUI connection made and has no answer to yet.
#[derive(Debug, Default)]
pub(crate) struct Requests(Vec<(RequestId, Pending)>);

impl Requests {
    /// Remembers `pending` for `id`, in place of what that id had.
    fn insert(&mut self, id: RequestId, pending: Pending) {
        match self.0.iter_mut().find(|(known, _)| *known == id) {
            Some(entry) => entry.1 = pending,
            None => self.0.push((id, pending)),
        }
    }

    fn take(&mut self, id: &RequestId) -> Option<Pending> {
        let at = self.0.iter().position(|(known, _)| known == id)?;
        Some(self.0.remove(at).1)
    }
}

/// A delivery that passed the broker's checks: the thread it goes to, and
/// what the window was when it was checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Admission {
    pub(crate) thread: String,
    revision: u64,
    /// Whether the thread was idle, so the message may start its turn.
    pub(crate) idle: bool,
}

/// The broker's state.
#[derive(Debug)]
pub(crate) struct Selection {
    /// The main thread the TUI shows.
    selected: Option<String>,
    /// Whether nothing was said in it yet.
    empty: bool,
    /// The TUI connection that chose the thread.
    owner: Option<ClientId>,
    /// Whether the window is moving to another thread: nothing is taken then.
    switching: bool,
    /// Counts every change of the window's thread.
    revision: u64,
    /// Whether each thread is idle, as Codex last said.
    idle: HashMap<String, bool>,
    /// Whether the window's first start is still to open in full-permission mode.
    fresh_bypass: bool,
    closed: bool,
    /// Whether the broker's own connection to Codex is initialized.
    ready: bool,
}

impl Selection {
    pub(crate) fn new(fresh_bypass: bool) -> Self {
        Self {
            selected: None,
            empty: false,
            owner: None,
            switching: false,
            revision: 0,
            idle: HashMap::new(),
            fresh_bypass,
            closed: false,
            ready: false,
        }
    }

    /// The broker's connection to Codex is initialized.
    pub(crate) fn ready(&mut self) {
        self.ready = true;
    }

    /// The broker's connection to Codex is gone.
    pub(crate) fn lose_control(&mut self) {
        self.ready = false;
    }

    /// The broker is closed: it takes nothing more and shows no thread.
    pub(crate) fn close(&mut self) {
        self.closed = true;
        self.selected = None;
        self.ready = false;
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed
    }

    /// The thread the window shows; none while it switches.
    pub(crate) fn session_id(&self) -> Option<&str> {
        self.selected.as_deref().filter(|_| !self.switching)
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether the shown thread has had nothing said in it.
    pub(crate) fn is_empty(&self) -> bool {
        !self.switching && self.empty
    }

    /// Whether a delivery would be taken now. `control_open` is whether the
    /// broker's connection to Codex is open.
    pub(crate) fn available(&self, control_open: bool) -> bool {
        !self.closed && self.ready && control_open && !self.switching && self.selected.is_some()
    }

    /// Takes a delivery for `session` if the window shows that thread and
    /// the broker can reach Codex: the thread is then used, so its turn starts
    /// or queues. Done in one step, so nothing can switch the window between
    /// the check and the request that follows it.
    pub(crate) fn admit(
        &mut self,
        session: &str,
        control_open: bool,
    ) -> Result<Admission, Refusal> {
        if !self.available(control_open) {
            return Err(Refusal::NativeSessionUnavailable);
        }
        let Some(thread) = self.selected.clone().filter(|thread| thread == session) else {
            return Err(Refusal::NativeSessionChanged);
        };
        self.empty = false;
        let idle = self.idle.get(&thread) == Some(&true);
        if idle {
            self.idle.insert(thread.clone(), false);
        }
        Ok(Admission {
            thread,
            revision: self.revision,
            idle,
        })
    }

    /// Whether the window still shows what `admission` was checked against.
    pub(crate) fn is_current(&self, admission: &Admission) -> bool {
        self.revision == admission.revision
            && self.selected.as_deref() == Some(admission.thread.as_str())
    }

    /// What a TUI connection said to Codex. Remembers it if it may change the
    /// window's thread or permissions, and returns the message to send in its
    /// place when the first start must open in full-permission mode.
    pub(crate) fn tui_said(
        &mut self,
        client: ClientId,
        requests: &mut Requests,
        message: &Value,
    ) -> Option<Value> {
        let method = message.get("method").and_then(Value::as_str);
        let none = Map::new();
        let params = message
            .get("params")
            .and_then(Value::as_object)
            .unwrap_or(&none);
        let user = params.get("threadSource").and_then(Value::as_str) == Some("user");
        // The TUI's own resume and fork name their workspace roots: a list, or
        // since Codex 0.159 null, so it is the field being there that counts.
        let roots = params.contains_key("runtimeWorkspaceRoots");
        let main_start = method == Some("thread/start")
            && params.get("ephemeral") == Some(&Value::Bool(false))
            && user;
        let main_resume = method == Some("thread/resume") && roots;
        let main_fork = method == Some("thread/fork")
            && params.get("ephemeral") != Some(&Value::Bool(true))
            && user
            && roots;
        let id = message.get("id").cloned();
        let mut rewritten = None;
        if main_start || main_resume || main_fork {
            // The window shows none while it switches; an error answer goes back to what it showed.
            let prior = if self.switching {
                None
            } else {
                self.selected.take()
            };
            let prior_empty = !self.switching && self.empty;
            self.owner = Some(client);
            self.empty = false;
            self.switching = true;
            self.revision += 1;
            requests.insert(
                id.clone(),
                Pending::Switch {
                    revision: self.revision,
                    prior,
                    prior_empty,
                    main_resume,
                },
            );
            if main_start && self.fresh_bypass {
                rewritten = Some(with_full_permissions(message));
            }
        }
        let names_selected = self.selected.is_some()
            && params.get("threadId").and_then(Value::as_str) == self.selected.as_deref();
        if method == Some("thread/settings/update")
            && names_selected
            && ["approvalPolicy", "sandbox", "permissions"]
                .iter()
                .any(|key| params.contains_key(*key))
        {
            requests.insert(id, Pending::PermissionChange);
        }
        if matches!(method, Some("turn/start" | "thread/queue/add")) && names_selected {
            self.empty = false;
            if let Some(thread) = &self.selected {
                self.idle.insert(thread.clone(), false);
            }
        }
        rewritten
    }

    /// What Codex said to a TUI connection: the answer to a request it made,
    /// or a message of Codex's own.
    pub(crate) fn codex_said(
        &mut self,
        client: ClientId,
        requests: &mut Requests,
        message: &Value,
    ) {
        // Only a response answers a request of the TUI's: the server numbers
        // the requests it sends on its own, and one may carry the same id.
        let pending = if message.get("method").is_none() {
            requests.take(&message.get("id").cloned())
        } else {
            None
        };
        let failed = js::truthy(message.get("error"));
        match pending {
            Some(Pending::PermissionChange) if !failed => self.fresh_bypass = false,
            Some(Pending::Switch {
                revision,
                prior,
                prior_empty,
                main_resume,
            }) if self.owner == Some(client) && revision == self.revision => {
                self.switched(message, failed, prior, prior_empty, main_resume);
            }
            _ => {}
        }
        self.observe(message);
    }

    /// What Codex said to the broker's own connection.
    pub(crate) fn control_said(&mut self, message: &Value) {
        self.observe(message);
    }

    /// The answer to the latest switch of the window's owner.
    fn switched(
        &mut self,
        message: &Value,
        failed: bool,
        prior: Option<String>,
        prior_empty: bool,
        main_resume: bool,
    ) {
        if failed {
            self.selected = prior;
            self.empty = prior_empty;
        } else {
            let result = message.get("result");
            let thread = result.and_then(|result| result.get("thread"));
            let candidate = thread
                .and_then(|thread| thread.get("id"))
                .and_then(Value::as_str)
                .filter(|id| is_thread_id(id))
                .filter(|_| !js::truthy(result.and_then(|result| result.get("readOnly"))));
            self.selected = candidate.map(str::to_string);
            let idle = status_is_idle(thread);
            self.empty = candidate.is_some()
                && thread
                    .and_then(|thread| thread.get("turns"))
                    .and_then(Value::as_array)
                    .is_some_and(Vec::is_empty)
                && idle;
            if let Some(selected) = &self.selected {
                if main_resume {
                    self.fresh_bypass = false;
                }
                self.idle.insert(selected.clone(), idle);
            }
        }
        self.switching = false;
    }

    /// Learns which threads are idle from what Codex says of them.
    fn observe(&mut self, message: &Value) {
        let method = message.get("method").and_then(Value::as_str);
        let params = message.get("params");
        let Some(thread) = params
            .and_then(|params| params.get("threadId"))
            .and_then(Value::as_str)
        else {
            return;
        };
        match method {
            Some("thread/status/changed") => {
                let status = params.and_then(|params| params.get("status"));
                let idle = status
                    .and_then(|status| status.get("type"))
                    .and_then(Value::as_str)
                    == Some("idle");
                self.idle.insert(thread.to_string(), idle);
            }
            Some("turn/started") => {
                self.idle.insert(thread.to_string(), false);
                if self.selected.as_deref() == Some(thread) {
                    self.empty = false;
                }
            }
            Some("turn/completed") => {
                self.idle.insert(thread.to_string(), true);
            }
            _ => {}
        }
    }

    /// A TUI connection ended: when it chose the thread, the window shows none.
    pub(crate) fn retire(&mut self, client: ClientId) {
        if self.owner == Some(client) {
            self.selected = None;
            self.empty = false;
            self.owner = None;
            self.switching = false;
            self.revision += 1;
        }
    }
}

/// A thread's `status.type` is `idle`.
fn status_is_idle(thread: Option<&Value>) -> bool {
    thread
        .and_then(|thread| thread.get("status"))
        .and_then(|status| status.get("type"))
        .and_then(Value::as_str)
        == Some("idle")
}

/// `message` with its params opening the thread in full-permission mode, the
/// way it goes out: keys in the order JavaScript wrote them.
fn with_full_permissions(message: &Value) -> Value {
    let mut params = message
        .get("params")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    params.insert("approvalPolicy".into(), "never".into());
    params.insert("sandbox".into(), "danger-full-access".into());
    params.insert("permissions".into(), Value::Null);
    let mut message = message.clone();
    if let Some(object) = message.as_object_mut() {
        object.insert("params".into(), Value::Object(params));
    }
    js_order(message)
}

/// A thread id as Codex writes one: a hyphenated UUID of version 1 to 8 and
/// variant 8 to b, in any case.
pub(crate) fn is_thread_id(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(at, byte)| match at {
            8 | 13 | 18 | 23 => *byte == b'-',
            14 => matches!(byte, b'1'..=b'8'),
            19 => matches!(byte.to_ascii_lowercase(), b'8'..=b'9' | b'a'..=b'b'),
            _ => byte.is_ascii_hexdigit(),
        })
}

#[cfg(test)]
mod tests;
