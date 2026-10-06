//! The API under test: the daemon's own dispatch (`api::handle`: the screens
//! first, then the window's token, then the routes), in the real server, over a
//! ledger of the player's own, whose clock and session names are the ones
//! Node's calls drew and whose events are heard as they are logged; a roster
//! that is the daemon's own, over a file the player writes, and says which
//! agents it was asked for; the windows' tokens; and a count of the wake-ups
//! the API sent.
//!
//! The screens are mounted, as the daemon mounts them: inert, over a home of
//! their own, with a UI token no trace uses. Node's traces of the agents' API
//! were recorded with none (`ui` was null), so a path of the screens' is
//! answered by them here and by the API's routes there; that is the one
//! exchange of the traces that is theirs (`player::DEPARTURES`).

use std::cell::RefCell;
use std::rc::Rc;

use cf_base::env::Env;
use cf_base::refusal::Refusal;
use cf_catalog::{AgentRow, Catalog};
use cf_daemon::api::context::AgentRows;
use cf_daemon::api::{handle, Api, Handler};
use cf_daemon::roster::Agents;
use serde_json::{json, Value};

use crate::front::{screens, Front};
use crate::ledger;

/// The UI token of the screens the API is mounted under. No trace of the API
/// carries it (`tests/api/main.rs` holds that), so no request opens them.
pub const UI_TOKEN: &str = "the-ui-token-no-trace-of-the-api-carries";

/// A request the API took, in the order it came: what it was, as its handler
/// was given it. (How it was answered is the bytes the answer was written as,
/// which only a client of the API has: a run of `cf` has it from the relay.)
#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub bearer: Option<String>,
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
        // Mounted as the daemon mounts them, over a home that is none of the API's.
        let screens = screens(
            UI_TOKEN,
            Env::default(),
            Rc::new(Agents::new(
                Catalog::bundled().expect("the catalog"),
                home.path().join("screens").join("agents.json"),
            )),
            Rc::new(|| Ok(())),
        );
        let seen = Rc::new(RefCell::new(Vec::<Seen>::new()));
        let (context, heard) = (Rc::clone(&front.context), Rc::clone(&seen));
        let handler: Rc<Handler> = Rc::new(move |request| {
            let (context, screens) = (Rc::clone(&context), Rc::clone(&screens));
            heard.borrow_mut().push(Seen {
                method: request.method.to_string(),
                path: request.path.clone(),
                bearer: request.bearer().map(str::to_owned),
            });
            Box::pin(async move { handle(&context, &screens, request).await })
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
