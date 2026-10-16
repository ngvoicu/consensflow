//! What the tracker follows of the windows' processes, and the strangers it finds:
//! with no process, the system's answers written by hand.

use super::*;

fn process(pid: u32, command: &str) -> Process {
    Process {
        pid,
        command: command.to_owned(),
    }
}

fn window(pane: &str, generation: i64) -> Owner {
    (pane.to_owned(), generation)
}

/// What the system says of `pids`: each there, begun as `began <pid>` (said to the
/// second, as `ps` says it, so never after a look).
fn there(pids: &[u32]) -> BTreeMap<u32, String> {
    pids.iter()
        .map(|pid| (*pid, format!("began {pid}")))
        .collect()
}

/// The system's answer to the look that saw them: `there`.
fn known(tracker: &mut Tracker, pids: &[u32]) {
    tracker.identify(0, Ok(&there(pids)));
}

/// When the process `pid` the tracker follows began, if the system said at the look
/// that saw it.
fn began(tracker: &Tracker, pid: u32) -> Option<&str> {
    match &tracker.tracked.get(&pid)?.identity {
        Identity::Began(began) => Some(began),
        Identity::Asking | Identity::Never(_) => None,
    }
}

/// The numbers of what a check found to end.
fn leftovers(found: &Found) -> Vec<u32> {
    found
        .leftovers
        .iter()
        .map(|leftover| leftover.process.pid)
        .collect()
}

#[test]
fn a_process_still_there_a_grace_after_its_window_went_is_a_stranger_said_once() {
    let mut tracker = Tracker::default();
    let a = window("p1-a", 1);
    let new = tracker.saw(
        &a,
        &[process(10, "node dev-server.mjs"), process(11, "sleep 600")],
    );
    assert_eq!(new, [10, 11], "the numbers whose identity the look asks");
    known(&mut tracker, &[10, 11]);
    // Asked at every look that shows it, and the same process still.
    assert_eq!(tracker.saw(&a, &[process(10, "node dev-server.mjs")]), [10]);
    known(&mut tracker, &[10]);
    tracker.saw(&window("p1-b", 1), &[process(20, "npm test")]);
    known(&mut tracker, &[20]);
    assert!(tracker.due(1_000_000).is_empty(), "nothing went yet");
    tracker.gone(&a, 1_000, &[]);
    assert!(tracker.due(1_000 + GRACE).is_empty(), "within the grace");
    assert_eq!(tracker.due(1_001 + GRACE), [10, 11]);
    // After it: 10 is still there as it was, 11 is not.
    let found = tracker.strangers(1_001 + GRACE, &there(&[10]));
    assert_eq!(leftovers(&found), [10]);
    let left = &found.leftovers[0];
    assert_eq!(left.process, process(10, "node dev-server.mjs"));
    assert_eq!(
        (left.pane.as_str(), left.since, left.began.as_str()),
        ("p1-a", 1_000, "began 10")
    );
    assert!(found.unresolved.is_empty());
    assert_eq!(
        tracker.strangers(10 * GRACE, &there(&[10, 11])),
        Found::default(),
        "said once"
    );
}

#[test]
fn a_number_another_process_took_since_is_no_stranger() {
    // Windows gives a number again to a process that begins later: a build that ended
    // early in a long run, its number a user's program's by the time its window went.
    let mut tracker = Tracker::default();
    let a = window("p1-a", 1);
    tracker.saw(&a, &[process(10, "npm run build")]);
    // Seen in 2022, by its creation time.
    let shown_at = 1_655_526_500_000;
    tracker.identify(
        shown_at,
        Ok(&BTreeMap::from([(10, "133000000000000000".to_owned())])),
    );
    tracker.gone(&a, shown_at, &[]);
    assert!(began(&tracker, 10).is_some());
    let reused = BTreeMap::from([(10, "133000000099999999".to_owned())]);
    assert_eq!(
        tracker.strangers(shown_at + 2 * GRACE, &reused),
        Found::default()
    );
}

