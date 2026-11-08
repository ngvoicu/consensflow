use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use cf_engine::seams::Adapters;
use cf_harness::contract::{Adapter, FirstMessage, Launch, LaunchId, Prepared, Skill, Specs, Work};
use cf_harness::testing::fake_executable;
use cf_proto::models::{Freshness, ModelChoice, ModelListing};
use cf_proto::specs::Variant;
use tokio::sync::Notify;

use super::*;

/// A harness whose list answers what the test gives it, in order, each once the test
/// lets it (where it holds a gate), and what it was asked.
#[derive(Default)]
pub(crate) struct Listing {
    answers: RefCell<VecDeque<Result<ModelListing, String>>>,
    /// What every ask answers once `answers` is used up, where a test gives one.
    always: RefCell<Option<ModelListing>>,
    /// Why every ask fails, where a test has the harness fail.
    failing: RefCell<Option<String>>,
    pub(crate) asked: RefCell<Vec<bool>>,
    gate: Option<Rc<Notify>>,
    /// A gate that holds only a Refresh (a read that fetches).
    refresh_gate: Option<Rc<Notify>>,
}

impl Listing {
    pub(crate) fn answering(
        answers: impl IntoIterator<Item = Result<ModelListing, String>>,
    ) -> Rc<Self> {
        Rc::new(Self {
            answers: RefCell::new(answers.into_iter().collect()),
            ..Self::default()
        })
    }

    /// A harness that lists `listing` at every ask, until a test sets another ([`Listing::set`]).
    pub(crate) fn repeating(listing: ModelListing) -> Rc<Self> {
        Rc::new(Self {
            always: RefCell::new(Some(listing)),
            ..Self::default()
        })
    }

    /// What every ask answers from now on.
    pub(crate) fn set(&self, listing: ModelListing) {
        self.answers.borrow_mut().clear();
        *self.failing.borrow_mut() = None;
        *self.always.borrow_mut() = Some(listing);
    }

    /// Every ask fails with `why` from now on, until a test sets a listing.
    pub(crate) fn fail(&self, why: &str) {
        *self.failing.borrow_mut() = Some(why.to_owned());
    }
}

impl Adapter for Listing {
    fn prepare<'a>(&'a self, _launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>> {
        unreachable!("a list is read with no window")
    }

    fn first_message(&self) -> FirstMessage {
        unreachable!("a list is read with no window")
    }

    fn specmint(&self, _variant: Variant, _folder: &str) -> Result<Skill, String> {
        unreachable!("a list is read with no window")
    }

    fn place_specmint(&self, _specs: &Specs<'_>, _id: &LaunchId) -> Result<(), String> {
        unreachable!("a list is read with no window")
    }

    fn models(&self, fetch: bool) -> Work<'_, Result<ModelListing, String>> {
        self.asked.borrow_mut().push(fetch);
        let gate = self
            .gate
            .clone()
            .or_else(|| self.refresh_gate.clone().filter(|_| fetch));
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.notified().await;
            }
            if let Some(why) = self.failing.borrow().clone() {
                return Err(why);
            }
            let answer = self.answers.borrow_mut().pop_front();
            answer
                .or_else(|| self.always.borrow().clone().map(Ok))
                .unwrap_or_else(|| Err("asked once too often".to_owned()))
        })
    }
}

