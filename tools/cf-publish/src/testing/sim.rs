//! GitHub on this machine, as far as the release workflow touches it: the
//! releases and their assets that `gh` makes (the commands the publisher runs,
//! and no others: one it does not know, or a flag it does not know, is a
//! failure, so a command the publisher starts using is noticed here), and the
//! addresses installed apps and the checks download from, which serve the
//! published ones (a draft's assets are not served).
//!
//! ```text
//! let github = Github::start();
//! github.base()                  // where files are downloaded from: <base>/<tag>/<name>
//! github.gh(&args, cwd)          // one `gh` call: the answer
//! github.release(tag, spec)      // a release as it already is
//! github.fail(|args, index| …)   // says why a call fails, or nothing
//! github.serve(path, served)     // what a download address serves instead
//! github.requests()              // the download addresses asked for
//! github.trace()                 // after each call: the call, and every release's assets
//! ```
//!
//! `POST /__gh` runs a call for a `gh` that is a process of its own (the
//! workflow's step, run as written, with the `fake-gh` binary on its PATH).

mod commands;

use std::collections::{BTreeMap, HashMap};
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use serde_json::{json, Value};

use self::commands::{api, refuse, release, snapshot};
use super::http::{Reply, Request, Server};
use crate::gh::{Answer, Gh};

/// What a download address serves instead of the release's file.
#[derive(Clone, Debug)]
pub enum Served {
    /// A status and no body.
    Status(u16),
    /// A body, with 200, whatever the release holds.
    Body(Vec<u8>),
    /// The connection dropped, with no answer.
    Reset,
    /// A redirect to another address, as GitHub's downloads are.
    Redirect(String),
}

impl From<u16> for Served {
    fn from(status: u16) -> Self {
        Served::Status(status)
    }
}

impl From<&str> for Served {
    fn from(body: &str) -> Self {
        Served::Body(body.as_bytes().to_vec())
    }
}

impl From<String> for Served {
    fn from(body: String) -> Self {
        Served::Body(body.into_bytes())
    }
}

type Sometimes = Arc<Mutex<dyn FnMut() -> Option<Served> + Send>>;
type Failing = Arc<Mutex<dyn FnMut(&[String], usize) -> Option<String> + Send>>;

#[derive(Clone)]
enum Override {
    Always(Served),
    /// Asked on each request: what it serves then, or nothing, for the release's file.
    Sometimes(Sometimes),
}

struct Asset {
    id: u64,
    data: Vec<u8>,
}

struct Model {
    draft: bool,
    prerelease: bool,
    title: String,
    notes: String,
    /// In the order they were made; a renamed asset moves to the end.
    assets: Vec<(String, Asset)>,
}

/// A release as the tests give it: whether it is a draft or a prerelease, and
/// its assets (name to bytes).
#[derive(Default)]
pub struct Spec {
    draft: bool,
    prerelease: bool,
    assets: Vec<(String, Vec<u8>)>,
}

impl Spec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn draft(mut self) -> Self {
        self.draft = true;
        self
    }

    pub fn prerelease(mut self) -> Self {
        self.prerelease = true;
        self
    }

    /// These assets, in this order.
    pub fn assets<N: AsRef<str>, D: AsRef<[u8]>>(
        mut self,
        assets: impl IntoIterator<Item = (N, D)>,
    ) -> Self {
        self.assets.extend(
            assets
                .into_iter()
                .map(|(name, data)| (name.as_ref().to_string(), data.as_ref().to_vec())),
        );
        self
    }
}

/// A release's state after a call: whether it is a draft, and its assets' names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub draft: bool,
    pub assets: Vec<String>,
}

/// One `gh` call: its arguments, how it ended, and every release after it.
#[derive(Clone, Debug)]
pub struct Call {
    pub args: Vec<String>,
    pub status: i32,
    pub after: BTreeMap<String, Snapshot>,
}

#[derive(Default)]
struct State {
    releases: BTreeMap<String, Model>,
    overrides: HashMap<String, Override>,
    requests: Vec<String>,
    trace: Vec<Call>,
    failing: Option<Failing>,
    calls: usize,
    next_id: u64,
}

struct Inner {
    repo: String,
    base: OnceLock<String>,
    state: Mutex<State>,
}

/// GitHub, to ask and to change: what a test and its closures hold.
#[derive(Clone)]
pub struct Handle(Arc<Inner>);

/// GitHub, serving on loopback until it is dropped.
pub struct Github {
    handle: Handle,
    _server: Server,
}

impl Deref for Github {
    type Target = Handle;

    fn deref(&self) -> &Handle {
        &self.handle
    }
}

impl Gh for Github {
    fn run(&self, args: &[String], cwd: &Path) -> Answer {
        self.handle.gh(args, cwd)
    }
}

