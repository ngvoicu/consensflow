//! D4's P4-B boundary tests R1 to R13 (T-2090): the same inputs and expected outputs on both
//! heads, in terms of documents, epochs, windows, numbers and bytes in, and written bytes,
//! pending notices, what the page may do and confirmations out. A ticket, an event or a
//! callback alone never satisfies one. The head's own half is `adapter`.

mod adapter;

use super::*;
use adapter::{Doc, Notice, Rig};

fn w5() -> (&'static str, u64) {
    ("W", 5)
}

fn names(notices: &[Notice], key: &PaneKey) -> bool {
    notices.iter().any(|notice| &notice.key == key)
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap()
}

/// R1. The old page's first key, still in transit when the page reloads, is never written
/// after the new page's, and never silently: the window's loss survives to the current page.
#[test]
fn r1_an_old_first_call_in_transit_never_overtakes_the_new_pages() {
    for middle_outage in [false, true] {
        let rig = Rig::new();
        let w = rig.window(w5().0, w5().1);
        rig.navigate();
        let old = rig.open_page("s:1");
        rig.navigate();
        if middle_outage {
            // A document that prepared, and whose activation was never answered.
            rig.prepare("s:2");
            rig.navigate();
        }
        let newest = rig.open_page(if middle_outage { "s:3" } else { "s:2" });
        rig.input(&newest, &w, 1, b"D").unwrap();
        let late = rig.input(&old, &w, 1, b"A");
        assert!(late.is_err(), "the old page's first call is refused: {late:?}");
        assert_eq!(
            text(rig.written(&w)),
            "D",
            "outage={middle_outage}: only the newest page's key is written, with no wait for A"
        );
        assert!(
            names(&rig.pull(&newest).unwrap(), &w.key),
            "outage={middle_outage}: the window's loss survives to the current page"
        );
    }
    // The old call arrives while nobody is active.
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let old = rig.open_page("s:1");
    rig.navigate();
    assert!(rig.input(&old, &w, 1, b"A").is_err());
    assert_eq!(text(rig.written(&w)), "");
    let newest = rig.open_page("s:2");
    assert!(names(&rig.pull(&newest).unwrap(), &w.key));
    // Control: the old call was committed before the reload, so it is written, in order, and
    // nothing is told.
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let old = rig.open_page("s:1");
    rig.input(&old, &w, 1, b"A").unwrap();
    rig.navigate();
    let newest = rig.open_page("s:2");
    rig.input(&newest, &w, 1, b"D").unwrap();
    assert_eq!(text(rig.written(&w)), "AD");
    assert!(rig.pull(&newest).unwrap().is_empty());
}

/// R2, the protocol: the host answers a preparation with its own epoch for that document.
#[test]
fn r2_the_host_answers_a_preparation_with_its_epoch() {
    let rig = Rig::new();
    rig.navigate();
    let doc = rig.prepare("s:1");
    assert!(
        doc.epoch.is_some(),
        "the host numbers each main document and answers the preparing page with it"
    );
    rig.activate(&doc).unwrap();
    rig.navigate();
    let next = rig.prepare("s:2");
    assert!(next.epoch > doc.epoch, "a later document has a later epoch");
}

/// R2. No stale document can seize ownership, move the epoch, replace the output
/// subscription or spend the current page's number 1; each of its calls leaves a notice.
#[test]
fn r2_a_stale_document_cannot_seize_the_page_or_spend_its_numbers() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let old = rig.prepare("a:1");
    let held_second_prepare = old.clone();
    // Two reloads, to the same URL, the second page with a fresh identity of its own.
    rig.navigate();
    rig.navigate();
    // The held second preparation and the old activation arrive after navigation.
    let _ = rig.try_prepare(&held_second_prepare.page);
    assert!(rig.activate(&old).is_err(), "a stale activation seizes nothing");
    let newest = rig.prepare("b:1");
    assert!(rig.activate(&old).is_err(), "nor before the newest page's own");
    rig.activate(&newest).unwrap();
    // Q's first activation answer was lost: asked again, the same answer.
    rig.activate(&newest).unwrap();
    assert!(rig.activate(&old).is_err(), "nor after it");
    let epoch = rig.host_epoch();
    // Old, unbound and future epochs, and a wrong page at the current epoch.
    let stale = [
        old.clone(),
        old.claiming(newest.epoch.unwrap()),
        Doc::unbound("a:1", newest.epoch),
        newest.claiming(newest.epoch.unwrap() + 9),
    ];
    for doc in &stale {
        assert!(
            rig.input(doc, &w, 1, b"X").is_err(),
            "a stale call is refused: {doc:?}"
        );
    }
    rig.input(&newest, &w, 1, b"D").unwrap();
    assert_eq!(text(rig.written(&w)), "D", "the newest page's number 1 writes once");
    assert!(!rig.may_subscribe(&old), "a stale page does not replace the subscription");
    assert!(rig.may_subscribe(&newest));
    assert_eq!(rig.host_epoch(), epoch, "no stale call moves the epoch");
    assert!(names(&rig.pull(&newest).unwrap(), &w.key), "each leaves a recoverable notice");
}