#[test]
fn an_identity_is_taken_by_the_look_that_saw_the_process_or_never() {
    // The look that saw the child could not ask who it was; its window went, and its
    // number is a user's program's by the next answer. That answer is never taken for
    // the child's: unresolved where its number is there, and never to be ended.
    let mut tracker = Tracker::default();
    let a = window("p1-a", 1);
    tracker.saw(&a, &[process(10, "npm run build")]);
    tracker.identify(1_000, Err("the asking ran out of its time"));
    tracker.gone(&a, 2_000, &[]);
    tracker.identify(3_000, Ok(&there(&[10])));
    assert_eq!(began(&tracker, 10), None);
    // Seen again by its window's tree, it is asked nothing.
    assert!(tracker.saw(&a, &[process(10, "npm run build")]).is_empty());
    let found = tracker.strangers(2_001 + GRACE, &there(&[10]));
    assert!(found.leftovers.is_empty(), "{found:?}");
    assert_eq!(found.unresolved.len(), 1);
    assert_eq!(found.unresolved[0].process.pid, 10);
    assert!(
        found.unresolved[0]
            .why
            .contains("could not say who it was at the look that saw it"),
        "{}",
        found.unresolved[0].why
    );
    assert!(found.unresolved[0].why.ends_with("it was left running"));
    // A number the system names nowhere had ended: followed no more, whoever it was.
    let mut tracker = Tracker::default();
    tracker.saw(&a, &[process(12, "sleep 1")]);
    tracker.identify(0, Ok(&BTreeMap::new()));
    assert!(tracker.followed().is_empty());
    let mut tracker = Tracker::default();
    tracker.saw(&a, &[process(13, "sleep 1")]);
    tracker.identify(0, Err("ps failed"));
    tracker.all_gone(0);
    assert_eq!(tracker.left(1, &BTreeMap::new()), Found::default());
}

#[test]
fn a_process_the_system_says_began_after_the_look_saw_its_number_is_never_known() {
    // Windows says to the moment when a process began, and its asking takes seconds:
    // the child seen at 10:00:00 ended, and the process the answer names under its
    // number began after the look saw it. Another's.
    let shown_at = 1_760_522_400_000_i64;
    let ticks = |at: i64| ((at + 11_644_473_600_000) * 10_000).to_string();
    let mut tracker = Tracker::default();
    let a = window("p1-a", 1);
    tracker.saw(&a, &[process(10, "node -e test")]);
    tracker.identify(
        shown_at,
        Ok(&BTreeMap::from([(10, ticks(shown_at + 4_000))])),
    );
    assert_eq!(began(&tracker, 10), None);
    // One that began before the look saw it is the one it saw.
    tracker.saw(&a, &[process(11, "npm test")]);
    tracker.identify(
        shown_at,
        Ok(&BTreeMap::from([(11, ticks(shown_at - 4_000))])),
    );
    assert_eq!(began(&tracker, 11), Some(ticks(shown_at - 4_000).as_str()));
}

