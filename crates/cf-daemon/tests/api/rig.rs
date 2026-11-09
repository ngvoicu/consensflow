//! The API under test: the daemon's own dispatch (`api::handle`: the screens
//! first, then the window's token, then the routes), in the real server, over a
//! ledger of the player's own, whose clock and session names are the ones
//! Node's calls drew and whose events are heard as they are logged; the windows'
//! tokens; and a count of the wake-ups the API sent. Node's API read the saved
//! agents (its roster stand-in); this one reads none: a member holds its own model
//! (harness-models).
//!
//! The screens are mounted, as the daemon mounts them: inert, over a home of
//! their own, with a UI token no trace uses. Node's traces of the agents' API
//! were recorded with none (`ui` was null), so a path of the screens' is
//! answered by them here and by the API's routes there; that is the one
//! exchange of the traces that is theirs (`player::DEPARTURES`).

use std::cell::RefCell;
use std::rc::Rc;

use cf_base::env::Env;
use cf_daemon::api::{handle, Api, Handler};

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

/// The API, listening, and what a player reaches of it.
pub struct Rig {
    pub ledger: ledger::Rig,
    pub front: Front,
    pub seen: Rc<RefCell<Vec<Seen>>>,
    pub api: Api,
    _home: tempfile::TempDir,
}

impl Rig {
    pub async fn start() -> Self {
        let home = tempfile::tempdir().expect("a home");
        let ledger = ledger::Rig::open(&home.path().join("consensflow.db"));
        let front = Front::new(home.path(), Rc::clone(&ledger.ledger));
        // Mounted as the daemon mounts them, over a home that is none of the API's.
        let screens = screens(UI_TOKEN, Env::default());
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
            _home: home,
            ledger,
            front,
            seen,
            api,
        }
    }

    /// How many requests the API has taken.
    pub fn received(&self) -> usize {
        self.seen.borrow().len()
    }
}