/// R3. Until a document is activated nothing it sends is enqueued; after it is, its first
/// key is number 1 and writes once.
#[test]
fn r3_nothing_is_enqueued_before_the_documents_activation() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let doc = rig.prepare("s:1");
    for attempt in 1..=3 {
        assert!(rig.input(&doc, &w, attempt, b"k").is_err());
    }
    assert!(rig.reply(&doc, &w, 4, b"r").is_err());
    assert_eq!(text(rig.written(&w)), "", "zero input is enqueued before the acknowledgement");
    rig.activate(&doc).unwrap();
    // The page's own gate keeps these from being sent at all (the page suite); what the host
    // refused before is still owed to the person.
    let owed = rig.pull(&doc).unwrap();
    assert!(names(&owed, &w.key), "nothing a refused call carried vanishes unsaid");
    rig.input(&doc, &w, 1, b"D").unwrap();
    assert_eq!(text(rig.written(&w)), "D");
}

/// R4. Calls arrive in WebView2's order, 1 4 2 5 3.
#[test]
fn r4_the_webview2_order_is_written_once_and_a_reload_cancels_what_is_parked() {
    let order = [1_u64, 4, 2, 5, 3];
    let byte = |number: u64| [b'A' + (number - 1) as u8];
    // First run: completes before navigation.
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let old = rig.open_page("s:1");
    for number in order {
        rig.input(&old, &w, number, &byte(number)).unwrap();
    }
    rig.navigate();
    let next = rig.open_page("s:2");
    assert_eq!(text(rig.written(&w)), "ABCDE", "exactly once, in order");
    assert!(rig.pull(&next).unwrap().is_empty());
    // Second run: A, D, B observed; E parked behind a missing 3; the reload; Q's F; then the
    // old page's 3 and its unload call 6.
    for deliver_three in [true, false] {
        let rig = Rig::new();
        let w = rig.window("W", 5);
        rig.navigate();
        let old = rig.open_page("s:1");
        for number in [1_u64, 4, 2, 5] {
            rig.input(&old, &w, number, &byte(number)).unwrap();
        }
        assert_eq!(text(rig.written(&w)), "AB");
        rig.navigate();
        let next = rig.open_page("s:2");
        rig.input(&next, &w, 1, b"F").unwrap();
        assert_eq!(text(rig.written(&w)), "F", "Q's call is not held behind the old page's");
        if deliver_three {
            assert!(rig.input(&old, &w, 3, b"C").is_err());
        }
        assert!(rig.input(&old, &w, 6, b"U").is_err(), "an unload call is refused and told");
        assert_eq!(text(rig.written(&w)), "", "no late arrival reaches the writer");
        assert!(names(&rig.pull(&next).unwrap(), &w.key), "the parked calls and the gap are told");
    }
    // The limit: a trailing unload call that never reaches the host has no number after it and
    // no skip, and nothing can be inferred of it.
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let old = rig.open_page("s:1");
    for number in 1..=3_u64 {
        rig.input(&old, &w, number, &byte(number)).unwrap();
    }
    rig.navigate();
    let next = rig.open_page("s:2");
    assert_eq!(text(rig.written(&w)), "ABC");
    assert!(rig.pull(&next).unwrap().is_empty(), "a call that never arrived cannot be told");
}

/// R5, in the lane: number 3 at 0 ms, number 2 at 1,000 ms, number 1 never; each call has its
/// own deadline, and the page's absence changes nothing.
#[test]
fn r5_each_call_waits_its_own_bound_and_the_gap_is_told() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let page = rig.open_page("s:1");
    rig.input(&page, &w, 3, b"C").unwrap();
    thread::sleep(Duration::from_millis(1000));
    rig.input(&page, &w, 2, b"B").unwrap();
    assert_eq!(text(rig.written(&w)), "");
    thread::sleep(ORDER_BOUND - Duration::from_millis(1000) + Duration::from_millis(250));
    assert_eq!(text(rig.written(&w)), "BC", "by C's own deadline");
    // The page could not receive events or answers meanwhile: the notice is there at recovery.
    assert!(names(&rig.pull(&page).unwrap(), &w.key));
}

