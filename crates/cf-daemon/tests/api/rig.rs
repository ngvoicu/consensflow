//! The API under test: the real routes, in the real server, over a ledger of
//! the player's own, whose clock and session names are the ones Node's calls
//! drew and whose events are heard as they are logged; a roster that is the
//! daemon's own, over a file the player writes, and says which agents it was
//! asked for; the windows' tokens; and a count of the wake-ups the API sent.
//!
//! The screens are not mounted: Node's traces of the agents' API were recorded
//! with none (`ui` was null), and a screen's path is answered by them, not by
//! the API, once they are.

use std::cell::RefCell;
use std::rc::Rc;

use cf_base::js;
use cf_base::refusal::Refusal;
use cf_catalog::{AgentRow, Catalog};
use cf_daemon::api::answer::{Answer, Content, Failure};
use cf_daemon::api::callers::caller_of;
use cf_daemon::api::context::{AgentRows, Context};
use cf_daemon::api::request::Request;
use cf_daemon::api::routes::{dispatch, recognize};
use cf_daemon::api::{Api, Handler};
use cf_daemon::roster::Agents;
use serde_json::{json, Value};

use crate::front::Front;
use crate::ledger;

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
    pub ledger: ledger::Rig,
    pub front: Front,
    asked: Rc<RefCell<Vec<String>>>,
    pub seen: Rc<RefCell<Vec<Seen>>>,
    pub api: Api,
}

impl Rig {
    pub async fn start() -> Self {
        let home = tempfile::tempdir().expect("a home");
        let ledger = ledger::Rig::open(&home.path().join("consensflow.db"));
        let asked = Rc::new(RefCell::new(Vec::new()));
        let roster = Roster {
            agents: Agents::new(
                Catalog::bundled().expect("the catalog"),
                home.path().join("agents.json"),
            ),
            asked: Rc::clone(&asked),
        };
        let front = Front::new(home.path(), Rc::clone(&ledger.ledger), Rc::new(roster));
        let seen = Rc::new(RefCell::new(Vec::<Seen>::new()));
        let (context, heard) = (Rc::clone(&front.context), Rc::clone(&seen));
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
        let closing = front.context.closing.clone();
        let api = Api::start(handler, closing, Rc::clone(&front.spawn))
            .await
            .expect("the API");
        Self {
            home,
            ledger,
            front,
            asked,
            seen,
            api,
        }
    }

    /// How many requests the API has taken.
    pub fn received(&self) -> usize {
        self.seen.borrow().len()
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