pub(crate) struct ByKind(pub(crate) Vec<(&'static str, Rc<Listing>)>);

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

/// A home with `installed` on its PATH.
struct Home {
    _folder: tempfile::TempDir,
    env: Env,
}

fn home(installed: &[&str]) -> Home {
    let folder = tempfile::tempdir().unwrap();
    let bin = folder.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for name in installed {
        fake_executable(&bin.join(name));
    }
    let env = Env::from_vars([("PATH", bin.as_os_str())]);
    Home {
        _folder: folder,
        env,
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

/// `work`, done, for a read that has Pi's folder read on a thread of its own (the real Pi
/// adapter's, when Pi lists no model): polled until it is, for up to ten seconds of real time.
fn eventually<T>(work: impl Future<Output = T>) -> T {
    let mut work = pin!(work);
    let began = std::time::Instant::now();
    loop {
        if let Poll::Ready(value) = work.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            return value;
        }
        assert!(
            began.elapsed() < std::time::Duration::from_secs(10),
            "waited"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// `work`, done, for a read of the real Pi adapter, which waits on the clock for Pi's providers
/// and, where Pi lists none, on a thread of its own that reads Pi's folder: polled until it is
/// done; the clock is fired only where nothing real moved for a while, so that a wait on it
/// never cuts the folder's read short.
fn eventually_clocked<T>(
    work: impl Future<Output = T>,
    time: &cf_harness::testing::ManualTime,
) -> T {
    let mut work = pin!(work);
    let began = std::time::Instant::now();
    loop {
        for _ in 0..25 {
            if let Poll::Ready(value) = work.as_mut().poll(&mut Context::from_waker(Waker::noop()))
            {
                return value;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            began.elapsed() < std::time::Duration::from_secs(20) && time.fire_next(i64::MAX),
            "the read waits on nothing"
        );
    }
}

/// A model list is read when it is asked for, whenever that is (A4-28, T-1321): the page
/// opens a dialog and asks, and what the harness offers at that moment is what it is shown.
#[test]
fn every_ask_reads_the_harness_and_the_refresh_fetches() {
    let home = home(&["devin"]);
    let devin = Listing::answering([
        Ok(models(&["swe-2"], 1_000)),
        Ok(models(&["swe-3"], 2_000)),
        Ok(models(&["swe-4"], 3_000)),
    ]);
    let lists = lists(&home, ByKind(vec![("devin", Rc::clone(&devin))]));

    let first = done(lists.read(Harness::Devin, false)).unwrap();
    assert_eq!(first.listing.models[0].id, "swe-2");
    assert_eq!(
        (first.listing.read_at, first.listing.is_stale()),
        (Some(1_000), false)
    );
    let again = done(lists.read(Harness::Devin, false)).unwrap();
    assert_eq!(
        again.listing.models[0].id, "swe-3",
        "what the harness lists now, not the first read"
    );
    assert_ne!(again.id, first.id, "each read has an id of its own");
    assert_eq!(*devin.asked.borrow(), [false, false]);

    let refreshed = done(lists.read(Harness::Devin, true)).unwrap();
    assert_eq!(refreshed.listing.models[0].id, "swe-4");
    assert_eq!(refreshed.listing.read_at, Some(3_000));
    assert_eq!(
        *devin.asked.borrow(),
        [false, false, true],
        "the Refresh fetches"
    );
    // Each read is kept by its id, for a submission made from the list it showed.
    for read in [&first, &again, &refreshed] {
        assert_eq!(
            lists.shown(Harness::Devin, read.id).as_ref(),
            Some(&read.listing)
        );
    }
}

/// A harness whose list cannot be read has no list to check a pick against: refused, with why,
/// unless the environment has the daemon take it on the harness's word (the rigs', whose stand-in
/// harnesses list nothing); where the list can be read it is checked either way.
#[test]
fn a_pick_on_a_harness_that_cannot_be_read_is_refused_unless_the_environment_takes_it_on_its_word()
{
    for (taken, expected) in [(false, Err("Not logged in".to_owned())), (true, Ok(None))] {
        let folder = home(&["devin"]);
        let env = if taken {
            folder.env.clone().with_var(UNREAD_MODELS_VARIABLE, "1")
        } else {
            folder.env.clone()
        };
        let devin = Listing::answering([
            Err("Not logged in".to_owned()),
            Err("Not logged in".to_owned()),
        ]);
        let lists = ModelLists::new(Rc::new(ByKind(vec![("devin", Rc::clone(&devin))])), env);
        assert_eq!(
            done(lists.checked(Harness::Devin, None)),
            expected,
            "taken: {taken}"
        );
    }
    // Where the list can be read it is the list, whatever the variable says.
    let folder = home(&["devin"]);
    let env = folder.env.clone().with_var(UNREAD_MODELS_VARIABLE, "1");
    let devin = Listing::answering([Ok(models(&["swe-2"], 1_000))]);
    let lists = ModelLists::new(Rc::new(ByKind(vec![("devin", devin)])), env);
    let listing = done(lists.checked(Harness::Devin, None)).unwrap().unwrap();
    assert_eq!(listing.models[0].id, "swe-2");
}

#[test]
fn the_variable_that_has_a_daemon_take_unread_models_on_their_word_is_named_as_the_rig_names_it() {
    assert_eq!(
        UNREAD_MODELS_VARIABLE,
        "CONSENSFLOW_UNREAD_MODELS_ARE_TAKEN"
    );
    let env = Env::from_vars([("PATH", "/bin")]);
    assert!(!unread_models_are_taken(&env));
    assert!(!unread_models_are_taken(
        &env.clone().with_var(UNREAD_MODELS_VARIABLE, "0")
    ));
    assert!(!unread_models_are_taken(
        &env.clone().with_var(UNREAD_MODELS_VARIABLE, "")
    ));
    assert!(unread_models_are_taken(
        &env.with_var(UNREAD_MODELS_VARIABLE, "1")
    ));
}

/// The reads a submission can be checked against are the last of each harness, by id, and an id
/// names a read of its own harness only.
#[test]
fn only_the_last_reads_of_a_harness_are_kept_for_a_submission() {
    let home = home(&["devin"]);
    let lists = lists(&home, ByKind(vec![]));
    let ids: Vec<u64> = (0..=KEPT_READS)
        .map(|n| lists.record(Harness::Devin, models(&["a"], i64::try_from(n).unwrap())))
        .collect();
    assert!(
        lists.shown(Harness::Devin, ids[0]).is_none(),
        "the oldest went"
    );
    assert!(lists.shown(Harness::Devin, ids[1]).is_some());
    assert!(lists.shown(Harness::Devin, *ids.last().unwrap()).is_some());
    assert!(
        lists.shown(Harness::Pi, ids[1]).is_none(),
        "a read of Devin's is none of Pi's"
    );
}

/// A read that fails lists no model and says why: the list read before is not put in its
/// place, stale or not. What the form already holds stays the page's to show.
#[test]
fn a_failed_read_lists_no_model_and_says_why_not_the_list_before() {
    let home = home(&["devin"]);
    let devin = Listing::answering([
        Ok(models(&["swe-2"], 1_000)),
        Err("Not logged in".to_owned()),
    ]);
    let lists = lists(&home, ByKind(vec![("devin", Rc::clone(&devin))]));
    let good = done(lists.read(Harness::Devin, false)).unwrap();

    let failed = done(lists.read(Harness::Devin, false)).unwrap();
    let list = &failed.listing;
    assert!(list.models.is_empty(), "no old list in its place: {list:?}");
    assert_eq!(list.read_at, None);
    assert_eq!(
        list.freshness,
        Freshness::Stale {
            why: "Not logged in".to_owned()
        }
    );
    assert_eq!(list.default_model(), None);
    assert_eq!(
        lists.shown(Harness::Devin, failed.id).as_ref(),
        Some(list),
        "the failed read is kept as it was"
    );
    assert_eq!(
        lists.shown(Harness::Devin, good.id).as_ref(),
        Some(&good.listing),
        "and a submission made from the list before it is still checked against that list"
    );
}

#[test]
fn a_harness_not_installed_here_is_never_asked() {
    let home = home(&[]);
    let claude = Listing::answering([Ok(models(&["opus"], 1_000))]);
    let lists = lists(&home, ByKind(vec![("claude-code", Rc::clone(&claude))]));
    assert_eq!(
        done(lists.get(Harness::Claude, false)),
        Err("claude is not installed here".to_owned())
    );
    assert!(claude.asked.borrow().is_empty());
}

#[test]
fn a_harness_with_no_adapter_is_said_to_have_none() {
    let home = home(&["devin"]);
    let lists = lists(&home, ByKind(vec![]));
    assert_eq!(
        done(lists.get(Harness::Devin, false)),
        Err("devin has no adapter".to_owned())
    );
}

#[test]
fn two_reads_of_one_harness_are_one_after_the_other() {
    let home = home(&["devin"]);
    let gate = Rc::new(Notify::new());
    let devin = Rc::new(Listing {
        answers: RefCell::new(
            [Ok(models(&["swe-2"], 1_000)), Ok(models(&["swe-3"], 2_000))].into(),
        ),
        gate: Some(Rc::clone(&gate)),
        ..Listing::default()
    });
    let lists = lists(&home, ByKind(vec![("devin", Rc::clone(&devin))]));
    let mut context = Context::from_waker(Waker::noop());
    let mut first = pin!(lists.get(Harness::Devin, true));
    let mut second = pin!(lists.get(Harness::Devin, true));
    assert!(first.as_mut().poll(&mut context).is_pending());
    assert!(second.as_mut().poll(&mut context).is_pending());
    assert_eq!(
        *devin.asked.borrow(),
        [true],
        "two probes of one harness at once have hung: the second waits for the first"
    );
    gate.notify_one();
    assert!(first.as_mut().poll(&mut context).is_ready());
    gate.notify_one();
    let Poll::Ready(second) = second.as_mut().poll(&mut context) else {
        panic!("the second read waited past the first");
    };
    assert_eq!(second.unwrap().models[0].id, "swe-3");
    assert_eq!(*devin.asked.borrow(), [true, true]);
}

/// A read serves only the asks that came before it started (A4-28, T-1321, review blocker 2):
/// a dialog that opens while another's read of its harness is on its way is not given that
/// read, which began before it opened, but a read begun for it.
#[test]
fn an_ask_that_arrives_after_a_read_started_waits_for_a_new_read() {
    let home = home(&["devin"]);
    let gate = Rc::new(Notify::new());
    let devin = Rc::new(Listing {
        answers: RefCell::new(
            [Ok(models(&["swe-2"], 1_000)), Ok(models(&["swe-3"], 2_000))].into(),
        ),
        gate: Some(Rc::clone(&gate)),
        ..Listing::default()
    });
    let lists = lists(&home, ByKind(vec![("devin", Rc::clone(&devin))]));
    let mut context = Context::from_waker(Waker::noop());
    let mut first = pin!(lists.get(Harness::Devin, false));
    let mut second = pin!(lists.get(Harness::Devin, false));
    assert!(first.as_mut().poll(&mut context).is_pending());
    assert_eq!(
        *devin.asked.borrow(),
        [false],
        "the first read is on its way"
    );
    assert!(second.as_mut().poll(&mut context).is_pending());
    gate.notify_one();
    let Poll::Ready(read) = first.as_mut().poll(&mut context) else {
        panic!("the read did not finish");
    };
    assert_eq!(read.unwrap().models[0].id, "swe-2");
    assert!(
        second.as_mut().poll(&mut context).is_pending(),
        "the ask that came after the first read began waits for a read of its own"
    );
    assert_eq!(
        *devin.asked.borrow(),
        [false, false],
        "a second read begins for it"
    );
    gate.notify_one();
    let Poll::Ready(waited) = second.as_mut().poll(&mut context) else {
        panic!("the second read did not finish");
    };
    assert_eq!(
        waited.unwrap().models[0].id,
        "swe-3",
        "not the read before it"
    );
}

/// Asks that are already waiting when a read begins share it: one read of a harness at a
/// time, and no more of them than the asks need.
#[test]
fn asks_already_waiting_when_a_read_begins_share_it() {
    let home = home(&["devin"]);
    let gate = Rc::new(Notify::new());
    let devin = Rc::new(Listing {
        answers: RefCell::new(
            [
                Ok(models(&["swe-2"], 1_000)),
                Ok(models(&["swe-3"], 2_000)),
                Ok(models(&["swe-4"], 3_000)),
            ]
            .into(),
        ),
        gate: Some(Rc::clone(&gate)),
        ..Listing::default()
    });
    let lists = lists(&home, ByKind(vec![("devin", Rc::clone(&devin))]));
    let mut context = Context::from_waker(Waker::noop());
    let mut first = pin!(lists.get(Harness::Devin, false));
    let mut second = pin!(lists.get(Harness::Devin, false));
    let mut third = pin!(lists.get(Harness::Devin, false));
    assert!(first.as_mut().poll(&mut context).is_pending());
    assert!(second.as_mut().poll(&mut context).is_pending());
    assert!(third.as_mut().poll(&mut context).is_pending());
    gate.notify_one();
    assert!(first.as_mut().poll(&mut context).is_ready());
    assert!(
        second.as_mut().poll(&mut context).is_pending(),
        "its read begins"
    );
    gate.notify_one();
    let Poll::Ready(second_got) = second.as_mut().poll(&mut context) else {
        panic!("the second read did not finish");
    };
    let Poll::Ready(third_got) = third.as_mut().poll(&mut context) else {
        panic!("the ask that was waiting when the second read began did not take it");
    };
    assert_eq!(second_got.unwrap().models[0].id, "swe-3");
    assert_eq!(
        third_got.unwrap().models[0].id,
        "swe-3",
        "the read that began while it waited"
    );
    assert_eq!(
        *devin.asked.borrow(),
        [false, false],
        "two reads for three asks"
    );
}

/// A Refresh reads for itself, however the reads of its harness were queued: one that waited
/// behind a read which began after it came is not given that read, which fetches nothing.
#[test]
fn a_refresh_that_waited_behind_a_read_begun_after_it_came_still_reads_and_fetches_for_itself() {
    let home = home(&["devin"]);
    let gate = Rc::new(Notify::new());
    let devin = Rc::new(Listing {
        answers: RefCell::new(
            [
                Ok(models(&["swe-2"], 1_000)),
                Ok(models(&["swe-3"], 2_000)),
                Ok(models(&["swe-4"], 3_000)),
            ]
            .into(),
        ),
        gate: Some(Rc::clone(&gate)),
        ..Listing::default()
    });
    let lists = lists(&home, ByKind(vec![("devin", Rc::clone(&devin))]));
    let mut context = Context::from_waker(Waker::noop());
    let mut first = pin!(lists.get(Harness::Devin, false));
    let mut waiting = pin!(lists.get(Harness::Devin, false));
    let mut refresh = pin!(lists.get(Harness::Devin, true));
    assert!(first.as_mut().poll(&mut context).is_pending());
    assert!(waiting.as_mut().poll(&mut context).is_pending());
    assert!(refresh.as_mut().poll(&mut context).is_pending());
    gate.notify_one();
    assert!(first.as_mut().poll(&mut context).is_ready());
    // The ask before the Refresh begins the second read, which began after the Refresh came.
    assert!(waiting.as_mut().poll(&mut context).is_pending());
    gate.notify_one();
    assert!(waiting.as_mut().poll(&mut context).is_ready());
    // The Refresh does not take it: it reads, and fetches, for itself.
    assert!(refresh.as_mut().poll(&mut context).is_pending());
    gate.notify_one();
    let Poll::Ready(refreshed) = refresh.as_mut().poll(&mut context) else {
        panic!("the Refresh did not finish");
    };
    assert_eq!(refreshed.unwrap().models[0].id, "swe-4");
    assert_eq!(*devin.asked.borrow(), [false, false, true]);
}

/// A Refresh of Pi is slow (its update, then its read: the page waits on it up to its bridge's
/// 30 s), and another harness's read does not wait for it: each harness has its own turn (the
/// review T-1270, F8: a Claude listing timed out behind a Pi Refresh).
#[test]
fn a_harness_is_read_while_another_harness_is_being_refreshed() {
    let home = home(&["claude", "pi"]);
    let gate = Rc::new(Notify::new());
    let claude = Listing::answering([Ok(models(&["opus"], 1_000)), Ok(models(&["opus-2"], 2_000))]);
    let pi = Rc::new(Listing {
        answers: RefCell::new([Ok(models(&["a/b"], 1_000))].into()),
        refresh_gate: Some(Rc::clone(&gate)),
        ..Listing::default()
    });
    let lists = lists(
        &home,
        ByKind(vec![
            ("claude-code", Rc::clone(&claude)),
            ("pi", Rc::clone(&pi)),
        ]),
    );

    let mut context = Context::from_waker(Waker::noop());
    let mut refresh = pin!(lists.get(Harness::Pi, true));
    assert!(refresh.as_mut().poll(&mut context).is_pending());
    assert_eq!(*pi.asked.borrow(), [true], "Pi's Refresh is in flight");

    let mut claude_asked = pin!(lists.get(Harness::Claude, false));
    let Poll::Ready(read) = claude_asked.as_mut().poll(&mut context) else {
        panic!("a Claude read waited behind a Pi Refresh");
    };
    assert_eq!(read.unwrap().models[0].id, "opus");
    let mut again = pin!(lists.get(Harness::Claude, false));
    let Poll::Ready(read) = again.as_mut().poll(&mut context) else {
        panic!("a second Claude read waited behind a Pi Refresh");
    };
    assert_eq!(read.unwrap().models[0].id, "opus-2", "read again, not kept");

    gate.notify_one();
    let Poll::Ready(refreshed) = refresh.as_mut().poll(&mut context) else {
        panic!("the Refresh did not finish once Pi answered");
    };
    assert_eq!(refreshed.unwrap().models[0].id, "a/b");
}

/// A harness this daemon has not read yet is read while another's Refresh runs: each harness
/// has its own turn.
#[test]
fn a_harness_not_read_yet_is_read_while_another_harness_is_being_read() {
    let home = home(&["devin", "pi"]);
    let gate = Rc::new(Notify::new());
    let pi = Rc::new(Listing {
        answers: RefCell::new([Ok(models(&["a/b"], 1_000))].into()),
        gate: Some(Rc::clone(&gate)),
        ..Listing::default()
    });
    let devin = Listing::answering([Ok(models(&["swe-2"], 1_000))]);
    let lists = lists(
        &home,
        ByKind(vec![("pi", Rc::clone(&pi)), ("devin", Rc::clone(&devin))]),
    );
    let mut context = Context::from_waker(Waker::noop());
    let mut slow = pin!(lists.get(Harness::Pi, false));
    assert!(slow.as_mut().poll(&mut context).is_pending());
    let mut other = pin!(lists.get(Harness::Devin, false));
    let Poll::Ready(read) = other.as_mut().poll(&mut context) else {
        panic!("Devin's first read waited for Pi's");
    };
    assert_eq!(read.unwrap().models[0].id, "swe-2");
    gate.notify_one();
    assert!(slow.as_mut().poll(&mut context).is_ready());
}

/// What Pi's process says can carry a credential (T-1270): an extension's error that echoes the
/// header it was given, a git URL with a token in it. **The words of Pi's process are screened
/// whole, before any cut** (the third reading, F2): if anything in what was kept of them may hold
/// a credential, none of them is quoted, whatever line it is on and however a value runs over
/// lines. The failure's own description, its exit code and the names of what Pi was loading stay.
/// None reaches the ledger, which the page reads, or the caller.
const BEARER: &str = "SYNTHETICSTDERRTOKEN0123456789";
const URL_TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB";
const WITHHELD: &str = "(Pi's words withheld: they may hold a credential)";
/// What a read of Pi that ended says before the words of Pi.
const ENDED: &str = "pi --mode rpc ended before it answered (exit 1); it said:\n";

fn assert_withheld(words: &str, hidden: &[&str]) {
    for leaked in hidden {
        assert!(!words.contains(leaked), "{leaked} in {words}");
    }
    assert!(
        words.contains(WITHHELD),
        "Pi's words were withheld: {words}"
    );
}

/// What a read of `harness` that failed with `failure` is returned as, by the daemon, and what
/// is kept of it for a submission.
fn failed_on(harness: Harness, failure: &str) -> (String, String) {
    let home = home(&[harness.as_str()]);
    let adapter = Listing::answering([Err(failure.to_owned())]);
    let lists = lists(&home, ByKind(vec![(harness.kind(), adapter)]));
    let read = done(lists.read(harness, false)).unwrap();
    let Freshness::Stale { why } = &read.listing.freshness else {
        panic!("a failed read is stale: {:?}", read.listing);
    };
    let kept = lists.shown(harness, read.id).unwrap();
    let Freshness::Stale { why: kept } = &kept.freshness else {
        panic!("{kept:?}");
    };
    (why.clone(), kept.clone())
}

fn failed_with(failure: &str) -> (String, String) {
    failed_on(Harness::Pi, failure)
}

/// The harmless failure words of the other harnesses that the third reading found withheld
/// (F9): what tells the user to log in or what to do about billing. They read as they did in
/// alpha.2, byte for byte, in what the daemon returns and in what it keeps.
const ELSEWHERE: &str = include_str!("tests/other-harnesses-words.json");

#[test]
fn the_failure_words_of_the_other_harnesses_read_as_they_did_and_tell_the_user_what_to_do() {
    let words: Vec<serde_json::Value> = serde_json::from_str(ELSEWHERE).unwrap();
    assert!(
        words.len() >= 12,
        "the third reading's messages are all here"
    );
    for entry in &words {
        let harness = Harness::from_name(entry["harness"].as_str().unwrap()).unwrap();
        let message = entry["message"].as_str().unwrap();
        let (told, kept) = failed_on(harness, message);
        assert_eq!(told, message, "{harness:?}, as the daemon returns it");
        assert_eq!(kept, message, "{harness:?}, as it is kept");
    }
}

/// The named ones, in the words of the third reading, so that a change of the file above cannot
/// take them out unseen.
#[test]
fn the_login_words_of_claude_code_devin_and_muse_are_not_withheld() {
    for (harness, message) in [
        (Harness::Claude, "Invalid API key · Please run /login"),
        (
            Harness::Devin,
            "Not logged in. Run `devin auth login` to authenticate.",
        ),
        (
            Harness::Muse,
            "No Meta credentials were found. Run `muse login`.",
        ),
        (
            Harness::Muse,
            "API error 401 [request_id=x]: invalid api key (authentication_error)",
        ),
    ] {
        let (told, kept) = failed_on(harness, message);
        assert_eq!((told.as_str(), kept.as_str()), (message, message));
    }
}

#[test]
fn a_failure_that_quotes_a_credential_is_kept_and_returned_without_it() {
    let failure = format!("{ENDED}Error: Failed to load extension\nAuthorization: Bearer {BEARER}");
    let (told, kept) = failed_with(&failure);
    for why in [told, kept] {
        assert_withheld(&why, &[BEARER]);
        assert!(
            why.starts_with("pi --mode rpc ended before it answered (exit 1); it said:"),
            "ConsensFlow's own description and the exit code stay: {why}"
        );
    }
}

/// The review's probe (F2): a key with a blank in it, behind a name the mask knows only for a
/// value with no blank. The mask hid nothing of it.
#[test]
fn a_key_with_blanks_in_it_is_withheld_whole_not_masked() {
    let failure = format!("{ENDED}extension failed: API_KEY: sk-abcd efghijkl mnopqrst");
    let (told, kept) = failed_with(&failure);
    for why in [told, kept] {
        assert_withheld(&why, &["sk-abcd", "efghijkl", "mnopqrst"]);
        assert!(why.contains("ended before it answered (exit 1)"), "{why}");
    }
}

#[test]
fn a_password_of_several_words_is_withheld_whole_not_masked_at_its_first() {
    let failure = format!("{ENDED}login failed, password: correct horse battery staple");
    let (told, _) = failed_with(&failure);
    assert_withheld(&told, &["correct", "horse", "battery", "staple"]);
}

/// The third reading's probe (F2): a value that runs over lines. The first line was withheld and
/// both continuations were kept, in the listing and in the ledger.
#[test]
fn a_key_that_runs_over_lines_is_withheld_with_every_line_after_it() {
    let failure = format!("{ENDED}Error: extension failed\nAPI_KEY: sk-abcd\nefghijkl\nmnopqrst");
    let (told, kept) = failed_with(&failure);
    for why in [told, kept] {
        assert_withheld(&why, &["sk-abcd", "efghijkl", "mnopqrst"]);
        assert!(
            !why.contains("extension failed"),
            "the words of Pi are withheld as a block, not line by line: {why}"
        );
    }
}

/// T-1276, `frame-label-continuation`: a label in a line of the shape of a stack frame, and the
/// words of its value on the lines below it. Returned and kept, none of it.
#[test]
fn a_label_in_a_line_shaped_like_a_frame_withholds_the_lines_of_its_value() {
    let failure = format!("{ENDED}at password: correct (native)\nhorse\nbattery\nstaple");
    let (told, kept) = failed_with(&failure);
    for why in [told, kept] {
        assert_withheld(&why, &["correct", "horse", "battery", "staple"]);
    }
}

/// T-1276, `frame-wrapped-token`: a frame between the two halves of a token the wrap cut in two.
/// Returned and kept, no part of it.
#[test]
fn a_token_cut_in_two_with_a_frame_between_its_halves_is_withheld_whole() {
    let failure = format!("{ENDED}s\nat load (native)\nk-fakeCloverOak12345");
    let (told, kept) = failed_with(&failure);
    for why in [told, kept] {
        assert_withheld(&why, &["k-fakeCloverOak12345"]);
    }
}

/// T-1276, `wrapped-url-userinfo`: a line end in the middle of a URL's user and password. Returned
/// and kept, neither half.
#[test]
fn a_url_whose_user_and_password_a_line_end_splits_is_withheld_whole() {
    let failure = format!("{ENDED}FetchError: https://alder:copper\nwillow@host.invalid/catalog");
    let (told, kept) = failed_with(&failure);
    for why in [told, kept] {
        assert_withheld(&why, &["alder", "copper", "willow"]);
    }
}

/// T-1276, `label-before-4KiB`, `long-password-continuation`: a read of Pi that kept only the end
/// of what Pi said puts the sentence after the mark, and the daemon returns and keeps it as it is.
#[test]
fn the_sentence_a_cut_read_put_in_place_of_pis_words_is_returned_and_kept_as_it_is() {
    let failure = format!("{ENDED}{WITHHELD}");
    let (told, kept) = failed_with(&failure);
    assert_eq!(told, failure);
    assert_eq!(kept, failure);
}

#[test]
fn a_credential_word_on_any_line_withholds_the_words_before_it_and_after_it() {
    for words in [
        "key: abcdefgh ijklmnop\nthe rest",
        "alpha line\ntoken = a b c\nomega line",
        "\"secret\": \"two words\"\nand more",
        "one\ntwo\nauthorization bearer zzzz yyyy",
        "Password=hunter two\nthree",
        "key:\nvalue on the next line",
    ] {
        let (told, _) = failed_with(&format!("{ENDED}{words}"));
        assert!(told.contains(WITHHELD), "{words} was quoted: {told}");
        for line in words.lines() {
            assert!(!told.contains(line), "{line} in {told}");
        }
    }
}

/// What Pi says when it has no credential in it is said as it was, with the description before
/// it and the names of what it was loading.
#[test]
fn the_words_of_pi_that_hold_no_credential_are_quoted_as_they_were() {
    let failure = "pi --mode rpc did not answer within 30 s. Pi loads the packages and extensions \
                   it has before it lists, and one of them may be waiting for something (packages \
                   npm:@scope/slow-provider@1.2.3); it said:\nError: Failed to load extension \
                   \"/x/e.ts\": Cannot find module 'left-pad'\nHint: Start without extensions \
                   using \"pi -ne\".";
    let (told, kept) = failed_with(failure);
    assert_eq!(told, failure);
    assert_eq!(kept, failure);
}

#[test]
fn a_stale_listing_a_harness_answers_is_kept_without_the_credential_in_its_reason() {
    let home = home(&["pi"]);
    let stale = models(&["a/b"], 1_000).stale(format!(
        "Pi's own catalog: pi update --models failed: https://user:{URL_TOKEN}@example.invalid/x"
    ));
    let pi = Listing::answering([Ok(stale)]);
    let lists = lists(&home, ByKind(vec![("pi", pi)]));
    let read = done(lists.read(Harness::Pi, false)).unwrap();
    let listing = &read.listing;
    let Freshness::Stale { why } = &listing.freshness else {
        panic!("{listing:?}");
    };
    assert!(!why.contains(URL_TOKEN) && !why.contains("user:"), "{why}");
    assert_eq!(listing.models.len(), 1, "the models are as they were");
    let kept = lists.shown(Harness::Pi, read.id).unwrap();
    assert_eq!(
        &kept, listing,
        "what a submission is checked against is what was returned"
    );
}

/// The real Pi adapter, over a Pi folder and a Pi that is scripted, kept by the daemon's
/// lists: what the page reads of why Pi listed no model, from end to end
/// (the three the review T-1270 named: a token in a git package's URL, one in an
/// extension's error, one in the environment).
mod from_pi_to_the_ledger {
    use cf_harness::pi::PiAdapter;
    use cf_harness::testing::{ChildScript, Ends, Fakes};

    use super::*;

    struct RealPi(Rc<dyn Adapter>);

    impl Adapters for RealPi {
        fn adapter(&self, harness: &str) -> Option<Rc<dyn Adapter>> {
            (harness == "pi").then(|| Rc::clone(&self.0))
        }
    }

    /// What the page reads of Pi's list, when Pi has `files` in its folder and `vars` in its
    /// environment, starts as `child` says (the lines it writes, and what it says on its error
    /// stream and the code it exits with), and the human's Refresh (`refresh`) has `pi update
    /// --models` answer `update`: the listing the daemon told, which is the one it kept.
    fn pi_listing(
        files: &[(&str, &str)],
        vars: &[(&str, &str)],
        child: (&[&str], Ends, Option<(&str, i32)>),
        update: Option<Result<String, cf_process::Failed>>,
        refresh: bool,
    ) -> ModelListing {
        let home = home(&["pi"]);
        let agent = home._folder.path().join("agent");
        std::fs::create_dir_all(&agent).unwrap();
        for (name, text) in files {
            std::fs::write(agent.join(name), text).unwrap();
        }
        let env = vars.iter().fold(
            home.env
                .clone()
                .with_var("PI_CODING_AGENT_DIR", &agent.to_string_lossy()),
            |env, (name, value)| env.with_var(name, value),
        );
        let fakes = Fakes::new(&env);
        let (lines, ends, said) = child;
        fakes.processes.child(
            "pi",
            ChildScript {
                lines: lines.iter().map(|line| (*line).to_owned()).collect(),
                ends,
            },
        );
        if let Some((said, code)) = said {
            fakes.processes.child_said("pi", said, Some(code));
        }
        if let Some(update) = update {
            fakes.processes.run_answer("pi update --models", update);
        }
        let services = fakes.services(&env, home._folder.path());
        let adapter: Rc<dyn Adapter> = Rc::new(PiAdapter::new(&services));
        let lists = ModelLists::new(Rc::new(RealPi(adapter)), env);
        let read = eventually_clocked(lists.read(Harness::Pi, refresh), &fakes.time).unwrap();
        let kept = lists.shown(Harness::Pi, read.id).unwrap();
        assert_eq!(kept, read.listing, "what is kept is what the daemon told");
        read.listing
    }

    /// The reason a listing is stale, which the page reads.
    fn stale_why(listing: &ModelListing) -> String {
        let Freshness::Stale { why } = &listing.freshness else {
            panic!("the listing is stale for why: {listing:?}");
        };
        why.clone()
    }

    /// What the page reads of why Pi listed none, when it starts as `child` says.
    fn why_pi_listed_none(
        files: &[(&str, &str)],
        vars: &[(&str, &str)],
        child: (&[&str], Ends, Option<(&str, i32)>),
    ) -> String {
        stale_why(&pi_listing(files, vars, child, None, false))
    }

    /// What a Pi with no provider answers each of the asks of a read that waits for its providers
    /// (the page's does, A4-28): no model, every time, so that what it says is confirmed.
    fn empties() -> Vec<&'static str> {
        (1..=12)
            .map(|ask| {
                let id = if ask == 1 {
                    "models".to_owned()
                } else {
                    format!("models-{ask}")
                };
                let line = format!(
                    r#"{{"type":"response","id":"{id}","success":true,"data":{{"models":[]}}}}"#
                );
                &*Box::leak(line.into_boxed_str())
            })
            .collect()
    }

    #[test]
    fn a_token_in_a_git_packages_url_is_in_neither_the_ledger_nor_the_page() {
        let settings = serde_json::json!({ "packages": [
            format!("git:https://user:{URL_TOKEN}@example.invalid/t/p.git?token={URL_TOKEN}#r")
        ] })
        .to_string();
        let why = why_pi_listed_none(
            &[("settings.json", &settings)],
            &[],
            (&empties(), Ends::Asked, None),
        );
        assert!(!why.contains(URL_TOKEN) && !why.contains("user:"), "{why}");
        assert!(
            why.contains("git:https://example.invalid/t/p.git"),
            "the package is named: {why}"
        );
    }

    #[test]
    fn a_token_in_an_extensions_error_is_in_neither_the_ledger_nor_the_page() {
        let said = format!(
            "Error: Failed to load extension \"/x/e.ts\": Authorization: Bearer {BEARER}\n"
        );
        let why = why_pi_listed_none(&[], &[], (&[], Ends::Itself, Some((&said, 1))));
        assert_withheld(&why, &[BEARER]);
        assert!(why.contains("ended before it answered (exit 1)"), "{why}");
    }

    #[test]
    fn a_token_in_the_environment_is_in_neither_the_ledger_nor_the_page() {
        let why = why_pi_listed_none(
            &[],
            &[
                ("OPENROUTER_API_KEY", URL_TOKEN),
                ("ANTHROPIC_OAUTH_TOKEN", BEARER),
            ],
            (&empties(), Ends::Asked, None),
        );
        for leaked in [URL_TOKEN, BEARER] {
            assert!(!why.contains(leaked), "{leaked} in {why}");
        }
        // The line is Pi's setup in ConsensFlow's own words, and a variable's name says "key"
        // and "token": it names each by whose it is, so that the rule that withholds a line
        // which may hold a credential does not take the whole of it.
        assert!(
            !why.contains("API_KEY") && !why.contains("OAUTH_TOKEN") && !why.contains(WITHHELD),
            "the line is not withheld and names no variable: {why}"
        );
        assert!(
            why.contains("OPENROUTER") && why.contains("ANTHROPIC"),
            "the providers are named: {why}"
        );
    }

    /// What Pi, started, says on its error stream and ends with exit 1.
    fn pi_ends_saying(said: &str) -> String {
        why_pi_listed_none(&[], &[], (&[], Ends::Itself, Some((said, 1))))
    }

    /// The third reading's probe (F2, "cut before filter"): five lines that begin `password:
    /// correct`. The harness kept the last four before the engine looked, so the label was gone
    /// and the rest of the password was told.
    #[test]
    fn five_lines_that_begin_with_a_password_are_withheld_not_cut_to_the_four_after_it() {
        let why =
            pi_ends_saying("password: correct\nhorse\nbattery\nstaple\nprovider unavailable\n");
        assert_withheld(&why, &["correct", "horse", "battery", "staple"]);
        assert!(why.contains("ended before it answered (exit 1)"), "{why}");
        assert!(
            !why.contains("provider unavailable"),
            "all of Pi's words go: {why}"
        );
    }

    /// The third reading's probe (F2, "multi-line key"), from Pi's process into the ledger.
    #[test]
    fn a_key_that_runs_over_lines_in_pi_s_error_stream_is_in_neither_the_ledger_nor_the_page() {
        let why = pi_ends_saying("Error: extension failed\nAPI_KEY: sk-abcd\nefghijkl\nmnopqrst\n");
        assert_withheld(&why, &["sk-abcd", "efghijkl", "mnopqrst"]);
        assert!(why.contains("ended before it answered (exit 1)"), "{why}");
    }

    /// A pretty-printed JSON error object holding an `apiKey` has the key on a line of its own.
    #[test]
    fn a_pretty_printed_error_object_holding_an_api_key_is_in_neither_the_ledger_nor_the_page() {
        let error = serde_json::to_string_pretty(&serde_json::json!({
            "error": { "message": "provider failed", "config": {
                "apiKey": "sk-abcd efghijkl", "retries": 3 } }
        }))
        .unwrap();
        let why = pi_ends_saying(&format!("{error}\n"));
        assert_withheld(&why, &["sk-abcd", "efghijkl"]);
        assert!(!why.contains("provider failed"), "{why}");
    }

    /// The same words with no credential in them are quoted: this is what the user is told.
    #[test]
    fn what_pi_says_that_holds_no_credential_is_told_as_it_was_with_its_exit_code() {
        let said = "Error: Failed to load extension \"/x/e.ts\": boom\n    at load (/x/e.ts:1:1)\n\
                    Hint: Start without extensions using \"pi -ne\".\n";
        let why = pi_ends_saying(said);
        assert_eq!(
            why,
            format!(
                "{ENDED}Error: Failed to load extension \"/x/e.ts\": boom\nHint: Start without \
                 extensions using \"pi -ne\"."
            )
        );
    }

    /// What `pi update --models` said is Pi's words too: a credential on a line before its last
    /// was left behind by `lines().last()`, and the line after it was told.
    #[test]
    fn what_a_failed_pi_update_said_is_screened_whole_before_its_last_line_is_taken() {
        let failed = cf_process::Failed {
            message: "Command failed: pi update --models\npassword: correct\nhorse\nbattery\n"
                .to_owned(),
            code: Some(1),
            killed: false,
            stdout: String::new(),
        };
        let listed = r#"{"type":"response","id":"models","success":true,"data":{"models":[{"provider":"a","id":"b","name":"b"}]}}"#;
        let listing = pi_listing(
            &[],
            &[],
            (&[listed], Ends::Asked, None),
            Some(Err(failed)),
            true,
        );
        assert_eq!(listing.models.len(), 1, "the list Pi keeps");
        let why = stale_why(&listing);
        assert_withheld(&why, &["correct", "horse", "battery"]);
        assert!(
            why.contains("Pi's own catalog: pi update --models failed"),
            "{why}"
        );
    }

    /// Pi's refusal of the request carries Pi's words in its `error`.
    #[test]
    fn a_refusal_in_pi_s_reply_is_pi_s_words_and_screened_whole() {
        let refused = serde_json::json!({ "type": "response", "id": "models", "success": false,
            "error": "provider rejected: password: correct\nhorse" })
        .to_string();
        let why = why_pi_listed_none(&[], &[], (&[&refused], Ends::Asked, None));
        assert_withheld(&why, &["correct", "horse"]);
        assert!(why.contains("pi refused get_available_models"), "{why}");
    }

    /// Every kind of setup Pi's empty list is told from, with every kind of credential variable
    /// set, through the daemon. ConsensFlow's own words about a setup stand in the description,
    /// before the mark, where nothing is screened; they are held to the rule all the same (the
    /// round-3 rewording stays as long as it passes): put after the mark, as if they were Pi's
    /// words, none of them is withheld. A word of them that the rule takes (a variable named
    /// whole, `auth.json`, "such as OPENROUTER_API_KEY") would make the commonest message of a
    /// Pi that lists nothing a block withheld the day it is quoted.
    #[test]
    fn none_of_the_words_about_a_setup_is_taken_for_a_credential() {
        let big = " ".repeat(2 << 20);
        let oversize = format!(r#"{{"openai":{{"type":"api_key"}}{big}}}"#);
        let keys = [
            ("OPENAI_API_KEY", "x"),
            ("ANTHROPIC_OAUTH_TOKEN", "x"),
            ("HF_TOKEN", "x"),
            ("AWS_BEARER_TOKEN_BEDROCK", "x"),
            ("AWS_ACCESS_KEY_ID", "x"),
            ("GOOGLE_APPLICATION_CREDENTIALS", "x"),
            ("QWEN_TOKEN_PLAN_API_KEY", "x"),
            ("COPILOT_GITHUB_TOKEN", "x"),
            ("AZURE_OPENAI_API_KEY", "x"),
        ];
        let several = serde_json::json!({
            "packages": ["npm:@scope/slow-provider@1.2.3", "git:github.com/owner/repo@ref"],
            "extensions": ["./declared.ts"]
        })
        .to_string();
        type Case<'a> = (&'a str, Vec<(&'a str, &'a str)>, Vec<(&'a str, &'a str)>);
        let cases: [Case<'_>; 6] = [
            ("nothing", vec![], vec![]),
            ("logins", vec![("auth.json", r#"{"openai":{}}"#)], vec![]),
            (
                "custom, packages and extensions",
                vec![
                    ("models.json", r#"{"providers":{"mylocal":{}}}"#),
                    ("settings.json", &several),
                ],
                vec![],
            ),
            ("every kind of key", vec![], keys.to_vec()),
            (
                "a file of logins too large",
                vec![("auth.json", &oversize)],
                vec![],
            ),
            (
                "a login, a package and a key",
                vec![
                    ("auth.json", r#"{"openai":{}}"#),
                    ("settings.json", &several),
                ],
                keys.to_vec(),
            ),
        ];
        for (name, files, vars) in cases {
            let why = why_pi_listed_none(&files, &vars, (&empties(), Ends::Asked, None));
            assert!(why.starts_with("Pi lists no model"), "{name}: {why}");
            assert!(!why.contains(WITHHELD), "{name} was withheld: {why}");
            // These words stand before the mark, where nothing is screened; they are still held
            // to the rule: put after it, as if they were Pi's, none of them is withheld.
            let screened = cf_engine::quotable(&format!("x{}{why}", cf_proto::models::IT_SAID));
            assert!(
                !screened.contains(WITHHELD),
                "{name}: ConsensFlow's own words trip the rule: {why}"
            );
        }
    }

    /// The words ConsensFlow gives when Pi has nothing set up, the commonest of all, are no line
    /// that may hold a credential: they reach the page as they were written.
    #[test]
    fn a_pi_with_nothing_set_up_keeps_its_words_through_the_daemon() {
        let why = why_pi_listed_none(&[], &[], (&empties(), Ends::Asked, None));
        assert!(
            why.starts_with("Pi lists no model: no provider is set up (") && why.contains("/login"),
            "{why}"
        );
        assert!(!why.contains(WITHHELD), "{why}");
    }
}
