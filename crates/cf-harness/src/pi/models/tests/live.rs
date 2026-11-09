//! The list the page reads of Pi is the list a Pi window shows its human, not the one Pi has at
//! the moment it starts (A4-28, T-1321). A package's provider answers `get_available_models`
//! from its own cache at once and asks its server a moment later (`pi-cliproxyapi-provider`:
//! its cache at 0.3 s, the server's answer at 1.8 s, measured on Pi 1.5.0, offline and online
//! alike), and a human opens `/model` seconds after the start. The read asks again, until what
//! Pi lists has stood still for a while.
//!
//! Pi tells of no moment at which its providers have loaded (1.5.0's rpc has no event, no state
//! field and no command for it: `get_state` holds no models, `get_available_models` is the
//! runtime's snapshot, and "Model catalogs refreshed." is a line of the interactive picker), so
//! a list is **confirmed** only by standing still for 2.5 s, every ask answered. Any other end
//! of the wait (a refusal, an ask that goes unanswered, Pi ending, the cap reached while the
//! list still changes) leaves the list Pi gave **stale**, with a plain why, never fresh. An
//! answer that lists no model is waited on like any other.
//!
//! The stand-in Pi's lines answer the requests in the order they are asked, by their ids
//! (`models`, then `models-2`, `models-3`, …), and the manual clock says how long the read
//! waited.

use std::rc::Rc;

use cf_base::env::Env;
use serde_json::json;

use super::standin::answer;
use crate::contract::Adapter as _;
use crate::pi::models::wait_allowed;
use crate::pi::PiAdapter;
use crate::seams::Time as _;
use crate::testing::{fake_executable, ChildScript, Driver, Ends, Fakes, ManualTime};

/// What the stand-in Pi lists at each ask: the answer to the `n`th request is `lists[n - 1]`, the
/// last of them for every ask after.
fn lines(lists: &[&[&str]], asks: usize) -> Vec<String> {
    (1..=asks)
        .map(|n| {
            let id = if n == 1 {
                "models".to_owned()
            } else {
                format!("models-{n}")
            };
            answer(&id, lists[(n - 1).min(lists.len() - 1)])
        })
        .collect()
}

struct Window {
    adapter: Rc<PiAdapter>,
    fakes: Fakes,
    _folder: tempfile::TempDir,
}

/// A Pi that answers with `lines` and `ends` as given, as the page's adapter reads it.
fn window(lines: Vec<String>, ends: Ends) -> Window {
    let folder = tempfile::tempdir().unwrap();
    let pi = fake_executable(&folder.path().join("pi"));
    let agent = folder.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    let env = Env::from_vars([
        ("PATH", pi.parent().unwrap().as_os_str()),
        ("PI_CODING_AGENT_DIR", agent.as_os_str()),
    ]);
    let fakes = Fakes::new(&env);
    fakes.processes.child("pi", ChildScript { lines, ends });
    let services = fakes.services(&env, folder.path());
    Window {
        adapter: Rc::new(PiAdapter::new(&services)),
        fakes,
        _folder: folder,
    }
}

/// `driver`'s one work, run with the clock fired as the work waits on it.
fn drive<T>(driver: &mut Driver<T>, time: &ManualTime) -> T {
    loop {
        if let Some((_, value)) = driver.run().pop() {
            return value;
        }
        assert!(
            time.fire_next(i64::MAX),
            "the work waits on nothing the clock holds"
        );
    }
}

/// What the page's read made of the stand-in Pi: the ids it lists, why it is stale where it is,
/// and how many milliseconds of the clock it took.
struct Read {
    ids: Vec<String>,
    stale: Option<String>,
    ms: i64,
}

/// The page's read, with the clock fired as it waits on it.
fn read_listing(window: &Window) -> (Result<cf_proto::models::ModelListing, String>, i64) {
    let begun = window.fakes.time.wall_ms();
    let adapter = Rc::clone(&window.adapter);
    let mut driver = Driver::default();
    driver.begin(0, async move { adapter.models(false).await });
    let listed = drive(&mut driver, &window.fakes.time);
    (listed, window.fakes.time.wall_ms() - begun)
}

