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
/// the ledger keeps for the page.
fn failed_on(harness: Harness, failure: &str) -> (String, String) {
    let home = home(&[harness.as_str()]);
    let adapter = Listing::answering([Err(failure.to_owned())]);
    let lists = lists(&home, ByKind(vec![(harness.kind(), adapter)]));
    let listing = done(lists.get(&home.ledger, harness, false)).unwrap();
    let Freshness::Stale { why } = &listing.freshness else {
        panic!("a failed read is stale: {listing:?}");
    };
    let kept = home
        .ledger
        .borrow()
        .model_listing(harness.as_str())
        .unwrap()
        .unwrap();
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
        assert_eq!(kept, message, "{harness:?}, as the ledger keeps it");
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
        "key: abcdefgh12 ijklmnop\nthe rest",
        "alpha line\ntoken = abcdefgh12 b c\nomega line",
        "\"secret\": \"two words\"\nand more",
        "one\ntwo\nauthorization: Bearer zzzzyyyy1234",
        "Password=hunter two\nthree",
        "key:\nabcdefgh12\nvalue on the next line",
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
    // A reason with no words of Pi's in it is ConsensFlow's, masked as a note masks it.
    assert!(!why.contains(BEARER) && why.contains("[masked]"), "{why}");
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
        let listing = eventually(lists.get(&home.ledger, Harness::Pi, refresh)).unwrap();
        let kept = home.ledger.borrow().model_listing("pi").unwrap().unwrap();
        assert_eq!(kept, listing, "the page reads what the daemon told");
        listing
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

    /// A4-08: Pi's failures that name a credential and hold none (alpha.3 withheld each for its
    /// word) reach the page as Pi said them, with the exit code.
    #[test]
    fn what_pi_says_that_only_names_a_credential_is_told_as_it_was_with_its_exit_code() {
        for said in [
            "Error: Failed to load extension \"/home/h/.pi/agent/extensions/oauth.ts\": OAuth \
             login expired. Run /login\nHint: Start without extensions using \"pi -ne\".",
            "Error: Failed to load extension \"/x/e.ts\": Invalid key: expected string",
            "Error: Failed to load extension \"/x/e.ts\": OPENAI_API_KEY is not set",
            "Error: Failed to load extension \"/x/e.ts\": Cannot find module 'oauth-helper'",
            "SyntaxError: Unexpected token: }",
        ] {
            let why = pi_ends_saying(&format!("{said}\n"));
            assert_eq!(why, format!("{ENDED}{said}"), "{said}");
        }
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
            why.starts_with("Pi's own catalog: pi update --models failed"),
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
            let why = why_pi_listed_none(&files, &vars, (&[EMPTY], Ends::Asked, None));
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
        let why = why_pi_listed_none(&[], &[], (&[EMPTY], Ends::Asked, None));
        assert!(
            why.starts_with("Pi lists no model: no provider is set up (") && why.contains("/login"),
            "{why}"
        );
        assert!(!why.contains(WITHHELD), "{why}");
    }
}
