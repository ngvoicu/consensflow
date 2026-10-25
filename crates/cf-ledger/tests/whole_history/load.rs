//! The clock is no measure of what a read takes of the ledger's file: another
//! build running beside the test moves it by more than the bound the reads of
//! the 24 MiB item were once held to (T-238, a gate run at a load of 33: 1.18 s
//! for 200 pages of the long item against 318 ms for the two parts', where the
//! bound was twice). The reads' pages do not move with the machine's load, so
//! they are what `reads.rs` counts. This is the failure, kept to be shown by
//! hand: `cargo test -p cf-ledger --test whole_history -- --ignored under_load
//! --nocapture`.

use std::hint::black_box;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cf_ledger::{HistoryFrom, Ledger};

use super::reads::{ask, page_cost, reviewers, Reviewers};

/// The time the item's parts take read one after the other: the 394 the test
/// was once held to 5 s for.
fn walk(ledger: &Ledger, project: i64, conversation: i64, place: i64) -> Duration {
    let started = Instant::now();
    let mut from = Some(0);
    while let Some(at) = from {
        from = ledger
            .read_history_part(project, "chief", conversation, place, at)
            .expect("a part")
            .next;
    }
    started.elapsed()
}

/// Threads that do nothing but compute until they are dropped: to the
/// scheduler, what another build running beside the test is.
struct Busy {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Busy {
    /// `per_core` of them for each core of the machine.
    fn start(per_core: usize) -> Busy {
        let cores = thread::available_parallelism().map_or(1, usize::from);
        let stop = Arc::new(AtomicBool::new(false));
        let threads = (0..cores * per_core)
            .map(|_| {
                let stop = Arc::clone(&stop);
                thread::spawn(move || {
                    let mut at = 1_u64;
                    while !stop.load(Ordering::Relaxed) {
                        at = black_box(at.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1));
                    }
                })
            })
            .collect();
        Busy { stop, threads }
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// The time 200 pages of one entry from `from` take, as the test took it.
fn clock(ledger: &Ledger, project: i64, from: HistoryFrom) -> Duration {
    let started = Instant::now();
    for _ in 0..200 {
        let read = ledger
            .read_history(project, "chief", &ask(from, None, false, 1))
            .expect("read");
        assert_eq!(read.entries.len(), 1);
    }
    started.elapsed()
}

#[test]
#[ignore = "loads the machine for seconds: run by hand, with --nocapture"]
fn the_clock_the_24_mib_item_s_pages_were_held_to_fails_under_load_and_their_pages_do_not_move() {
    let dir = tempfile::tempdir().expect("a folder");
    let Reviewers {
        ledger,
        project,
        conversation,
        place,
        two_parts,
        ..
    } = reviewers(&dir);
    // Each once to warm up, then the pages each takes, at rest.
    clock(&ledger, project, two_parts);
    clock(&ledger, project, HistoryFrom::Newest);
    let at_rest = (
        page_cost(&ledger, project, HistoryFrom::Newest),
        page_cost(&ledger, project, two_parts),
    );
    // The two parts' 200 pages at the machine's own load, and the long item's
    // as a build starts beside the test: the bound was twice. A window's
    // share of a machine that is busy already moves from one to the next, so
    // it is tried again where the first did not.
    let at_rest_walk = walk(&ledger, project, conversation, place);
    let busy = Busy::start(4);
    let loaded_walk = walk(&ledger, project, conversation, place);
    drop(busy);
    println!("the item's 394 parts took {at_rest_walk:?} at the machine's own load, {loaded_walk:?} beside a build");
    let mut shown = None;
    for attempt in 1..=20 {
        let two = clock(&ledger, project, two_parts);
        let busy = Busy::start(4);
        let long = clock(&ledger, project, HistoryFrom::Newest);
        let loaded = (
            page_cost(&ledger, project, HistoryFrom::Newest),
            page_cost(&ledger, project, two_parts),
        );
        drop(busy);
        println!(
            "attempt {attempt}: 200 pages of the long item took {long:?}, of the two parts' {two:?}; pages {loaded:?}, at rest {at_rest:?}"
        );
        assert_eq!(loaded, at_rest, "the pages moved with the load");
        if long >= two * 2 {
            shown = Some((long, two));
            break;
        }
    }
    assert!(shown.is_some(), "the clock held to the bound in 20 tries");
}