fn read(window: &Window) -> Read {
    let (listed, ms) = read_listing(window);
    let listing = listed.unwrap();
    Read {
        ids: listing.models.iter().map(|model| model.model()).collect(),
        stale: match &listing.freshness {
            cf_proto::models::Freshness::Stale { why } => Some(why.clone()),
            cf_proto::models::Freshness::Fresh => None,
        },
        ms,
    }
}

/// Pi's refusal of the request `id`.
fn refusal(id: &str) -> String {
    json!({ "type": "response", "id": id, "success": false, "error": "no" }).to_string()
}

const CACHE: &[&str] = &["p/a", "p/b"];
const LIVE: &[&str] = &["p/a", "p/b", "p/new"];

#[test]
fn the_page_s_read_lists_what_a_package_adds_after_pi_starts() {
    let window = window(lines(&[CACHE, CACHE, LIVE], 60), Ends::Asked);
    let got = read(&window);
    assert_eq!(
        got.ids,
        ["p/a", "p/b", "p/new"],
        "the list Pi shows once its providers loaded"
    );
    assert_eq!(got.stale, None, "a list that stood still is confirmed");
}

#[test]
fn a_list_that_stands_still_ends_the_wait_and_one_that_changes_starts_it_again() {
    let still = window(lines(&[CACHE], 60), Ends::Asked);
    let got = read(&still);
    assert_eq!(
        got.ms, 2_500,
        "a list that never changes is the list after 2.5 s"
    );
    assert_eq!(got.stale, None);

    // It changes at the third ask, 500 ms in; 2.5 s of quiet follow.
    let changed = window(lines(&[CACHE, CACHE, LIVE], 60), Ends::Asked);
    assert_eq!(read(&changed).ms, 500 + 2_500);
}

#[test]
fn a_list_that_never_stops_changing_is_stale_for_it_at_the_cap() {
    let ids: Vec<Vec<String>> = (0..80).map(|n| vec![format!("p/m{n}")]).collect();
    let lists: Vec<Vec<&str>> = ids
        .iter()
        .map(|row| row.iter().map(String::as_str).collect())
        .collect();
    let lists: Vec<&[&str]> = lists.iter().map(Vec::as_slice).collect();
    let window = window(lines(&lists, 80), Ends::Asked);
    let got = read(&window);
    assert_eq!(got.ms, 12_000, "the wait has a cap");
    assert_eq!(got.ids.len(), 1, "the last list Pi gave");
    let why = got
        .stale
        .expect("a list still changing is no confirmed list");
    assert!(
        why.contains("still loading its models") && why.contains("still changing"),
        "{why}"
    );
}

#[test]
fn a_pi_that_ends_after_its_first_answer_leaves_its_list_stale() {
    let window = window(lines(&[CACHE], 1), Ends::Itself);
    let got = read(&window);
    assert_eq!(got.ids, ["p/a", "p/b"], "the list Pi gave is kept");
    let why = got.stale.expect("a list Pi ended on is not confirmed");
    assert!(
        why.contains("still loading its models") && why.contains("ended"),
        "{why}"
    );
}

#[test]
fn an_ask_that_goes_unanswered_leaves_the_list_stale() {
    // Pi answers once and then says nothing, and stays: the second ask is given 5 s.
    let window = window(lines(&[CACHE], 1), Ends::Asked);
    let got = read(&window);
    assert_eq!(got.ids, ["p/a", "p/b"]);
    assert_eq!(got.ms, 250 + 5_000, "one poll, then the ask's own bound");
    let why = got
        .stale
        .expect("a list whose second ask went unanswered is not confirmed");
    assert!(
        why.contains("still loading its models") && why.contains("did not answer"),
        "{why}"
    );
}