/// R5, the cut: at 1,200 ms the page reloads, and neither call writes.
#[test]
fn r5_a_cut_before_the_bound_writes_neither_parked_call_and_tells_the_gap() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let old = rig.open_page("s:1");
    rig.input(&old, &w, 3, b"C").unwrap();
    thread::sleep(Duration::from_millis(1000));
    rig.input(&old, &w, 2, b"B").unwrap();
    thread::sleep(Duration::from_millis(200));
    rig.navigate();
    let next = rig.open_page("s:2");
    thread::sleep(ORDER_BOUND + Duration::from_millis(250));
    assert_eq!(text(rig.written(&w)), "", "neither B nor C writes");
    assert!(names(&rig.pull(&next).unwrap(), &w.key));
}

/// R5, the known gap: number 2 is refused on decode and its warning is shown and confirmed,
/// number 1 is still missing, and the page reloads before the bound. A new notice is owed.
#[test]
fn r5_a_known_missing_number_owes_a_fresh_notice_after_an_earlier_one_was_shown() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let old = rig.open_page("s:1");
    let first = rig.refuse_decode(&old, &w, 2).unwrap();
    rig.told(&old, &[(w.key.clone(), first)]).unwrap();
    assert!(rig.pull(&old).unwrap().is_empty(), "the first warning was shown and confirmed");
    rig.navigate();
    let next = rig.open_page("s:2");
    let owed = rig.pull(&next).unwrap();
    assert!(
        owed.iter().any(|notice| notice.key == w.key && notice.loss > first),
        "a new pending notice for the missing number 1: {owed:?}"
    );
}

/// R6, the allowance: two large pastes parked behind a missing number never write after the
/// cut, and their reservations are released at it.
#[test]
fn r6_parked_pastes_are_cancelled_at_the_cut_and_their_allowance_released() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let old = rig.open_page("s:1");
    let paste = vec![b'p'; 4 * 1024 * 1024 - 64];
    rig.input(&old, &w, 3, &paste).unwrap();
    rig.input(&old, &w, 2, &paste).unwrap();
    assert!(rig.reserved(&w) > 7 * 1024 * 1024);
    rig.navigate();
    let next = rig.open_page("s:2");
    assert_eq!(rig.reserved(&w), 0, "their reservations are released at the cut");
    rig.input(&next, &w, 1, &paste).unwrap();
    assert_eq!(rig.written(&w).len(), paste.len(), "only the new page's paste is written");
    assert!(names(&rig.pull(&next).unwrap(), &w.key));
}

/// R7. A window that closes while its call waits in order: the call never writes, and the
/// notice names the closed window on the current page.
#[test]
fn r7_a_window_closed_with_a_call_waiting_is_named_on_the_current_page() {
    for retired_after_navigation in [false, true] {
        let rig = Rig::new();
        let w = rig.window("W", 5);
        rig.navigate();
        let old = rig.open_page("s:1");
        let ticket = rig.input(&old, &w, 2, b"B").unwrap();
        if retired_after_navigation {
            rig.navigate();
            rig.retire(&w);
        } else {
            rig.retire(&w);
            assert!(rig.wait(&old, &ticket).is_some(), "the page's wait is answered, not left hanging");
            rig.navigate();
        }
        let next = rig.open_page("s:2");
        assert_eq!(text(rig.written(&w)), "", "B is never written");
        let owed = rig.pull(&next).unwrap();
        assert!(
            owed.iter().any(|notice| notice.key == w.key && notice.closed),
            "retired after navigation={retired_after_navigation}: the closed window is named: {owed:?}"
        );
    }
    // A successful call followed by the window's retirement produces no warning of its own.
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let page = rig.open_page("s:1");
    rig.input(&page, &w, 1, b"A").unwrap();
    rig.retire(&w);
    assert!(rig.pull(&page).unwrap().is_empty());
}

fn fill_the_writer(rig: &Rig, doc: &Doc, w: &adapter::Window) {
    for number in 1..=INPUT_QUEUE_CAPACITY as u64 {
        rig.input(doc, w, number, b"x").unwrap();
    }
}

