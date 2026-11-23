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
/// for the exact window, which the page can pull after the answer was lost to a reload.
#[test]
fn r8_every_refusal_exit_leaves_a_loss_for_the_surviving_page() {
    type Setup = fn(&Rig, &Doc, &adapter::Window) -> Result<String, adapter::Refused>;
    let exits: [(&str, Setup); 6] = [
        ("a full writer queue refuses the call at once", |rig, doc, w| {
            fill_the_writer(rig, doc, w);
            rig.input(doc, w, INPUT_QUEUE_CAPACITY as u64 + 1, b"K")
        }),
        ("a parked call pumped into a full writer queue", |rig, doc, w| {
            fill_the_writer(rig, doc, w);
            let n = INPUT_QUEUE_CAPACITY as u64;
            rig.input(doc, w, n + 2, b"K")?;
            // Its number comes: the call before it takes the order past it into the full queue.
            rig.input(doc, w, n + 1, b"J").map(|_| String::new())
        }),
        ("a human's bytes past the window's allowance", |rig, doc, w| {
            let paste = vec![b'p'; 4 * 1024 * 1024 - 64];
            rig.input(doc, w, 1, &paste)?;
            rig.input(doc, w, 2, &paste)?;
            rig.input(doc, w, 3, &paste)
        }),
        ("a reply past the replies' allowance", |rig, doc, w| {
            rig.reply(doc, w, 1, &vec![b'r'; 64 * 1024 + 1])
        }),
        ("a window with no writer queue", |rig, doc, _w| {
            let gone = rig.window("gone", 1);
            let key = rig.writer_queue_gone(gone);
            rig.remove_route(&key);
            rig.input_for(doc, &key, 1, b"K")
        }),
        ("a writer queue that has closed", |rig, doc, _w| {
            let closed = rig.window("closed", 1);
            let key = rig.writer_queue_gone(closed);
            rig.input_for(doc, &key, 1, b"K")
        }),
    ];
    for (name, setup) in exits {
        let rig = Rig::new();
        let w = rig.window("W", 5);
        rig.navigate();
        let old = rig.open_page("s:1");
        let result = setup(&rig, &old, &w);
        assert!(
            result.is_err() || name.contains("pumped"),
            "{name}: the refusal is told to the caller: {result:?}"
        );
        // The refusal answer is lost to the reload: the surviving page pulls the loss.
        rig.navigate();
        let next = rig.open_page("s:2");
        let owed = rig.pull(&next).unwrap();
        assert!(
            !owed.is_empty() && owed.iter().all(|notice| notice.loss > 0),
            "{name}: a numbered loss survives for the surviving page: {owed:?}"
        );
        let written = rig.written(&w);
        assert!(
            !written.contains(&b'K'),
            "{name}: no refused byte reaches the writer"
        );
    }
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
    let absent = slots + 2;
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
