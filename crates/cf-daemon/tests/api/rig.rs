//! The API under test: the real routes, in the real server, over a ledger of
//! the player's own, whose clock and session names are the ones Node's calls
//! drew and whose events are heard as they are logged; a roster that is the
//! daemon's own, over a file the player writes, and says which agents it was
//! asked for; the windows' tokens; and a count of the wake-ups the API sent.
//!
//! The screens are not mounted: Node's traces of the agents' API were recorded
//! with none (`ui` was null), and a screen's path is answered by them, not by
//! the API, once they are.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use cf_base::js;
use cf_base::refusal::Refusal;
use cf_catalog::{AgentRow, Catalog};
use cf_daemon::api::answer::{Answer, Content, Failure};
use cf_daemon::api::callers::caller_of;
use cf_daemon::api::context::{AgentRows, Closing, Context};
use cf_daemon::api::credentials::Credentials;
use cf_daemon::api::request::Request;
use cf_daemon::api::routes::{dispatch, recognize};
use cf_daemon::api::{Api, Handler};
use cf_daemon::errors::Errors;
use cf_daemon::files::{Log, Trace};
use cf_daemon::roster::Agents;
use cf_daemon::seams::DaemonSpawn;
use cf_ledger::{open_ledger, Event, Ledger, Options};
use serde_json::{json, Value};

use crate::dump::dump;
use crate::replay::{Queues, Recorded};

/// What the API answered a request, as the server would say it.
#[derive(Debug, Clone, PartialEq)]
pub struct Answered {
    pub status: u16,
    pub content_type: Option<&'static str>,
    pub body: String,
}

impl Answered {
    fn of(answer: &Result<Answer, Failure>) -> Self {
        let answer = match answer {
            Ok(answer) => answer.clone(),
            Err(failure) => failure.answer(),
        };
        let (content_type, body) = match &answer.content {
            Content::Json(value) => (Some("application/json"), js::stringify(value)),
            Content::Html(page) => (Some("text/html; charset=utf-8"), page.clone()),
            Content::Nothing => (None, String::new()),
        };
        Self {
            status: answer.status,
            content_type,
            body,
        }
    }
}

/// A request the API took, in the order it came: what it was, and what it
/// was answered once it was.
#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub bearer: Option<String>,
    pub answered: Option<Answered>,
}

/// The saved agents the API reads, which it is asked for by name.
struct Roster {
    agents: Agents,
    asked: Rc<RefCell<Vec<String>>>,
}

impl AgentRows for Roster {
    fn row(&self, agent: &str) -> Result<Option<AgentRow>, Refusal> {
        self.asked.borrow_mut().push(agent.to_owned());
        self.agents.row(agent)
    }
}

/// The API, listening, and what a player reaches of it.
pub struct Rig {
    home: tempfile::TempDir,
    pub ledger: Rc<RefCell<Ledger>>,
    pub credentials: Rc<Credentials>,
    pub queues: Rc<Queues>,
    events: Rc<RefCell<Vec<Event>>>,
    kicks: Rc<Cell<usize>>,
    kicked: Cell<usize>,
    asked: Rc<RefCell<Vec<String>>>,
    pub seen: Rc<RefCell<Vec<Seen>>>,
    pub api: Api,
}

