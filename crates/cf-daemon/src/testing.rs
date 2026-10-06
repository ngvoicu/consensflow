//! What the daemon's unit tests stand on: a home in a temporary folder, a
//! real ledger in it with a project, a chief, a worker and a question the
//! worker put to the chief, the windows' tokens, and the context a handler of
//! the agents' API is given over them; the engine's executor as the daemon
//! runs it, driven on the local set a test is in; and the daemon's end of a
//! bridge with the app's end for the test.

use std::cell::{Cell, RefCell};
use std::io;
use std::rc::Rc;

use bytes::Bytes;
use cf_base::refusal::Refusal;
use cf_bridge::local::{Bridge, BridgeBuilder};
use cf_catalog::AgentRow;
use cf_ledger::{
    open_ledger, MessageView, NewChief, NewMember, NewProject, NewQuestion, Options, ProjectView,
};
use cf_proto::bridge::Role;
use futures_util::stream;
use http_body_util::StreamBody;
use hyper::body::Frame;
use hyper::Method;
use tokio::sync::mpsc;

use crate::api::answer::{Answer, Content, Failure};
use crate::api::body::Body;
use crate::api::context::{AgentRows, Closing, Context};
use crate::api::credentials::Credentials;
use crate::api::request::Request;
use crate::errors::Errors;
use crate::files::{Log, Trace};
use crate::host::daemon_bridge;
use crate::seams::DaemonSpawn;

/// Where the engine's work runs in a test: the daemon's own, on a home of its
/// own, with its driver spawned on the local set the caller is in.
pub struct Worked {
    /// The home, which holds the log and the trace until it goes.
    pub home: tempfile::TempDir,
    pub spawn: Rc<DaemonSpawn>,
}

/// The engine's work for a test, run by the daemon's executor and its driver.
/// Called inside the local set the test runs in.
pub fn worked() -> Worked {
    let home = tempfile::tempdir().unwrap();
    let errors = Rc::new(Errors::new(
        Rc::new(Log::new(home.path())),
        Rc::new(Trace::new(home.path())),
    ));
    let spawn = Rc::new(DaemonSpawn::new(errors));
    spawn.drive();
    Worked { home, spawn }
}

/// The daemon's end of a bridge and the app's, over memory: the test is the
/// app (`Role::Host`) and asks what the page asks. Both connections run on
/// the local set the caller is in.
pub fn bridge_pair() -> (Bridge, Bridge) {
    connected(BridgeBuilder::new(Role::Daemon))
}

/// [`bridge_pair`], the daemon's end built as the daemon builds it
/// ([`daemon_bridge`]): the work the frames of a read woke is run to its end
/// before the next read.
pub fn bridge_pair_over(spawn: &Rc<DaemonSpawn>) -> (Bridge, Bridge) {
    connected(daemon_bridge(spawn))
}

fn connected(daemon: BridgeBuilder) -> (Bridge, Bridge) {
    let (daemon_end, app_end) = tokio::io::duplex(256 * 1024);
    let (daemon_input, daemon_output) = tokio::io::split(daemon_end);
    let (app_input, app_output) = tokio::io::split(app_end);
    let (daemon, daemon_connection) = daemon.connect(daemon_input, daemon_output);
    let (app, app_connection) = BridgeBuilder::new(Role::Host).connect(app_input, app_output);
    tokio::task::spawn_local(daemon_connection);
    tokio::task::spawn_local(app_connection);
    (daemon, app)
}

/// A place for what the daemon says on its error output, which the test reads.
#[derive(Clone, Default)]
pub struct Said(Rc<RefCell<Vec<u8>>>);

impl Said {
    /// Everything said so far.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.borrow()).into_owned()
    }
}