impl Gh for Handle {
    fn run(&self, args: &[String], cwd: &Path) -> Answer {
        self.gh(args, cwd)
    }
}

impl Github {
    /// GitHub as `ngvoicu/consensflow`, with no release yet.
    pub fn start() -> Self {
        let handle = Handle(Arc::new(Inner {
            repo: "ngvoicu/consensflow".to_string(),
            base: OnceLock::new(),
            state: Mutex::new(State {
                next_id: 1000,
                ..State::default()
            }),
        }));
        let serving = handle.clone();
        let server = Server::start(Arc::new(move |request| serving.answer(request)));
        let _ = handle.0.base.set(server.base());
        Self {
            handle,
            _server: server,
        }
    }

    /// A handle that outlives the borrow, for a closure the simulator calls.
    pub fn handle(&self) -> Handle {
        self.handle.clone()
    }
}

fn respond(status: u16, body: Vec<u8>) -> Reply {
    Reply::Respond {
        status,
        headers: Vec::new(),
        body,
    }
}

impl Handle {
    fn state(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().expect("the simulator's state")
    }

    /// Where files are downloaded from: `<base>/<tag>/<name>`.
    pub fn base(&self) -> &str {
        self.0.base.get().map_or("", String::as_str)
    }

    /// `<owner>/<repo>`.
    pub fn repo(&self) -> &str {
        &self.0.repo
    }

    /// One `gh` call, in the folder `cwd`.
    pub fn gh(&self, args: &[String], cwd: &Path) -> Answer {
        let (index, failing) = {
            let mut state = self.state();
            let index = state.calls;
            state.calls += 1;
            (index, state.failing.clone())
        };
        // Asked with the state free: it may be a closure that changes a release.
        let why = failing.and_then(|failing| {
            let mut failing = failing.lock().expect("the failing closure");
            failing(args, index)
        });
        let answer = match why {
            Some(why) => refuse(&why, 1),
            None => {
                let mut state = self.state();
                let (group, rest) = args
                    .split_first()
                    .map_or(("", &[][..]), |(group, rest)| (group.as_str(), rest));
                let answer = match group {
                    "release" => match rest.split_first() {
                        Some((verb, rest)) => release(&mut state, self.repo(), verb, rest, cwd),
                        None => Err("the simulator does not know gh release undefined".to_string()),
                    },
                    "api" => api(&mut state, self.repo(), rest),
                    other => Err(format!("the simulator does not know gh {other}")),
                };
                answer.unwrap_or_else(|message| refuse(&message, 2))
            }
        };
        let mut state = self.state();
        let after = snapshot(&state);
        state.trace.push(Call {
            args: args.to_vec(),
            status: answer.status,
            after,
        });
        answer
    }

    /// Every call so far: its arguments.
    pub fn calls(&self) -> Vec<Vec<String>> {
        self.state()
            .trace
            .iter()
            .map(|call| call.args.clone())
            .collect()
    }

    /// Every call so far, with how it ended and every release after it.
    pub fn trace(&self) -> Vec<Call> {
        self.state().trace.clone()
    }

    /// The download addresses asked for, in order.
    pub fn requests(&self) -> Vec<String> {
        self.state().requests.clone()
    }

    /// A release as it already is.
    pub fn release(&self, tag: &str, spec: Spec) {
        let mut state = self.state();
        let assets = spec
            .assets
            .into_iter()
            .map(|(name, data)| {
                let id = state.next_id;
                state.next_id += 1;
                (name, Asset { id, data })
            })
            .collect();
        state.releases.insert(
            tag.to_string(),
            Model {
                draft: spec.draft,
                prerelease: spec.prerelease,
                title: tag.to_string(),
                notes: String::new(),
                assets,
            },
        );
    }

    /// An asset's bytes replaced where it is, as no `gh` call does: a file that
    /// is not the one uploaded.
    pub fn put(&self, tag: &str, name: &str, data: impl AsRef<[u8]>) {
        let mut state = self.state();
        let one = state.releases.get_mut(tag).expect("the release is there");
        let (_, asset) = one
            .assets
            .iter_mut()
            .find(|(held, _)| held == name)
            .expect("the asset is there");
        asset.data = data.as_ref().to_vec();
    }

    /// An asset gone where it is, as no `gh` call of the publisher does: a file
    /// somebody else deleted.
    pub fn remove(&self, tag: &str, name: &str) {
        let mut state = self.state();
        let one = state.releases.get_mut(tag).expect("the release is there");
        one.assets.retain(|(held, _)| held != name);
    }

    pub fn has(&self, tag: &str) -> bool {
        self.state().releases.contains_key(tag)
    }