/// R8. Every exit that refuses a call before its bytes reach the writer leaves a numbered loss
/// for the exact window, which the page can pull after the answer was lost to a reload, and
/// the writer got exactly what was committed and nothing else.
#[test]
fn r8_every_refusal_exit_leaves_a_loss_for_the_surviving_page() {
    /// The call's answer, the window the notice must name, and the bytes W's writer must have.
    type Outcome = (Result<String, adapter::Refused>, PaneKey, Vec<u8>);
    type Setup = fn(&Rig, &Doc, &adapter::Window) -> Outcome;
    let exits: [(&str, Setup); 6] = [
        ("a full writer queue refuses the call at once", |rig, doc, w| {
            fill_the_writer(rig, doc, w);
            let refused = rig.input(doc, w, INPUT_QUEUE_CAPACITY as u64 + 1, b"K");
            (refused, w.key.clone(), vec![b'x'; INPUT_QUEUE_CAPACITY])
        }),
        ("a parked call pumped into a full writer queue", |rig, doc, w| {
            fill_the_writer(rig, doc, w);
            let n = INPUT_QUEUE_CAPACITY as u64;
            rig.input(doc, w, n + 2, b"K").unwrap();
            // Its number comes: the call before it takes the order past it into the full queue.
            let refused = rig.input(doc, w, n + 1, b"J");
            assert!(refused.is_err(), "the call that completes the order is refused: {refused:?}");
            (refused, w.key.clone(), vec![b'x'; INPUT_QUEUE_CAPACITY])
        }),
        ("a human's bytes past the window's allowance", |rig, doc, w| {
            let paste = vec![b'p'; 4 * 1024 * 1024 - 64];
            rig.input(doc, w, 1, &paste).unwrap();
            rig.input(doc, w, 2, &paste).unwrap();
            let refused = rig.input(doc, w, 3, &vec![b'q'; 200]);
            (refused, w.key.clone(), [paste.clone(), paste].concat())
        }),
        ("a reply past the replies' allowance", |rig, doc, w| {
            let refused = rig.reply(doc, w, 1, &vec![b'r'; 64 * 1024 + 1]);
            (refused, w.key.clone(), Vec::new())
        }),
        ("a window with no writer queue", |rig, doc, w| {
            let gone = rig.window("gone", 1);
            let key = rig.writer_queue_gone(gone);
            rig.remove_route(&key);
            let refused = rig.input_for(doc, &key, 1, b"K");
            let _ = w;
            (refused, key, Vec::new())
        }),
        ("a writer queue that has closed", |rig, doc, w| {
            let closed = rig.window("closed", 1);
            let key = rig.writer_queue_gone(closed);
            let refused = rig.input_for(doc, &key, 1, b"K");
            let _ = w;
            (refused, key, Vec::new())
        }),
    ];
    for (name, setup) in exits {
        let rig = Rig::new();
        let w = rig.window("W", 5);
        rig.navigate();
        let old = rig.open_page("s:1");
        let (result, named, expected) = setup(&rig, &old, &w);
        assert!(
            result.as_ref().is_err_and(|(_, loss)| loss.is_some()),
            "{name}: the refusal is told to the caller, numbered: {result:?}"
        );
        // The refusal answer is lost to the reload: the surviving page pulls the loss.
        rig.navigate();
        let next = rig.open_page("s:2");
        let owed = rig.pull(&next).unwrap();
        assert_eq!(
            owed.iter().map(|notice| notice.key.clone()).collect::<Vec<_>>(),
            vec![named],
            "{name}: one numbered notice, for the exact window: {owed:?}"
        );
        assert!(
            Some(owed[0].loss) >= result.unwrap_err().1,
            "{name}: and it is the caller's loss or a later one for the same window"
        );
        assert_eq!(rig.written(&w), expected, "{name}: the writer got exactly what was committed");
    }
}

/// Finding 6 (R10). A confirmation the old document makes as the page reloads, with the
/// navigation between the host's two steps on the earlier head, clears nothing the next page
/// has not seen.
#[test]
fn r10_a_confirmation_across_a_navigation_clears_nothing_unseen() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let old = rig.open_page("s:1");
    let loss = rig.skip(&old, &w, 1).unwrap();
    rig.told_across_navigation(&old, &[(w.key.clone(), loss)]);
    let next = rig.open_page("s:2");
    assert!(names(&rig.pull(&next).unwrap(), &w.key), "the next page has not seen it yet");
}

/// R9. The old page's timeout skip for its missing number 1 arrives after the reload: only the
/// new page's D is written, its ticket still resolves, the original can never write, and the
/// old loss survives.
#[test]
fn r9_a_delayed_skip_and_its_original_cannot_touch_the_new_pages_numbers() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let old = rig.open_page("s:1");
    rig.input(&old, &w, 2, b"B").unwrap();
    rig.navigate();
    let next = rig.open_page("s:2");
    let ticket = rig.input(&next, &w, 1, b"D").unwrap();
    assert!(rig.skip(&old, &w, 1).is_err(), "the delayed skip is no skip of the new page's");
    assert!(rig.skip(&old, &w, 1).is_err(), "nor its retry");
    assert!(rig.input(&old, &w, 1, b"A").is_err(), "the original comes late");
    assert_eq!(text(rig.written(&w)), "D");
    assert!(
        matches!(rig.wait(&next, &ticket), Some(Ok(Ok(())))),
        "the new page's ticket still resolves"
    );
    assert!(names(&rig.pull(&next).unwrap(), &w.key), "the old loss survives");
}