impl std::io::Write for Said {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Saved agents none of whose rows are asked for.
pub struct NoRows;

impl AgentRows for NoRows {
    fn row(&self, _agent: &str) -> Result<Option<AgentRow>, Refusal> {
        Ok(None)
    }
}

/// A project of two windows and a question waiting.
pub struct Scene {
    /// The home, which holds the ledger, the log and the trace until the scene goes.
    pub _home: tempfile::TempDir,
    pub context: Rc<Context>,
    /// How many times the dispatcher was woken.
    pub kicks: Rc<Cell<u32>>,
    pub project: ProjectView,
    /// The chief's window's token.
    pub chief: String,
    /// The worker `zeus`'s window's token.
    pub zeus: String,
    /// The question `zeus` asked the chief, which waits for an answer.
    pub question: MessageView,
}

/// A scene with its ledger open on a file of its own.
pub fn scene() -> Scene {
    let home = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&home.path().join("consensflow.db"), Options::default()).unwrap();
    let project = ledger
        .create_project(&NewProject {
            directory: "/work/app".to_owned(),
            name: "app".to_owned(),
            chief: NewChief {
                harness: "claude-code".to_owned(),
                agent: Some("mybuilder".to_owned()),
            },
            staff: Vec::new(),
            gate: false,
        })
        .unwrap();
    ledger
        .add_member(
            project.id,
            &NewMember {
                agent: "zeus".to_owned(),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["worker".to_owned()],
                tier: "standard".to_owned(),
            },
        )
        .unwrap();
    let question = ledger
        .ask(
            project.id,
            &NewQuestion {
                from: Some("zeus".to_owned()),
                to: "chief".to_owned(),
                body: Some("Which?".to_owned()),
                ..NewQuestion::default()
            },
        )
        .unwrap();
    let project = ledger.project(project.id).unwrap().unwrap();
    let credentials = Credentials::new();
    let token = |handle: &str| {
        let participant = project
            .participants
            .iter()
            .find(|p| p.handle == handle)
            .unwrap();
        credentials.issue(project.id, participant.id)
    };
    let (chief, zeus) = (token("chief"), token("zeus"));
    let kicks = Rc::new(Cell::new(0));
    let counted = Rc::clone(&kicks);
    let context = Rc::new(Context {
        ledger: Rc::new(RefCell::new(ledger)),
        credentials: Rc::new(credentials),
        kick: Rc::new(move || counted.set(counted.get() + 1)),
        closing: Closing::new(),
        roster: Rc::new(NoRows),
        log: Rc::new(Log::new(home.path())),
        trace: Rc::new(Trace::new(home.path())),
    });
    Scene {
        _home: home,
        context,
        kicks,
        project,
        chief,
        zeus,
        question,
    }
}

/// What sends the frames of a body [`sent_body`] made: a chunk or a failure at
/// a time, as the test chooses, and the end of the body when it is dropped.
pub struct Sending(mpsc::UnboundedSender<io::Result<Frame<Bytes>>>);

impl Sending {
    /// The next chunk of the body.
    pub fn chunk(&self, bytes: &[u8]) {
        let _ = self.0.send(Ok(Frame::data(Bytes::copy_from_slice(bytes))));
    }

    /// The connection fails, with `words`.
    pub fn fail(&self, words: &str) {
        let _ = self.0.send(Err(io::Error::other(words.to_owned())));
    }
}

/// A body as hyper hands one over, whose frames come when the test sends them:
/// what `Body::pumped` is given where a connection would give it hyper's own,
/// with the schedule in the test's hands.
pub fn sent_body() -> (
    Sending,
    impl hyper::body::Body<Data = Bytes, Error = io::Error> + Unpin + 'static,
) {
    let (sender, mut received) = mpsc::unbounded_channel();
    (
        Sending(sender),
        StreamBody::new(stream::poll_fn(move |context| received.poll_recv(context))),
    )
}

/// A request with `token` as its bearer, and `body` as the text it sends.
pub fn request(method: Method, target: &str, token: Option<&str>, body: &str) -> Request {
    let chunks = if body.is_empty() {
        Vec::new()
    } else {
        vec![Ok(bytes::Bytes::copy_from_slice(body.as_bytes()))]
    };
    Request::new(
        method,
        target,
        token.map(|token| format!("Bearer {token}")),
        Body::new(futures_util::stream::iter(chunks)),
    )
    .unwrap()
}

/// The JSON an answer carries, with its status.
pub fn said(answer: Result<Answer, Failure>) -> (u16, serde_json::Value) {
    let answer = answer.unwrap_or_else(|failure| failure.answer());
    match answer.content {
        Content::Json(body) => (answer.status, body),
        other => panic!("not JSON: {other:?}"),
    }
}
