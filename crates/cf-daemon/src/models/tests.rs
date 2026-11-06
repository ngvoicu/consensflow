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
    /// A gate that holds only a Refresh (a read that fetches).
    refresh_gate: Option<Rc<Notify>>,
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
        let gate = self
            .gate
            .clone()
            .or_else(|| self.refresh_gate.clone().filter(|_| fetch));
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
    let mut first = pin!(lists.get(&home.ledger, Harness::Devin, true));
    let mut second = pin!(lists.get(&home.ledger, Harness::Devin, true));
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

/// An ask that waited for the turn of a read of its harness is answered from what that read kept:
/// the harness is not read again for it.
#[test]
fn an_ask_that_waited_for_the_read_before_it_is_answered_from_what_that_read_kept() {
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
    let mut first = pin!(lists.get(&home.ledger, Harness::Devin, false));
    let mut second = pin!(lists.get(&home.ledger, Harness::Devin, false));
    assert!(first.as_mut().poll(&mut context).is_pending());
    assert!(second.as_mut().poll(&mut context).is_pending());
    gate.notify_one();
    let Poll::Ready(read) = first.as_mut().poll(&mut context) else {
        panic!("the read did not finish");
    };
    assert_eq!(read.unwrap().models[0].id, "swe-2");
    let Poll::Ready(waited) = second.as_mut().poll(&mut context) else {
        panic!("the ask that waited did not go on once the read kept its list");
    };
    assert_eq!(waited.unwrap().models[0].id, "swe-2");
    assert_eq!(*devin.asked.borrow(), [false], "read once, for both asks");
}