/// R9, capacity: with the page's numbers full, a skip for an absent number is not final until
/// its number is spent, and a retried skip makes no new notice.
#[test]
fn r9_a_skip_is_final_only_once_its_number_is_spent() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let page = rig.open_page("s:1");
    let slots = MAX_LANE_SLOTS as u64;
    for number in 2..=slots + 1 {
        rig.input(&page, &w, number, b"x").unwrap();
    }
    // The page's spent numbers are kept too, up to as many again as it parks.
    for number in slots + 2..=2 * slots + 1 {
        rig.skip(&page, &w, number).unwrap();
    }
    let absent = 2 * slots + 2;
    let first = rig.skip(&page, &w, absent);
    assert!(
        first.is_err(),
        "a skip that cannot spend its number is a retry, never a final loss: {first:?}"
    );
    // The numbers' own bounds pass and the order drains: the page's retry is then final.
    thread::sleep(ORDER_BOUND + Duration::from_millis(300));
    let spent = rig.skip(&page, &w, absent).expect("the retry is final");
    assert_eq!(rig.skip(&page, &w, absent), Ok(spent), "a retry makes no new notice");
    assert!(rig.input(&page, &w, absent, b"Z").is_err(), "the original never writes");
    assert!(!rig.written(&w).contains(&b'Z'));
    let owed = rig.pull(&page).unwrap();
    assert_eq!(owed.iter().filter(|notice| notice.key == w.key).count(), 1);
}

/// R10. A notice is cleared only by the current document, for the version it showed. A newer
/// version of the window's loss survives the confirmation of the older one, two windows are
/// confirmed apart, and the departed document's confirmation clears nothing.
#[test]
fn r10_only_the_current_document_clears_the_version_it_showed() {
    let rig = Rig::new();
    let w1 = rig.window("W1", 1);
    let w2 = rig.window("W2", 1);
    rig.navigate();
    let old = rig.open_page("s:1");
    let one = rig.skip(&old, &w1, 1).unwrap();
    let two = rig.skip(&old, &w2, 1).unwrap();
    let seen = rig.pull(&old).unwrap();
    assert_eq!(seen.len(), 2, "two windows, two notices");
    rig.navigate();
    let next = rig.open_page("s:2");
    // The departed document's confirmation arrives: it clears nothing.
    let _ = rig.told(&old, &[(w1.key.clone(), one), (w2.key.clone(), two)]);
    assert_eq!(rig.pull(&next).unwrap().len(), 2);
    // A newer loss of W1 comes while the older version is being confirmed.
    let newer = rig.skip(&next, &w1, 1).unwrap();
    assert!(newer > one);
    rig.told(&next, &[(w1.key.clone(), one), (w2.key.clone(), two)]).unwrap();
    let left = rig.pull(&next).unwrap();
    assert_eq!(left.len(), 1, "{left:?}");
    assert_eq!((left[0].key.clone(), left[0].loss), (w1.key.clone(), newer));
    // The known version, asked again, is the same notice (the page shows it once).
    assert_eq!(rig.pull(&next).unwrap(), left);
}

