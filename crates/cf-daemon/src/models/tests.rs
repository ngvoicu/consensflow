use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use cf_engine::seams::Adapters;
use cf_harness::contract::{Adapter, FirstMessage, Launch, Prepared, Skill, Work};
use cf_harness::testing::fake_executable;
use cf_ledger::{open_ledger, Ledger, Options};
use cf_proto::models::{Freshness, ModelChoice, ModelListing};
use cf_proto::specs::Variant;
use tokio::sync::Notify;

use super::*;

/// A harness whose list answers what the test gives it, in order, each once the test
/// lets it (where it holds a gate), and what it was asked.
#[derive(Default)]
struct Listing {
    answers: RefCell<VecDeque<Result<ModelListing, String>>>,
    asked: RefCell<Vec<bool>>,
    gate: Option<Rc<Notify>>,
}

impl Listing {
    fn answering(answers: impl IntoIterator<Item = Result<ModelListing, String>>) -> Rc<Self> {
        Rc::new(Self {
            answers: RefCell::new(answers.into_iter().collect()),
            ..Self::default()
        })
    }
}

impl Adapter for Listing {
    fn prepare<'a>(&'a self, _launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>> {
        unreachable!("a list is read with no window")
    }

    fn first_message(&self) -> FirstMessage {
        unreachable!("a list is read with no window")
    }

    fn specmint(&self, _variant: Variant) -> Result<Skill, String> {
        unreachable!("a list is read with no window")
    }

    fn models(&self, fetch: bool) -> Work<'_, Result<ModelListing, String>> {
        self.asked.borrow_mut().push(fetch);
        let gate = self.gate.clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.notified().await;
            }
            self.answers
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Err("asked once too often".to_owned()))
        })
    }
}