#[test]
fn a_number_its_window_shows_again_begun_at_another_time_is_a_new_process_known_by_it() {
    // Child A of the window ended; child B, of the same window and generation, has its
    // number now. B is known by when it began, and found by it once the window went.
    let mut tracker = Tracker::default();
    let a = window("p1-a", 1);
    tracker.saw(&a, &[process(10, "npm test")]);
    tracker.identify(0, Ok(&BTreeMap::from([(10, "began A".to_owned())])));
    tracker.saw(&a, &[process(10, "node serve.mjs")]);
    tracker.identify(0, Ok(&BTreeMap::from([(10, "began B".to_owned())])));
    assert_eq!(began(&tracker, 10), Some("began B"));
    tracker.gone(&a, 1_000, &[]);
    let found = tracker.strangers(1_001 + GRACE, &BTreeMap::from([(10, "began B".to_owned())]));
    assert_eq!(leftovers(&found), [10]);
    assert_eq!(found.leftovers[0].process.command, "node serve.mjs");
    // One the system says began after the look began taking the trees may be another,
    // given the number since, or the one the tree showed a moment after: it is never
    // known, and unresolved where its number is there at the end.
    let shown_at = 1_760_522_400_000_i64;
    let ticks = |at: i64| ((at + 11_644_473_600_000) * 10_000).to_string();
    let mut tracker = Tracker::default();
    tracker.saw(&a, &[process(11, "npm test")]);
    tracker.identify(
        shown_at,
        Ok(&BTreeMap::from([(11, ticks(shown_at - 9_000))])),
    );
    tracker.saw(&a, &[process(11, "npm test")]);
    tracker.identify(
        shown_at + 2_000,
        Ok(&BTreeMap::from([(11, ticks(shown_at + 5_000))])),
    );
    assert_eq!(began(&tracker, 11), None);
    tracker.gone(&a, shown_at + 3_000, &[]);
    let found = tracker.strangers(
        shown_at + 3_001 + GRACE,
        &BTreeMap::from([(11, ticks(shown_at + 5_000))]),
    );
    assert!(found.leftovers.is_empty(), "{found:?}");
    assert!(found.unresolved[0]
        .why
        .contains("began after the look saw it"));
}

#[test]
fn a_number_its_windows_exit_listed_holding_a_process_begun_since_is_judged_by_when() {
    // The look knew A under 10; the window's exit listed 10 at `exit`, and the system
    // says the process there now began at another time than A.
    let exit = 1_760_522_400_000_i64;
    let ticks = |at: i64| ((at + 11_644_473_600_000) * 10_000).to_string();
    let a = window("p1-a", 1);
    let followed = |tracker: &mut Tracker| {
        tracker.saw(&a, &[process(10, "npm test")]);
        tracker.identify(
            exit - 30_000,
            Ok(&BTreeMap::from([(10, ticks(exit - 60_000))])),
        );
        tracker.gone(&a, exit, &[process(10, "node serve.mjs")]);
    };
    // Begun before the exit listed the number: the window's own, a stranger.
    let mut tracker = Tracker::default();
    followed(&mut tracker);
    let now = BTreeMap::from([(10, ticks(exit - 5_000))]);
    assert_eq!(leftovers(&tracker.strangers(exit + 1 + GRACE, &now)), [10]);
    // Begun after: another's, given the number since.
    let mut tracker = Tracker::default();
    followed(&mut tracker);
    let now = BTreeMap::from([(10, ticks(exit + 5_000))]);
    assert_eq!(tracker.strangers(exit + 1 + GRACE, &now), Found::default());
    // Said to the second alone: unresolved.
    let mut tracker = Tracker::default();
    followed(&mut tracker);
    let now = BTreeMap::from([(10, "Fri Oct 16 00:29:15 2026".to_owned())]);
    let found = tracker.strangers(exit + 1 + GRACE, &now);
    assert!(found.leftovers.is_empty());
    assert!(found.unresolved[0]
        .why
        .contains("its window's exit listed the number"));
}

#[test]
fn a_number_another_window_has_now_is_asked_again_who_it_is() {
    // Window B's process has a number window A's had: who it is is asked again, and
    // B's process, left after B went, is found by B's identity, not A's.
    let mut tracker = Tracker::default();
    let (a, b) = (window("p1-a", 1), window("p1-b", 1));
    tracker.saw(&a, &[process(10, "npm run build")]);
    tracker.identify(0, Ok(&BTreeMap::from([(10, "began then".to_owned())])));
    let new = tracker.saw(&b, &[process(10, "node serve.mjs")]);
    assert_eq!(new, [10], "asked again");
    tracker.identify(0, Ok(&BTreeMap::from([(10, "began now".to_owned())])));
    tracker.gone(&b, 0, &[]);
    let found = tracker.strangers(2 * GRACE, &BTreeMap::from([(10, "began now".to_owned())]));
    assert_eq!(leftovers(&found), [10]);
    assert_eq!(found.leftovers[0].process.command, "node serve.mjs");
}

