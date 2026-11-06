use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use cf_engine::seams::Adapters;
use cf_harness::contract::{Adapter, FirstMessage, Launch, LaunchId, Prepared, Skill, Specs, Work};
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

    fn specmint(&self, _variant: Variant, _folder: &str) -> Result<Skill, String> {
        unreachable!("a list is read with no window")
    }

    fn place_specmint(&self, _specs: &Specs<'_>, _id: &LaunchId) -> Result<(), String> {
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
    let home = home(&["devin"]);
    let lists = lists(&home, ByKind(vec![]));
    assert_eq!(
        done(lists.get(&home.ledger, Harness::Devin, false)),
        Err("devin has no adapter".to_owned())
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

/// What a harness quotes of its own error stream or a package's source can carry a credential
/// (T-1270): an extension's error that echoes the header it was given, a git URL with a token
/// in it. None reaches the ledger, which the page reads, or the caller.
const BEARER: &str = "SYNTHETICSTDERRTOKEN0123456789";
const URL_TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB";

fn assert_no_credential(words: &str) {
    for leaked in [BEARER, URL_TOKEN] {
        assert!(!words.contains(leaked), "{leaked} in {words}");
    }
    assert!(words.contains("[masked]"), "something was hidden: {words}");
}

#[test]
fn a_failure_that_quotes_a_credential_is_kept_and_returned_without_it() {
    let home = home(&["pi"]);
    let failure = format!(
        "pi --mode rpc ended before it answered (exit 1); it said: Error: Failed to load \
         extension: Authorization: Bearer {BEARER}"
    );
    let pi = Listing::answering([Err(failure)]);
    let lists = lists(&home, ByKind(vec![("pi", pi)]));
    let listing = done(lists.get(&home.ledger, Harness::Pi, false)).unwrap();
    let Freshness::Stale { why } = &listing.freshness else {
        panic!("a failed read is stale: {listing:?}");
    };
    assert_no_credential(why);
    assert!(
        why.contains("Failed to load extension"),
        "the rest is said: {why}"
    );
    let kept = home.ledger.borrow().model_listing("pi").unwrap().unwrap();
    let Freshness::Stale { why } = &kept.freshness else {
        panic!("{kept:?}");
    };
    assert_no_credential(why);
}

#[test]
fn a_stale_listing_a_harness_answers_is_kept_without_the_credential_in_its_reason() {
    let home = home(&["pi"]);
    let stale = models(&["a/b"], 1_000).stale(format!(
        "Pi's own catalog: pi update --models failed: https://user:{URL_TOKEN}@example.invalid/x"
    ));
    let pi = Listing::answering([Ok(stale)]);
    let lists = lists(&home, ByKind(vec![("pi", pi)]));
    let listing = done(lists.get(&home.ledger, Harness::Pi, false)).unwrap();
    let Freshness::Stale { why } = &listing.freshness else {
        panic!("{listing:?}");
    };
    assert_no_credential(why);
    assert_eq!(listing.models.len(), 1, "the models are as they were");
    let kept = home.ledger.borrow().model_listing("pi").unwrap().unwrap();
    assert_eq!(kept, listing, "what the page reads is what was returned");
}

/// A listing an older build kept (a reason with a credential in it, as nothing hid it then) is
/// told without it: what the ledger holds is not what the page is shown.
#[test]
fn a_listing_kept_by_an_older_build_is_told_without_the_credential_in_its_reason() {
    let home = home(&["pi"]);
    let pi = Listing::answering([Ok(models(&["a/b"], 1_000))]);
    let lists = lists(&home, ByKind(vec![("pi", pi)]));
    done(lists.get(&home.ledger, Harness::Pi, false)).unwrap();
    let older = models(&["a/b"], 1_000).stale(format!("failed: Authorization: Bearer {BEARER}"));
    home.ledger
        .borrow_mut()
        .keep_model_listing("pi", &older)
        .unwrap();
    let told = done(lists.get(&home.ledger, Harness::Pi, false)).unwrap();
    let Freshness::Stale { why } = &told.freshness else {
        panic!("{told:?}");
    };
    assert_no_credential(why);
}

/// The real Pi adapter, over a Pi folder and a Pi that is scripted, kept by the daemon's
/// lists into a ledger: what the page reads of why Pi listed no model, from end to end
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
    /// environment, and starts as `child` says (the lines it writes, and what it says on its error
    /// stream and the code it exits with).
    fn why_pi_listed_none(
        files: &[(&str, &str)],
        vars: &[(&str, &str)],
        child: (&[&str], Ends, Option<(&str, i32)>),
    ) -> String {
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
        let services = fakes.services(&env, home._folder.path());
        let adapter: Rc<dyn Adapter> = Rc::new(PiAdapter::new(&services));
        let lists = ModelLists::new(Rc::new(RealPi(adapter)), env);
        let listing = done(lists.get(&home.ledger, Harness::Pi, false)).unwrap();
        let kept = home.ledger.borrow().model_listing("pi").unwrap().unwrap();
        assert_eq!(kept, listing, "the page reads what the daemon told");
        let Freshness::Stale { why } = kept.freshness else {
            panic!("Pi listed none: the listing is stale for why");
        };
        why
    }

    const EMPTY: &str = r#"{"type":"response","id":"models","success":true,"data":{"models":[]}}"#;

    #[test]
    fn a_token_in_a_git_packages_url_is_in_neither_the_ledger_nor_the_page() {
        let settings = serde_json::json!({ "packages": [
            format!("git:https://user:{URL_TOKEN}@example.invalid/t/p.git?token={URL_TOKEN}#r")
        ] })
        .to_string();
        let why = why_pi_listed_none(
            &[("settings.json", &settings)],
            &[],
            (&[EMPTY], Ends::Asked, None),
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
        assert_no_credential(&why);
        assert!(why.contains("ended before it answered (exit 1)"), "{why}");
        assert!(
            why.contains("Failed to load extension"),
            "the rest is said: {why}"
        );
    }

    #[test]
    fn a_token_in_the_environment_is_in_neither_the_ledger_nor_the_page() {
        let why = why_pi_listed_none(
            &[],
            &[
                ("OPENROUTER_API_KEY", URL_TOKEN),
                ("ANTHROPIC_OAUTH_TOKEN", BEARER),
            ],
            (&[EMPTY], Ends::Asked, None),
        );
        for leaked in [URL_TOKEN, BEARER] {
            assert!(!why.contains(leaked), "{leaked} in {why}");
        }
        assert!(
            why.contains("OPENROUTER_API_KEY"),
            "the key is named: {why}"
        );
    }
}