/// R11. A stale call is refused and told whatever the age of its page, and the losses past the
/// host's bound are counted, with a version of their own, never dropped.
#[test]
fn r11_a_stale_call_is_told_at_any_age_and_overflow_is_counted_apart() {
    for departures in [32_u64, 33, 40] {
        let rig = Rig::new();
        let w = rig.window("W", 5);
        rig.navigate();
        let old = rig.open_page("s:1");
        for index in 2..=departures + 1 {
            rig.navigate();
            rig.open_page(&format!("s:{index}"));
        }
        rig.navigate();
        let current = rig.open_page(&format!("s:{}", departures + 2));
        assert!(rig.input(&old, &w, 1, b"A").is_err());
        assert!(
            names(&rig.pull(&current).unwrap(), &w.key),
            "after {departures} departures the stale call is still told"
        );
    }
    // Eight pages that never activated, a parked call before them: told on recovery.
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let old = rig.open_page("s:1");
    rig.input(&old, &w, 2, b"B").unwrap();
    for index in 2..=9 {
        rig.navigate();
        rig.prepare(&format!("s:{index}"));
    }
    rig.navigate();
    let recovered = rig.open_page("s:10");
    assert!(names(&rig.pull(&recovered).unwrap(), &w.key));
    // 512, then 513 unconfirmed distinct windows.
    let rig = Rig::new();
    rig.navigate();
    let page = rig.open_page("s:1");
    let first = PaneKey::new("window-0", 1);
    for index in 0..MAX_PENDING_LOSSES {
        rig.skip_key(&page, &PaneKey::new(format!("window-{index}"), 1), 1).unwrap();
    }
    assert_eq!(rig.pull(&page).unwrap().len(), MAX_PENDING_LOSSES);
    assert_eq!(rig.overflow(&page), None);
    rig.skip_key(&page, &PaneKey::new("window-512", 1), 1).unwrap();
    let shown = rig.pull(&page).unwrap();
    assert_eq!(shown.len(), MAX_PENDING_LOSSES);
    assert!(!names(&shown, &first), "the oldest is represented by the aggregate");
    let (version, count) = rig.overflow(&page).expect("the first overflow is counted");
    assert_eq!(count, 1);
    // One more is evicted while the aggregate's confirmation is in flight.
    rig.skip_key(&page, &PaneKey::new("window-513", 1), 1).unwrap();
    // A window's notice confirmed apart does not confirm the count.
    let confirmed: Vec<_> = shown.iter().take(1).map(|n| (n.key.clone(), n.loss)).collect();
    rig.told(&page, &confirmed).unwrap();
    assert!(rig.overflow(&page).is_some(), "the count is confirmed on its own");
    rig.told_overflow(&page, version, count).unwrap();
    let (later, left) = rig.overflow(&page).expect("the further eviction survives");
    assert_eq!(left, 1, "the confirmation of that version leaves the one further record");
    assert!(later > version);
    rig.told_overflow(&page, later, left).unwrap();
    assert_eq!(rig.overflow(&page), None);
}

/// R12. A write that fails before any byte, after a prefix, or whose worker vanishes leaves an
/// outcome the host owns, whatever becomes of the page's wait, before or after the cut.
#[test]
fn r12_a_failed_or_vanished_write_leaves_a_host_owned_outcome() {
    type Fate = fn(&Rig, &adapter::Window);
    let fates: [(&str, Fate); 3] = [
        ("fails before writing", |rig, w| {
            rig.writer_finishes(w, Err(InputError::refused("stale-pane", "the pane is gone")))
        }),
        ("fails after a prefix", |rig, w| {
            rig.writer_finishes(w, Err(InputError::Uncertain("the write stopped half way".into())))
        }),
        ("vanishes without answering", |rig, w| rig.writer_vanishes(w)),
    ];
    for (name, fate) in fates {
        for after_the_cut in [false, true] {
            let rig = Rig::new();
            let w = rig.window("W", 5);
            rig.navigate();
            let page = rig.open_page("s:1");
            let ticket = rig.input(&page, &w, 1, b"A").unwrap();
            if after_the_cut {
                rig.navigate();
            }
            fate(&rig, &w);
            // The page never fetched its wait: nothing but the host's record can tell.
            let current = if after_the_cut { rig.open_page("s:2") } else { page.clone() };
            let _ = ticket;
            let owed = rig.pull(&current).unwrap();
            assert!(
                names(&owed, &w.key),
                "{name}, after the cut={after_the_cut}: the outcome is recorded: {owed:?}"
            );
        }
    }
    // A prefix is never replayed: the failed call's number stays spent.
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let page = rig.open_page("s:1");
    rig.input(&page, &w, 1, b"AB").unwrap();
    rig.writer_finishes(&w, Err(InputError::Uncertain("half".into())));
    assert!(rig.input(&page, &w, 1, b"AB").is_err(), "the same number is not written again");
    assert_eq!(text(rig.written(&w)), "");
}

// Round 2 (T-2134's findings): each is a boundary the built head did not hold.