struct ByKind(Vec<(&'static str, Rc<Listing>)>);

impl Adapters for ByKind {
    fn adapter(&self, harness: &str) -> Option<Rc<dyn Adapter>> {
        self.0
            .iter()
            .find(|(kind, _)| *kind == harness)
            .map(|(_, listing)| Rc::clone(listing) as Rc<dyn Adapter>)
    }
}

/// What a read at `at` answers.
fn models(ids: &[&str], at: i64) -> ModelListing {
    ModelListing::read(
        ids.iter().map(|id| ModelChoice::new(*id, *id)).collect(),
        at,
        "a test",
    )
}

/// A home with `installed` on its PATH, and its ledger.
struct Home {
    _folder: tempfile::TempDir,
    env: Env,
    ledger: RefCell<Ledger>,
}

fn home(installed: &[&str]) -> Home {
    let folder = tempfile::tempdir().unwrap();
    let bin = folder.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for name in installed {
        fake_executable(&bin.join(name));
    }
    let env = Env::from_vars([("PATH", bin.as_os_str())]);
    let ledger = open_ledger(&folder.path().join("consensflow.db"), Options::default()).unwrap();
    Home {
        _folder: folder,
        env,
        ledger: RefCell::new(ledger),
    }
}

fn lists(home: &Home, adapters: ByKind) -> ModelLists {
    ModelLists::new(Rc::new(adapters), home.env.clone())
}

/// `work`, done: it waits on nothing the test has not let go.
fn done<T>(work: impl Future<Output = T>) -> T {
    match pin!(work).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("waited"),
    }
}

#[test]
fn a_harness_is_read_at_the_first_ask_of_a_daemon_s_life_then_kept_and_read_again_at_refresh() {
    let home = home(&["devin"]);
    let devin = Listing::answering([Ok(models(&["swe-2"], 1_000)), Ok(models(&["swe-3"], 2_000))]);
    let lists = lists(&home, ByKind(vec![("devin", Rc::clone(&devin))]));

    let first = done(lists.get(&home.ledger, Harness::Devin, false)).unwrap();
    assert_eq!(first.models[0].id, "swe-2");
    assert_eq!((first.read_at, first.is_stale()), (Some(1_000), false));
    let again = done(lists.get(&home.ledger, Harness::Devin, false)).unwrap();
    assert_eq!(again, first, "kept: no second read in this daemon's life");
    assert_eq!(*devin.asked.borrow(), [false]);

    let refreshed = done(lists.get(&home.ledger, Harness::Devin, true)).unwrap();
    assert_eq!(refreshed.models[0].id, "swe-3");
    assert_eq!(refreshed.read_at, Some(2_000));
    assert_eq!(*devin.asked.borrow(), [false, true], "the Refresh fetches");
    assert_eq!(
        home.ledger.borrow().model_listing("devin").unwrap(),
        Some(refreshed)
    );
}

#[test]
fn a_new_daemon_reads_again_at_its_first_ask_and_a_failure_keeps_the_listing_before_stale() {
    let home = home(&["devin"]);
    let devin = Listing::answering([Ok(models(&["swe-2"], 1_000))]);
    done(lists(&home, ByKind(vec![("devin", devin)])).get(&home.ledger, Harness::Devin, false))
        .unwrap();

    let restarted = Listing::answering([Err("Not logged in".to_owned())]);
    let after = lists(&home, ByKind(vec![("devin", Rc::clone(&restarted))]));
    let list = done(after.get(&home.ledger, Harness::Devin, false)).unwrap();
    assert_eq!(*restarted.asked.borrow(), [false]);
    assert_eq!(list.models[0].id, "swe-2", "the list read before");
    assert_eq!(list.read_at, Some(1_000));
    assert_eq!(
        list.freshness,
        Freshness::Stale {
            why: "Not logged in".to_owned()
        }
    );
    assert_eq!(list.default_model(), None);
}

#[test]
fn a_harness_not_installed_here_is_never_asked() {
    let home = home(&[]);
    let claude = Listing::answering([Ok(models(&["opus"], 1_000))]);
    let lists = lists(&home, ByKind(vec![("claude-code", Rc::clone(&claude))]));
    assert_eq!(
        done(lists.get(&home.ledger, Harness::Claude, false)),
        Err("claude is not installed here".to_owned())
    );
    assert!(claude.asked.borrow().is_empty());
    assert_eq!(home.ledger.borrow().model_listing("claude").unwrap(), None);
}

#[test]
fn a_harness_with_no_adapter_is_said_to_have_none() {
    let home = home(&["opencode"]);
    let lists = lists(&home, ByKind(vec![]));
    assert_eq!(
        done(lists.get(&home.ledger, Harness::Opencode, false)),
        Err("opencode has no adapter".to_owned())
    );
}

#[test]
fn two_harnesses_are_read_one_after_the_other() {
    let home = home(&["claude", "devin"]);
    let gate = Rc::new(Notify::new());
    let claude = Rc::new(Listing {
        answers: RefCell::new([Ok(models(&["opus"], 1_000))].into()),
        gate: Some(Rc::clone(&gate)),
        ..Listing::default()
    });
    let devin = Listing::answering([Ok(models(&["swe-2"], 1_000))]);
    let lists = lists(
        &home,
        ByKind(vec![
            ("claude-code", Rc::clone(&claude)),
            ("devin", Rc::clone(&devin)),
        ]),
    );
    let mut context = Context::from_waker(Waker::noop());
    let mut first = pin!(lists.get(&home.ledger, Harness::Claude, false));
    let mut second = pin!(lists.get(&home.ledger, Harness::Devin, false));
    assert!(first.as_mut().poll(&mut context).is_pending());
    assert!(second.as_mut().poll(&mut context).is_pending());
    assert_eq!(*claude.asked.borrow(), [false]);
    assert!(
        devin.asked.borrow().is_empty(),
        "Devin waits for Claude's read"
    );

    gate.notify_one();
    assert!(first.as_mut().poll(&mut context).is_ready());
    let Poll::Ready(devin_list) = second.as_mut().poll(&mut context) else {
        panic!("Devin's read waited past Claude's");
    };
    assert_eq!(devin_list.unwrap().models[0].id, "swe-2");
}