impl Rig {
    pub async fn start() -> Self {
        let home = tempfile::tempdir().expect("a home");
        let queues = Rc::new(Queues::new());
        let events = Rc::new(RefCell::new(Vec::new()));
        let told = Rc::clone(&events);
        let drawn = queues.names.share();
        let ledger = open_ledger(
            &home.path().join("consensflow.db"),
            Options {
                clock: Box::new(Recorded(queues.readings.share())),
                names: Box::new(move || drawn.draw().unwrap_or_default()),
                trace: Box::new(move |event| told.borrow_mut().push(event.clone())),
            },
        )
        .expect("a ledger");
        let ledger = Rc::new(RefCell::new(ledger));
        let credentials = Rc::new(Credentials::new());
        let kicks = Rc::new(Cell::new(0));
        let counted = Rc::clone(&kicks);
        let asked = Rc::new(RefCell::new(Vec::new()));
        let roster = Roster {
            agents: Agents::new(
                Catalog::bundled().expect("the catalog"),
                home.path().join("agents.json"),
            ),
            asked: Rc::clone(&asked),
        };
        let log = Rc::new(Log::new(home.path()));
        let trace = Rc::new(Trace::new(home.path()));
        let closing = Closing::new();
        let context = Rc::new(Context {
            ledger: Rc::clone(&ledger),
            credentials: Rc::clone(&credentials),
            kick: Rc::new(move || counted.set(counted.get() + 1)),
            closing: closing.clone(),
            roster: Rc::new(roster),
            log: Rc::clone(&log),
            trace: Rc::clone(&trace),
        });
        let spawn = Rc::new(DaemonSpawn::new(Rc::new(Errors::new(log, trace))));
        spawn.drive();
        let seen = Rc::new(RefCell::new(Vec::<Seen>::new()));
        let heard = Rc::clone(&seen);
        let handler: Rc<Handler> = Rc::new(move |request| {
            let (context, seen) = (Rc::clone(&context), Rc::clone(&heard));
            let at = {
                let mut heard = seen.borrow_mut();
                heard.push(Seen {
                    method: request.method.to_string(),
                    path: request.path.clone(),
                    bearer: request.bearer().map(str::to_owned),
                    answered: None,
                });
                heard.len() - 1
            };
            Box::pin(async move {
                let answer = through(&context, request).await;
                seen.borrow_mut()[at].answered = Some(Answered::of(&answer));
                answer
            })
        });
        let api = Api::start(handler, closing, spawn).await.expect("the API");
        Self {
            home,
            ledger,
            credentials,
            queues,
            events,
            kicks,
            kicked: Cell::new(0),
            asked,
            seen,
            api,
        }
    }

    /// The ledger's file, which a trace names «ledger».
    pub fn ledger_file(&self) -> PathBuf {
        self.home.path().join("consensflow.db")
    }

    /// `127.0.0.1:<port>`, which a trace names «api».
    pub fn address(&self) -> String {
        self.api
            .url()
            .strip_prefix("http://")
            .expect("an address")
            .to_owned()
    }

    /// How many requests the API has taken.
    pub fn received(&self) -> usize {
        self.seen.borrow().len()
    }

    /// The events the ledger logged since they were last taken, as the
    /// recorder wrote them.
    pub fn take_events(&self) -> Vec<Value> {
        self.events
            .borrow_mut()
            .drain(..)
            .map(|event| crate::replay::event_json(&event))
            .collect()
    }

    /// How many times the API woke the dispatcher since it was last asked.
    pub fn take_kicks(&self) -> usize {
        let total = self.kicks.get();
        total - self.kicked.replace(total)
    }

    /// The agents the API read a row of since it was last asked, in order.
    pub fn take_asked(&self) -> Vec<String> {
        self.asked.borrow_mut().drain(..).collect()
    }

    /// The saved agents, as the rows say (the answers of Node's roster
    /// stand-in): what the API reads of them.
    pub fn save_agents(&self, rows: &[Value]) {
        let file = json!({ "schemaVersion": 1, "agents": rows });
        std::fs::write(self.home.path().join("agents.json"), file.to_string())
            .expect("the agents file");
    }
}

/// The API's own checks and routes, in the order `api::handle` has them,
/// without the screens before them.
async fn through(context: &Context, request: Request) -> Result<Answer, Failure> {
    let caller = caller_of(context, &request)?;
    match recognize(&request.method, &request.path) {
        Some(route) => dispatch(context, &caller, route, request).await,
        None => Err(request.unknown_route()),
    }
}

/// The ledger closed where it is, and the database it left.
pub fn closed(rig: &Rig) -> Value {
    rig.ledger
        .borrow_mut()
        .close_in_place()
        .expect("the ledger closes");
    dump(&rig.ledger_file())
}
