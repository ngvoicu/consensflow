//! The stop of everything open, on a disk that is busy: where the time goes
//! that a quiet machine does not spend (T-376).
//!
//! `a_pass_that_never_ends_a_door_an_idle_connection_and_a_body_half_sent_cost_one_deadline`
//! once read 1.57 s in a run of the whole crate beside other trees' builds,
//! where its bound was 1.3 s, and 1.14 s alone. The wait is not what grows: it
//! is one deadline, to a few milliseconds, on the machine's clock too. What
//! grows is the stop's own work on the disk, which a build that writes
//! gigabytes is slow to have answered: the line it writes before it waits (an
//! append to `daemon.log`) and the close of the ledger after it (a checkpoint
//! and a sync, `synchronous = FULL`). That work is the machine's time and the
//! wait is the stop's, so the stop's test counts the wait in the clock of the
//! timers it waits on and leaves the disk out.
//!
//! This is that, reproduced: the disk kept as busy as a build keeps it, the
//! stop of the old test measured on the machine's clock until it crosses the
//! old bound, and then the same stop in the timers' clock, which reads one
//! deadline every time.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use super::*;

/// Where the old test's bound was: a stop that took more failed it.
const OLD_BOUND: Duration = Duration::from_millis(1_300);

/// How many rounds of each clock at most, and how many of the machine's
/// clock have to cross the old bound for the disk to have been busy enough.
const ROUNDS: usize = 120;
const CROSSINGS: usize = 3;

/// How many rounds in the timers' clock: they cost the disk's work and no wait.
const PAUSED_ROUNDS: usize = 40;

/// Threads that keep the disk as busy as a build does while they live: each
/// writes a file of 256 MB, syncs it, and writes it again.
struct BusyDisk {
    over: Arc<AtomicBool>,
    writers: Vec<JoinHandle<()>>,
}

impl BusyDisk {
    fn start(writers: usize, folder: &Path) -> Self {
        let over = Arc::new(AtomicBool::new(false));
        let writers = (0..writers)
            .map(|n| {
                let over = Arc::clone(&over);
                let file = folder.join(format!("busy-{n}"));
                std::thread::spawn(move || {
                    let block = vec![7u8; 256 * 1024 * 1024];
                    while !over.load(Ordering::Relaxed) {
                        let mut out = std::fs::File::create(&file).unwrap();
                        std::io::Write::write_all(&mut out, &block).unwrap();
                        out.sync_all().unwrap();
                    }
                })
            })
            .collect();
        Self { over, writers }
    }
}

impl Drop for BusyDisk {
    fn drop(&mut self) {
        self.over.store(true, Ordering::Relaxed);
        for writer in self.writers.drain(..) {
            let _ = writer.join();
        }
    }
}

/// One stop of everything open, in the clocks it was counted in.
struct Stopped {
    /// What the stop took on the machine's clock.
    machine: Duration,
    /// What the stop took in the timers' clock, when it ran in paused time.
    timers: Option<Duration>,
    /// On the machine's clock: from the ask to the tail, and the tail (the
    /// children, the ledger's close, the log).
    to_the_tail: Duration,
    in_the_tail: Duration,
}

/// The stop of everything open, once; in paused time or not.
async fn stop_everything(paused: bool) -> Stopped {
    let mut open = everything_open().await;
    let tail_at = Rc::new(Cell::new(None));
    let ends = Rc::clone(&open.rig.stopping.ends_children);
    let noted = Rc::clone(&tail_at);
    open.rig.stopping.ends_children = Rc::new(move || {
        noted.set(Some(Instant::now()));
        ends();
    });
    if paused {
        tokio::time::pause();
    }
    let timers_from = tokio::time::Instant::now();
    let started = Instant::now();
    open.rig.stopping.run("SIGTERM").await;
    let machine = started.elapsed();
    let timers = paused.then(|| timers_from.elapsed());
    if paused {
        tokio::time::resume();
    }
    let to_the_tail = tail_at.get().unwrap() - started;
    open.assert_stopped().await;
    Stopped {
        machine,
        timers,
        to_the_tail,
        in_the_tail: machine - to_the_tail,
    }
}

fn ms(duration: Duration) -> String {
    format!("{:7.1}", duration.as_secs_f64() * 1_000.0)
}

#[tokio::test]
#[ignore = "T-376's reproduction: needs a busy disk (four writers of 256 MB, over and over), about a minute"]
async fn a_busy_disk_reads_the_stop_long_on_the_machines_clock_and_one_deadline_on_the_timers() {
    let folder = tempfile::tempdir().unwrap();
    let _busy = BusyDisk::start(4, folder.path());

    println!("the machine's clock, as the old test read it (ms):");
    println!("   whole | to the tail | in the tail");
    let mut crossed = 0;
    LocalSet::new()
        .run_until(async {
            for _ in 0..ROUNDS {
                let stop = stop_everything(false).await;
                let crossing = stop.machine >= OLD_BOUND;
                println!(
                    "{} | {}     | {}  {}",
                    ms(stop.machine),
                    ms(stop.to_the_tail),
                    ms(stop.in_the_tail),
                    if crossing {
                        "<- past the old bound"
                    } else {
                        ""
                    }
                );
                crossed += usize::from(crossing);
                if crossed == CROSSINGS {
                    break;
                }
            }
        })
        .await;
    assert_eq!(
        crossed, CROSSINGS,
        "the disk was not busy enough to stall the old reading {CROSSINGS} times in {ROUNDS} rounds"
    );

    println!("the timers' clock, the same stop (ms; the machine's clock beside it is the disk's work alone):");
    LocalSet::new()
        .run_until(async {
            for _ in 0..PAUSED_ROUNDS {
                let stop = stop_everything(true).await;
                let timers = stop.timers.unwrap();
                println!("{} | machine {}", ms(timers), ms(stop.machine));
                assert!(
                    timers >= Duration::from_secs(1)
                        && timers <= Duration::from_secs(1) + Duration::from_millis(1),
                    "{timers:?}"
                );
            }
        })
        .await;
}