/// Finding 2. A refusal on the idle path keeps the gap's deadline: number 1 is missing, a valid
/// number 2 is refused for the window's byte budget (two committed 4 MiB pastes), and the gap
/// still raises its clock and its warning.
#[test]
fn f2_an_idle_refusal_keeps_the_gaps_deadline() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let page = rig.open_page("s:1");
    let paste = vec![b'p'; crate::validation::MAX_PAGE_INPUT_BYTES];
    // Two committed pastes hold the whole allowance (the test's writer has not drained them).
    rig.input(&page, &w, 1, &paste).unwrap();
    rig.input(&page, &w, 2, &paste).unwrap();
    rig.navigate();
    let next = rig.open_page("s:2");
    let refused = rig.input(&next, &w, 2, b"K").unwrap_err();
    let first = refused.1.expect("the refusal is numbered");
    rig.told(&next, &[(w.key.clone(), first)]).unwrap();
    thread::sleep(ORDER_BOUND + Duration::from_millis(250));
    let owed = rig.pull(&next).unwrap();
    assert!(
        owed.iter().any(|notice| notice.key == w.key && notice.loss > first),
        "the known gap below the refused number is told when its bound passes: {owed:?}"
    );
}

/// Finding 2. A bounds clock that cannot start never leaves a parked key unwritten, its
/// ticket unresolved or its bytes reserved: it ends in a numbered loss.
#[test]
fn f2_a_clock_that_cannot_start_leaves_nothing_parked() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let page = rig.open_page("s:1");
    rig.refuse_threads(true);
    let ticket = rig.input(&page, &w, 2, b"B").unwrap();
    rig.refuse_threads(false);
    assert_eq!(text(rig.written(&w)), "B", "the key is not left unwritten");
    assert!(matches!(rig.wait(&page, &ticket), Some(Ok(Ok(())))), "its ticket resolves");
    assert_eq!(rig.reserved(&w), 0, "no byte stays reserved");
    assert!(names(&rig.pull(&page).unwrap(), &w.key), "the missing number is a numbered loss");
}

/// Finding 3. A skip confirmed final removes the parked job: `A` after a confirmed skip of
/// `B` writes `A` only.
#[test]
fn f3_a_final_skip_removes_its_parked_job() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let page = rig.open_page("s:1");
    rig.input(&page, &w, 2, b"B").unwrap();
    rig.skip(&page, &w, 2).unwrap();
    rig.input(&page, &w, 1, b"A").unwrap();
    assert_eq!(text(rig.written(&w)), "A");
}

/// Finding 3. At the slot limit a refused number stays spent: it is not admitted once the lane
/// has drained.
#[test]
fn f3_a_refused_number_at_the_limit_stays_spent() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let page = rig.open_page("s:1");
    for number in 2..=MAX_LANE_SLOTS as u64 + 1 {
        rig.input(&page, &w, number, b"x").unwrap();
    }
    let absent = MAX_LANE_SLOTS as u64 + 2;
    assert!(rig.input(&page, &w, absent, b"K").is_err(), "the lane is full");
    thread::sleep(ORDER_BOUND + Duration::from_millis(300));
    assert!(rig.input(&page, &w, absent, b"K").is_err(), "its number was spent by the refusal");
    assert!(!rig.written(&w).contains(&b'K'));
}

/// Finding 4. A committed write that fails after its window retired is reported closed.
#[test]
fn f4_a_failed_write_after_retirement_names_the_closed_window() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let page = rig.open_page("s:1");
    rig.input(&page, &w, 1, b"A").unwrap();
    rig.retire(&w);
    rig.writer_finishes(&w, Err(InputError::Uncertain("the window ended".into())));
    let owed = rig.pull(&page).unwrap();
    assert!(owed.iter().any(|notice| notice.key == w.key && notice.closed), "{owed:?}");
}

/// Finding 4. A notice whose wording changes (open, then closed) changes its version, so the
/// receipt of the wording the person saw cannot clear the one they did not.
#[test]
fn f4_a_changed_notice_has_a_new_version() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let page = rig.open_page("s:1");
    let open = rig.skip(&page, &w, 1).unwrap();
    rig.retire(&w);
    let owed = rig.pull(&page).unwrap();
    assert!(owed.iter().any(|n| n.key == w.key && n.closed && n.loss > open), "{owed:?}");
    rig.told(&page, &[(w.key.clone(), open)]).unwrap();
    assert!(names(&rig.pull(&page).unwrap(), &w.key), "the old receipt clears nothing unseen");
}