/// A Refresh of Pi is slow (its update, then its read: the page waits on it up to its bridge's
/// 30 s), and what the home keeps of another harness is answered at once (the review T-1270,
/// F8: a cached Claude listing timed out behind a Pi Refresh).
#[test]
fn a_cached_list_is_answered_at_once_while_another_harness_is_being_refreshed() {
    let home = home(&["claude", "pi"]);
    let gate = Rc::new(Notify::new());
    let claude = Listing::answering([Ok(models(&["opus"], 1_000))]);
    let pi = Rc::new(Listing {
        answers: RefCell::new([Ok(models(&["a/b"], 1_000)), Ok(models(&["a/c"], 2_000))].into()),
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
    done(lists.get(&home.ledger, Harness::Claude, false)).unwrap();
    done(lists.get(&home.ledger, Harness::Pi, false)).unwrap();

    let mut context = Context::from_waker(Waker::noop());
    let mut refresh = pin!(lists.get(&home.ledger, Harness::Pi, true));
    assert!(refresh.as_mut().poll(&mut context).is_pending());
    assert_eq!(
        *pi.asked.borrow(),
        [false, true],
        "Pi's Refresh is in flight"
    );

    let mut claude_asked = pin!(lists.get(&home.ledger, Harness::Claude, false));
    let Poll::Ready(kept) = claude_asked.as_mut().poll(&mut context) else {
        panic!("a Claude list the home kept waited behind a Pi Refresh");
    };
    assert_eq!(kept.unwrap().models[0].id, "opus");
    let mut pi_asked = pin!(lists.get(&home.ledger, Harness::Pi, false));
    let Poll::Ready(kept) = pi_asked.as_mut().poll(&mut context) else {
        panic!("a Pi list the home kept waited behind its own Refresh");
    };
    assert_eq!(
        kept.unwrap().models[0].id,
        "a/b",
        "what was kept before the Refresh"
    );
    assert_eq!(*claude.asked.borrow(), [false], "nothing was read again");

    gate.notify_one();
    let Poll::Ready(refreshed) = refresh.as_mut().poll(&mut context) else {
        panic!("the Refresh did not finish once Pi answered");
    };
    assert_eq!(refreshed.unwrap().models[0].id, "a/c");
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
    let mut slow = pin!(lists.get(&home.ledger, Harness::Pi, false));
    assert!(slow.as_mut().poll(&mut context).is_pending());
    let mut other = pin!(lists.get(&home.ledger, Harness::Devin, false));
    let Poll::Ready(read) = other.as_mut().poll(&mut context) else {
        panic!("Devin's first read waited for Pi's");
    };
    assert_eq!(read.unwrap().models[0].id, "swe-2");
    gate.notify_one();
    assert!(slow.as_mut().poll(&mut context).is_ready());
}

/// What a harness quotes of its own error stream or a package's source can carry a credential
/// (T-1270): an extension's error that echoes the header it was given, a git URL with a token
/// in it. A line of a harness's words that may hold one is withheld whole, not masked (the
/// review's second reading, F2), and none reaches the ledger, which the page reads, or the
/// caller.
const BEARER: &str = "SYNTHETICSTDERRTOKEN0123456789";
const URL_TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB";
const WITHHELD: &str = "(a line withheld: it may hold a credential)";

fn assert_withheld(words: &str, hidden: &[&str]) {
    for leaked in hidden {
        assert!(!words.contains(leaked), "{leaked} in {words}");
    }
    assert!(words.contains(WITHHELD), "a line was withheld: {words}");
}

/// What a read that failed with `failure` is kept and returned as, by the daemon.
fn failed_with(failure: &str) -> (String, String) {
    let home = home(&["pi"]);
    let pi = Listing::answering([Err(failure.to_owned())]);
    let lists = lists(&home, ByKind(vec![("pi", pi)]));
    let listing = done(lists.get(&home.ledger, Harness::Pi, false)).unwrap();
    let Freshness::Stale { why } = &listing.freshness else {
        panic!("a failed read is stale: {listing:?}");
    };
    let kept = home.ledger.borrow().model_listing("pi").unwrap().unwrap();
    let Freshness::Stale { why: kept } = &kept.freshness else {
        panic!("{kept:?}");
    };
    (why.clone(), kept.clone())
}

#[test]
fn a_failure_that_quotes_a_credential_is_kept_and_returned_without_it() {
    let failure = format!(
        "pi --mode rpc ended before it answered (exit 1); it said:\nError: Failed to load \
         extension\nAuthorization: Bearer {BEARER}"
    );
    let (told, kept) = failed_with(&failure);
    for why in [told, kept] {
        assert_withheld(&why, &[BEARER]);
        assert!(
            why.contains("ended before it answered (exit 1)")
                && why.contains("Failed to load extension"),
            "what is no credential is said: {why}"
        );
    }
}

/// The review's probe (F2): a key with a blank in it, behind a name the mask knows only for a
/// value with no blank. The mask hid nothing of it.
#[test]
fn a_key_with_blanks_in_it_is_withheld_whole_not_masked() {
    let failure = "pi --mode rpc ended before it answered (exit 1); it said:\nextension failed: \
                   API_KEY: sk-abcd efghijkl mnopqrst";
    let (told, kept) = failed_with(failure);
    for why in [told, kept] {
        assert_withheld(&why, &["sk-abcd", "efghijkl", "mnopqrst"]);
        assert!(why.contains("ended before it answered (exit 1)"), "{why}");
    }
}

#[test]
fn a_password_of_several_words_is_withheld_whole_not_masked_at_its_first() {
    let failure = "pi --mode rpc ended before it answered (exit 1); it said:\nlogin failed, \
                   password: correct horse battery staple";
    let (told, _) = failed_with(failure);
    assert_withheld(&told, &["correct", "horse", "battery", "staple"]);
}

#[test]
fn a_key_named_with_a_value_after_it_is_withheld() {
    for line in [
        "key: abcdefgh ijklmnop",
        "token = a b c",
        "\"secret\": \"two words\"",
        "authorization bearer zzzz yyyy",
        "Password=hunter two",
    ] {
        let (told, _) = failed_with(&format!("Pi said:\n{line}"));
        assert!(told.contains(WITHHELD), "{line} was quoted: {told}");
        assert!(!told.contains(line), "{line} in {told}");
    }
}

#[test]
fn a_line_that_names_no_credential_is_quoted_as_it_was() {
    let failure = "pi --mode rpc did not answer within 30 s; it said:\nError: Failed to load \
                   extension \"/x/e.ts\": Cannot find module 'left-pad'\nPi loads the packages \
                   and extensions it has before it lists";
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
    let listing = done(lists.get(&home.ledger, Harness::Pi, false)).unwrap();
    let Freshness::Stale { why } = &listing.freshness else {
        panic!("{listing:?}");
    };
    assert!(!why.contains(URL_TOKEN) && !why.contains("user:"), "{why}");
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
    assert_withheld(why, &[BEARER]);
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
        let listing = eventually(lists.get(&home.ledger, Harness::Pi, false)).unwrap();
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
            (&[EMPTY], Ends::Asked, None),
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

    /// Every kind of setup Pi's empty list is told from, with every kind of credential variable
    /// set, through the daemon: the engine withholds a line that names a key, a token or a login
    /// file, and none of ConsensFlow's own words about Pi's setup is one. A word of them that is
    /// (a variable named whole, `auth.json`, "such as OPENROUTER_API_KEY") makes the commonest
    /// message of a Pi that lists nothing a line withheld.
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
            let why = why_pi_listed_none(&files, &vars, (&[EMPTY], Ends::Asked, None));
            assert!(why.starts_with("Pi lists no model"), "{name}: {why}");
            assert!(!why.contains(WITHHELD), "{name} was withheld: {why}");
        }
    }

    /// The words ConsensFlow gives when Pi has nothing set up, the commonest of all, are no line
    /// that may hold a credential: they reach the page as they were written.
    #[test]
    fn a_pi_with_nothing_set_up_keeps_its_words_through_the_daemon() {
        let why = why_pi_listed_none(&[], &[], (&[EMPTY], Ends::Asked, None));
        assert!(
            why.starts_with("Pi lists no model: no provider is set up (") && why.contains("/login"),
            "{why}"
        );
        assert!(!why.contains(WITHHELD), "{why}");
    }
}
