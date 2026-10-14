//! What the tests share: a ledger at a clock the test moves, which tells its
//! trace every event, and the few things a test does to it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cf_base::time::Clock;
use cf_ledger::{
    open_ledger, Event, Ledger, LedgerError, NewChief, NewProject, Options, ProjectView,
};
use serde_json::Value;

/// 2026-10-13T12:00:00.000Z, where the clock of every ledger here starts.
pub const START: i64 = 1_791_892_800_000;
pub const MINUTE: i64 = 60_000;

/// What the ledger says of a name it refuses.
pub const NAME_REFUSED: &str = "name must be text, not empty, at most 100 characters";

/// The ledger's clock: the cell the test moves.
struct Moving(Rc<Cell<i64>>);

impl Clock for Moving {
    fn now_ms(&mut self) -> i64 {
        self.0.get()
    }
}

/// A ledger at a clock the test moves, and every event it told the trace.
pub struct Home {
    pub ledger: Ledger,
    pub now: Rc<Cell<i64>>,
    pub told: Rc<RefCell<Vec<Event>>>,
    // Last, so the file goes after the ledger.
    _dir: tempfile::TempDir,
}

pub fn home() -> Home {
    let dir = tempfile::tempdir().expect("a home");
    let now = Rc::new(Cell::new(START));
    let told = Rc::new(RefCell::new(Vec::new()));
    let hearing = Rc::clone(&told);
    let ledger = open_ledger(
        &dir.path().join("consensflow.db"),
        Options {
            clock: Box::new(Moving(Rc::clone(&now))),
            trace: Box::new(move |event| hearing.borrow_mut().push(event.clone())),
            ..Options::default()
        },
    )
    .expect("a ledger");
    Home {
        ledger,
        now,
        told,
        _dir: dir,
    }
}

/// A project to open in `directory`, called `name`.
pub fn request(name: &str, directory: &str) -> NewProject {
    NewProject {
        directory: directory.to_owned(),
        name: name.to_owned(),
        chief: NewChief {
            harness: "claude-code".to_owned(),
            agent: Some("apollo".to_owned()),
        },
        staff: Vec::new(),
        gate: false,
    }
}

/// A project called `name`, in `/work/<name>`.
pub fn project(ledger: &mut Ledger, name: &str) -> ProjectView {
    ledger
        .create_project(&request(name, &format!("/work/{name}")))
        .expect("a project")
}

/// The data of the events of `kind` a project logged, oldest first.
pub fn events_of(ledger: &Ledger, project: i64, kind: &str) -> Vec<Value> {
    ledger
        .events(project, 0, 500)
        .expect("the events")
        .into_iter()
        .filter(|event| event.kind == kind)
        .map(|event| event.data)
        .collect()
}

/// What the trace was told of `kind`: the project and the data.
pub fn told_of(home: &Home, kind: &str) -> Vec<(i64, Value)> {
    home.told
        .borrow()
        .iter()
        .filter(|event| event.kind == kind)
        .map(|event| (event.project, event.data.clone()))
        .collect()
}

/// The code and the words of a refusal.
pub fn refusal<T>(said: Result<T, LedgerError>) -> (Option<&'static str>, String) {
    match said {
        Ok(_) => panic!("the ledger took what it should refuse"),
        Err(refused) => (refused.code(), refused.to_string()),
    }
}