/// Finding 5. The count past the bound: a repeated receipt clears nothing it did not show, and
/// the same windows evicted twice are counted once.
#[test]
fn f5_overflow_receipts_match_exactly_and_count_windows_once() {
    let rig = Rig::new();
    rig.navigate();
    let page = rig.open_page("s:1");
    for index in 0..=MAX_PENDING_LOSSES {
        rig.skip_key(&page, &PaneKey::new(format!("window-{index}"), 1), 1).unwrap();
    }
    let (first, count) = rig.overflow(&page).expect("one evicted");
    assert_eq!(count, 1);
    rig.skip_key(&page, &PaneKey::new("window-513", 1), 1).unwrap();
    let (second, count) = rig.overflow(&page).unwrap();
    assert_eq!(count, 2);
    rig.told_overflow(&page, first, 1).unwrap();
    assert_eq!(rig.overflow(&page).map(|(_, count)| count), Some(1));
    rig.told_overflow(&page, first, 1).unwrap();
    assert_eq!(
        rig.overflow(&page).map(|(version, count)| (version >= second, count)),
        Some((true, 1)),
        "the repeated receipt clears no unseen eviction"
    );
    // The same windows again (their losses evict each other): still the same distinct windows.
    for index in 0..=MAX_PENDING_LOSSES + 1 {
        rig.skip_key(&page, &PaneKey::new(format!("window-{index}"), 1), 2).unwrap();
    }
    let (_, distinct) = rig.overflow(&page).unwrap();
    assert!(distinct <= 3, "two rounds through the same windows are not counted twice: {distinct}");
}

/// Finding 6. A receipt clears the exact version it showed, for its own window only.
#[test]
fn f6_a_receipt_clears_the_exact_version_for_its_own_window() {
    let rig = Rig::new();
    let w1 = rig.window("W1", 1);
    let w2 = rig.window("W2", 1);
    rig.navigate();
    let page = rig.open_page("s:1");
    let one = rig.skip(&page, &w1, 1).unwrap();
    let two = rig.skip(&page, &w2, 1).unwrap();
    assert!(two > one);
    rig.told(&page, &[(w1.key.clone(), two)]).unwrap();
    assert!(names(&rig.pull(&page).unwrap(), &w1.key), "X's higher version does not clear W");
    rig.told(&page, &[(w1.key.clone(), one)]).unwrap();
    assert!(!names(&rig.pull(&page).unwrap(), &w1.key));
    assert!(names(&rig.pull(&page).unwrap(), &w2.key));
}

/// Finding 7. A writer that goes while the state lock is held still gives its call a loss
/// number, and one that cannot start its helper still leaves a notice.
#[test]
fn f7_a_writer_gone_with_the_lock_held_still_gets_its_number_and_notice() {
    for helper_fails in [false, true] {
        let rig = Rig::new();
        let w = rig.window("W", 5);
        rig.navigate();
        let page = rig.open_page("s:1");
        let ticket = rig.input(&page, &w, 1, b"A").unwrap();
        let completion = rig.completion(&page, &ticket);
        rig.refuse_threads(helper_fails);
        {
            let _held = rig.hold_state();
            rig.writer_vanishes(&w);
            assert!(
                completion.loss().is_some(),
                "helper fails={helper_fails}: the wait finds its loss number at once"
            );
        }
        rig.refuse_threads(false);
        rig.navigate();
        let next = rig.open_page("s:2");
        thread::sleep(Duration::from_millis(100));
        assert!(names(&rig.pull(&next).unwrap(), &w.key), "helper fails={helper_fails}");
    }
}

/// Finding 7. A timed-out call whose skip was confirmed, and whose writer then fails, makes one
/// loss and not two.
#[test]
fn f7_a_skipped_call_whose_writer_then_fails_makes_one_loss() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    let page = rig.open_page("s:1");
    rig.input(&page, &w, 1, b"A").unwrap();
    let skip = rig.skip(&page, &w, 1).unwrap();
    rig.told(&page, &[(w.key.clone(), skip)]).unwrap();
    rig.writer_finishes(&w, Err(InputError::Uncertain("half".into())));
    assert!(rig.pull(&page).unwrap().is_empty(), "the skip's confirmed loss was this call's");
}

/// Finding 8. A surviving host recovers a poisoned admission lock: navigation and the next
/// document's handshake still work.
#[test]
fn f8_a_poisoned_lock_is_recovered() {
    let rig = Rig::new();
    let w = rig.window("W", 5);
    rig.navigate();
    rig.open_page("s:1");
    rig.poison_state();
    rig.navigate();
    let next = rig.open_page("s:2");
    rig.input(&next, &w, 1, b"D").unwrap();
    assert_eq!(text(rig.written(&w)), "D");
}

/// Finding 9. A cut keeps no departed identity: a hundred reloads leave the host holding a
/// handful of identities, not a hundred.
#[test]
fn f9_cuts_keep_no_departed_identities() {
    let rig = Rig::new();
    rig.navigate();
    rig.open_page("s:1");
    for index in 2..=101 {
        rig.navigate();
        rig.open_page(&format!("s:{index}"));
    }
    assert!(rig.held_identities() < 8, "{} identities held", rig.held_identities());
}