#[test]
fn a_second_ask_that_pi_refuses_leaves_the_list_stale() {
    let mut script = lines(&[CACHE], 1);
    script.push(refusal("models-2"));
    let window = window(script, Ends::Asked);
    let got = read(&window);
    assert_eq!(got.ids, ["p/a", "p/b"], "the list Pi gave is kept");
    assert_eq!(got.ms, 250, "it ends at the refusal");
    let why = got
        .stale
        .expect("a refused second ask is not a confirmed list");
    assert!(
        why.contains("still loading its models") && why.contains("refused"),
        "{why}"
    );
}

#[test]
fn the_last_ask_of_a_wait_is_given_no_more_than_what_the_wait_has_left() {
    // Every list differs, so the wait runs to its cap; then Pi stops answering. The ask that
    // began 250 ms before the cap is given what is left (250 ms at the least), not 5 s.
    let ids: Vec<Vec<String>> = (0..47).map(|n| vec![format!("p/m{n}")]).collect();
    let lists: Vec<Vec<&str>> = ids
        .iter()
        .map(|row| row.iter().map(String::as_str).collect())
        .collect();
    let lists: Vec<&[&str]> = lists.iter().map(Vec::as_slice).collect();
    let window = window(lines(&lists, 47), Ends::Asked);
    let got = read(&window);
    assert_eq!(got.ids, ["p/m46"], "the last list Pi gave");
    assert!(
        got.ms <= 12_000 + 250,
        "the wait ended at its cap, plus the last ask's quarter second: {} ms",
        got.ms
    );
}

#[test]
fn a_first_answer_with_no_model_is_waited_on_like_any_other() {
    // A provider that registers a second after Pi starts: the first answer lists nothing.
    let window = window(lines(&[&[], &[], &[], &[], CACHE], 60), Ends::Asked);
    let got = read(&window);
    assert_eq!(got.ids, ["p/a", "p/b"], "what the provider added");
    assert_eq!(got.stale, None);
    assert_eq!(
        got.ms,
        1_000 + 2_500,
        "four polls to the list, then 2.5 s standing still"
    );
}

#[test]
fn a_first_answer_with_no_model_that_stays_so_is_no_list_after_the_same_wait() {
    let window = window(lines(&[&[]], 60), Ends::Asked);
    let (listed, ms) = read_listing(&window);
    assert!(listed.is_err(), "no model: {listed:?}");
    assert!(ms >= 2_500, "it waited as any read waits: {ms} ms");
}

#[test]
fn a_first_refusal_is_the_read_s_failure_and_waits_for_nothing() {
    let window = window(vec![refusal("models")], Ends::Asked);
    let (listed, ms) = read_listing(&window);
    assert!(listed.unwrap_err().contains("refused"), "Pi's own refusal");
    assert_eq!(ms, 0);
}

#[test]
fn the_wait_leaves_a_read_what_it_needs_of_its_bound() {
    use crate::shared::one_shot::BOUND_MS;
    assert_eq!(wait_allowed(0), 12_000, "a quick start waits the whole cap");
    assert_eq!(
        wait_allowed(20_000),
        7_000,
        "a slow one waits for what the bound leaves, less 3 s"
    );
    assert_eq!(wait_allowed(BOUND_MS - 3_000), 0);
    assert_eq!(
        wait_allowed(BOUND_MS * 2),
        0,
        "a read past its bound waits for nothing"
    );
}

#[test]
fn the_read_that_picks_a_launch_s_default_model_does_not_wait() {
    let window = window(lines(&[CACHE, LIVE], 60), Ends::Asked);
    let begun = window.fakes.time.wall_ms();
    let adapter = Rc::clone(&window.adapter);
    let mut driver = Driver::default();
    driver.begin(0, async move { adapter.default_model().await });
    let picked = drive(&mut driver, &window.fakes.time).unwrap();
    assert_eq!(picked.as_deref(), Some("p/a"));
    assert_eq!(
        window.fakes.time.wall_ms() - begun,
        0,
        "a window's launch waits for no provider"
    );
}