#[test]
fn a_tree_taken_before_its_windows_exit_brings_nothing_back_but_another_window_does() {
    let mut tracker = Tracker::default();
    let a = window("p1-a", 1);
    tracker.saw(&a, &[process(10, "node dev-server.mjs")]);
    known(&mut tracker, &[10]);
    tracker.gone(&a, 1_000, &[]);
    // The look took the window's tree a moment before the exit was told.
    tracker.saw(&a, &[process(10, "node dev-server.mjs")]);
    assert_eq!(
        tracker.due(1_001 + GRACE),
        [10],
        "the window went, whatever its tree said"
    );
    // The window opened again (a new generation) and has it: no stranger.
    let again = window("p1-a", 2);
    tracker.saw(&again, &[process(10, "node dev-server.mjs")]);
    assert!(tracker.due(10 * GRACE).is_empty());
}

#[test]
fn what_only_a_windows_exit_names_is_followed_from_the_exit_and_never_known() {
    // A child begun between two looks, never in a tree a look took: the exit lists it,
    // by its number and command alone. Its number there a grace later is unresolved.
    let mut tracker = Tracker::default();
    let a = window("p1-a", 1);
    tracker.gone(&a, 5_000, &[process(30, "python3 -m http.server")]);
    tracker.identify(5_000, Ok(&there(&[30])));
    assert_eq!(began(&tracker, 30), None);
    let found = tracker.strangers(5_001 + GRACE, &there(&[30]));
    assert!(found.leftovers.is_empty(), "{found:?}");
    assert_eq!(
        found.unresolved[0].process.command,
        "python3 -m http.server"
    );
    assert_eq!(found.unresolved[0].since, 5_000);
    assert!(found.unresolved[0]
        .why
        .contains("first known from its window's exit"));
    // One its window's looks showed keeps what the look took.
    let mut tracker = Tracker::default();
    tracker.saw(&a, &[process(31, "node serve.mjs")]);
    known(&mut tracker, &[31]);
    tracker.gone(&a, 5_000, &[process(31, "node serve.mjs")]);
    assert_eq!(
        leftovers(&tracker.strangers(5_001 + GRACE, &there(&[31]))),
        [31]
    );
}

#[test]
fn at_the_end_every_process_still_there_is_left_whatever_its_grace() {
    let mut tracker = Tracker::default();
    tracker.saw(&window("p1-a", 1), &[process(10, "node dev-server.mjs")]);
    tracker.saw(&window("p1-b", 1), &[process(20, "npm test")]);
    known(&mut tracker, &[10, 20]);
    tracker.all_gone(7_000);
    assert_eq!(tracker.followed(), [10, 20]);
    let found = tracker.left(7_500, &there(&[20]));
    assert_eq!(leftovers(&found), [20]);
    assert!(tracker.followed().is_empty());
}

#[test]
fn at_the_end_what_the_system_could_not_be_asked_of_is_unresolved_and_said() {
    let mut tracker = Tracker::default();
    tracker.saw(&window("p1-a", 1), &[process(10, "node dev-server.mjs")]);
    known(&mut tracker, &[10]);
    tracker.all_gone(7_000);
    let unresolved = tracker.unresolved_all(7_500, "ps ran out of its time");
    assert_eq!(unresolved.len(), 1);
    assert_eq!(
        (unresolved[0].process.pid, unresolved[0].since),
        (10, 7_000)
    );
    assert!(unresolved[0].why.ends_with("ps ran out of its time"));
    assert!(tracker.followed().is_empty());
}