    pub fn is_draft(&self, tag: &str) -> bool {
        self.state().releases[tag].draft
    }

    pub fn is_prerelease(&self, tag: &str) -> bool {
        self.state().releases[tag].prerelease
    }

    pub fn title(&self, tag: &str) -> String {
        self.state().releases[tag].title.clone()
    }

    pub fn notes(&self, tag: &str) -> String {
        self.state().releases[tag].notes.clone()
    }

    /// The names of a release's assets, sorted; none where there is no release.
    pub fn names(&self, tag: &str) -> Option<Vec<String>> {
        let state = self.state();
        let one = state.releases.get(tag)?;
        let mut names: Vec<String> = one.assets.iter().map(|(name, _)| name.clone()).collect();
        names.sort();
        Some(names)
    }

    /// An asset's bytes, if the release holds it.
    pub fn asset(&self, tag: &str, name: &str) -> Option<Vec<u8>> {
        let state = self.state();
        let one = state.releases.get(tag)?;
        let (_, asset) = one.assets.iter().find(|(held, _)| held == name)?;
        Some(asset.data.clone())
    }

    /// An asset's text; the release holds it.
    pub fn asset_text(&self, tag: &str, name: &str) -> String {
        String::from_utf8_lossy(&self.asset(tag, name).expect("the asset is there")).into_owned()
    }

    /// Every call from now on is asked of `failing(args, index)`: a reason it
    /// fails, or nothing.
    pub fn fail(&self, failing: impl FnMut(&[String], usize) -> Option<String> + Send + 'static) {
        self.state().failing = Some(Arc::new(Mutex::new(failing)));
    }

    /// No call fails any more.
    pub fn clear_fail(&self) {
        self.state().failing = None;
    }

    /// `path` serves `served` instead of what the release holds.
    pub fn serve(&self, path: &str, served: impl Into<Served>) {
        self.state()
            .overrides
            .insert(path.to_string(), Override::Always(served.into()));
    }

    /// `path` is asked of `serves` on each request: what it serves then, or
    /// nothing, for the release's file.
    pub fn serve_with(&self, path: &str, serves: impl FnMut() -> Option<Served> + Send + 'static) {
        self.state().overrides.insert(
            path.to_string(),
            Override::Sometimes(Arc::new(Mutex::new(serves))),
        );
    }

    /// `path` serves what the release holds again.
    pub fn unserve(&self, path: &str) {
        self.state().overrides.remove(path);
    }

    /// An HTTP request: a `gh` call from a process of its own, or a download.
    fn answer(&self, request: Request) -> Reply {
        if request.method == "POST" && request.path == "/__gh" {
            let Ok(call) = serde_json::from_slice::<Value>(&request.body) else {
                return respond(400, Vec::new());
            };
            let args: Vec<String> = call["args"]
                .as_array()
                .map(|args| {
                    args.iter()
                        .filter_map(|arg| arg.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let cwd = call["cwd"].as_str().unwrap_or(".").to_string();
            let answer = self.gh(&args, Path::new(&cwd));
            let body =
                json!({"status": answer.status, "stdout": answer.stdout, "stderr": answer.stderr});
            return respond(200, body.to_string().into_bytes());
        }
        let instead = {
            let mut state = self.state();
            state.requests.push(request.path.clone());
            state.overrides.get(&request.path).cloned()
        };
        // Asked with the state free, as `gh`'s closure is.
        let forced = match instead {
            None => None,
            Some(Override::Always(served)) => Some(served),
            Some(Override::Sometimes(serves)) => {
                let mut serves = serves.lock().expect("the serving closure");
                serves()
            }
        };
        if matches!(forced, Some(Served::Reset)) {
            return Reply::Drop;
        }
        let file = request
            .path
            .strip_prefix('/')
            .and_then(|path| path.split_once('/'))
            .filter(|(tag, name)| !tag.is_empty() && !name.is_empty() && !name.contains('/'))
            .and_then(|(tag, name)| {
                let state = self.state();
                let one = state.releases.get(tag).filter(|one| !one.draft)?;
                let (_, asset) = one.assets.iter().find(|(held, _)| held == name)?;
                Some(asset.data.clone())
            });
        match (forced, file) {
            (Some(Served::Status(status)), _) => respond(status, Vec::new()),
            (Some(Served::Redirect(to)), _) => Reply::Respond {
                status: 302,
                headers: vec![("Location".to_string(), to)],
                body: Vec::new(),
            },
            (Some(Served::Body(body)), _) => respond(200, body),
            (Some(Served::Reset), _) => Reply::Drop,
            (None, Some(data)) => respond(200, data),
            (None, None) => respond(404, Vec::new()),
        }
    }
}
