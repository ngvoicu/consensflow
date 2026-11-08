//! The list the page reads of Pi is the list a Pi window shows its human, not the one Pi has at
//! the moment it starts (A4-28, T-1321). A package's provider answers `get_available_models`
//! from its own cache at once and asks its server a moment later (`pi-cliproxyapi-provider`:
//! its cache at 0.3 s, the server's answer at 1.8 s, measured on Pi 1.5.0, offline and online
//! alike), and a human opens `/model` seconds after the start. The read asks again, until what
//! Pi lists has stood still for a while.
//!
//! The stand-in Pi's lines answer the requests in the order they are asked, by their ids
//! (`models`, then `models-2`, `models-3`, …), and the manual clock says how long the read
//! waited.

use std::rc::Rc;

use cf_base::env::Env;
use serde_json::json;

use super::standin::answer;
use crate::contract::Adapter as _;
use crate::pi::models::{wait_allowed, waits_for};
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
    _folder: tempfile::TempDir,
    adapter: Rc<PiAdapter>,
    fakes: Fakes,
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
        _folder: folder,
        adapter: Rc::new(PiAdapter::new(&services)),
        fakes,
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

/// The ids of what the page's read lists, and how many milliseconds of the clock it took.
fn read(window: &Window) -> (Vec<String>, i64) {
    let begun = window.fakes.time.wall_ms();
    let adapter = Rc::clone(&window.adapter);
    let mut driver = Driver::default();
    driver.begin(0, async move { adapter.models(false).await });
    let listing = drive(&mut driver, &window.fakes.time).unwrap();
    let ids = listing.models.iter().map(|model| model.model()).collect();
    (ids, window.fakes.time.wall_ms() - begun)
}

const CACHE: &[&str] = &["p/a", "p/b"];
const LIVE: &[&str] = &["p/a", "p/b", "p/new"];

#[test]
fn the_page_s_read_lists_what_a_package_adds_after_pi_starts() {
    let window = window(lines(&[CACHE, CACHE, LIVE], 60), Ends::Asked);
    let (ids, _) = read(&window);
    assert_eq!(
        ids,
        ["p/a", "p/b", "p/new"],
        "the list Pi shows once its providers loaded"
    );
}

#[test]
fn a_list_that_stands_still_ends_the_wait_and_one_that_changes_starts_it_again() {
    let still = window(lines(&[CACHE], 60), Ends::Asked);
    let (_, still_ms) = read(&still);
    assert_eq!(
        still_ms, 2_500,
        "a list that never changes is the list after 2.5 s"
    );

    // It changes at the third ask, 500 ms in; 2.5 s of quiet follow.
    let changed = window(lines(&[CACHE, CACHE, LIVE], 60), Ends::Asked);
    let (_, changed_ms) = read(&changed);
    assert_eq!(changed_ms, 500 + 2_500);
}

#[test]
fn a_list_that_never_stops_changing_is_given_up_on_at_the_cap() {
    let ids: Vec<Vec<String>> = (0..80).map(|n| vec![format!("p/m{n}")]).collect();
    let lists: Vec<Vec<&str>> = ids
        .iter()
        .map(|row| row.iter().map(String::as_str).collect())
        .collect();
    let lists: Vec<&[&str]> = lists.iter().map(Vec::as_slice).collect();
    let window = window(lines(&lists, 80), Ends::Asked);
    let (listed, ms) = read(&window);
    assert_eq!(ms, 12_000, "the wait has a cap");
    assert_eq!(listed.len(), 1, "the last list Pi gave");
}

#[test]
fn a_pi_that_ends_after_its_first_answer_is_still_read() {
    let window = window(lines(&[CACHE], 1), Ends::Itself);
    let (ids, _) = read(&window);
    assert_eq!(
        ids,
        ["p/a", "p/b"],
        "a wait that fails keeps the list Pi gave"
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
fn only_an_answer_that_lists_models_is_waited_on() {
    let listed = serde_json::from_str(&answer("models", CACHE)).unwrap();
    assert!(waits_for(&listed));
    let none = serde_json::from_str(&answer("models", &[])).unwrap();
    assert!(
        !waits_for(&none),
        "Pi's account of no provider is told at once"
    );
    let refused = json!({ "type": "response", "id": "models", "success": false, "error": "no" });
    assert!(!waits_for(&refused));
}
