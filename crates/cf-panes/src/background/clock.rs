//! What the host saw of the wall clock while a pane was open (`PaneClock`), for a look at
//! its commands to leave out what began before a monitor's call (`pane.commands`' `after`)
//! only where the clock is shown to have kept its order (Astraeus on 02a6cd4f: a clock set
//! back between a monitor's call and its command's start made the command look older, and
//! a look took the monitor for over while its command ran).
//!
//! A monitor's call is stamped by the wall clock as it is made, and its command's start is
//! the wall clock's too (`pbi_start`). Slewing, which is how the time of day is kept right,
//! only makes the wall clock run a little fast or slow: it never runs back, and keeps their
//! order. A step does not: a clock set back by more than the margin between a call and its
//! command's start makes the command look older than the call. So the host watches the
//! wall clock against the continuous one (`mach_continuous_time`, which no setting of the
//! time moves, and which runs on while the machine sleeps): it reads both every
//! [`SAMPLE`], and keeps, from the pane's open, the largest fall of the wall clock behind
//! the continuous one beyond what a slew can make ([`SLEW_PPM`]) between any reading and a
//! later one. A process's start is read twice (its wall start and its awake start,
//! `ri_proc_start_abstime`, which the awake clock `mach_absolute_time` keeps and no setting
//! of the time moves), and must agree with the readings around it. A process is left out by
//! its start only where (`PaneClock::began_before`):
//!
//! - the host read the clocks at least once each [`MAX_GAP`] of awake time since the pane
//!   opened, and the wall clock fell no more than [`BUDGET`] (less than the engine's margin)
//!   behind the continuous one beyond a slew, between any of those readings and a later one;
//! - the process's wall start, put on the continuous clock by its awake start, falls no
//!   more than [`PROCESS_NOISE`] behind the readings on either side of it, which no sleep
//!   lies between;
//! - and its wall start is before the time the look names.
//!
//! Anything else is counted, the safe way. Measured on a Mac (Darwin 27, T-334): a process's
//! wall start less its awake start agrees with the same difference read just after to ±4
//! µs (300 processes); the wall clock drifted 9 ppm behind the continuous one (0.55 ms a
//! minute), and moved at most 24 µs between readings 100 ms apart.
//!
//! A read of the clocks is a reading only where its two reads of each monotonic clock lie
//! within [`READ_BOUND`] (`Reading::of`): one put off between them would pair a wall time with
//! a monotonic time far from it. A pane whose first read is no reading has its clock from its
//! first reading on, and nothing begun before that is proved older.
//!
//! Limit. A wall clock set forward by more than the margin and set back again within one gap
//! between readings ([`SAMPLE`], [`MAX_GAP`] at most), a monitor's call stamped between the two
//! settings and its command started after the second, falls between readings: no reading
//! shows the clock as it was when the call was stamped, and the command's own two starts
//! agree with the clock after it (Astraeus on eb9f25ef). Only the call's own monotonic time
//! would prove more, and Claude Code's record of a call carries the wall clock's alone.
//! Written down as a known limit, not fixed: the chief's decision, 2026-10-23, T-346.

#[cfg(target_os = "macos")]
use std::sync::{Arc, Mutex, PoisonError, Weak};

use super::Member;

/// How often the host reads the clocks.
#[cfg(target_os = "macos")]
pub(crate) const SAMPLE: std::time::Duration = std::time::Duration::from_millis(100);

/// The most awake time between two readings for the watch to vouch for the clock: a
/// longer gap (a host that did not run) leaves the clock unknown for good, for that pane.
const MAX_GAP: i64 = 2_000_000;

/// How fast a slew may move the wall clock against the continuous one, in parts per
/// million: macOS slews at most 500 ppm.
const SLEW_PPM: i64 = 1_000;

/// How far the wall clock may fall behind the continuous one, beyond a slew, between any
/// two readings: well below the engine's margin (`turn_end`'s `CALL_MARGIN_MS`, 1 s).
const BUDGET: i64 = 500_000;

/// How far apart the two reads of a monotonic clock around a wall clock's read may lie for
/// a reading to be made of them. Measured on a Mac (T-349): 20,000 reads at a time lay at
/// most 1 µs apart idle, and at most 14 µs with every core busy.
const READ_BOUND: i64 = 1_000;

/// How far a process's own two starts may disagree with the readings around it.
const PROCESS_NOISE: i64 = 5_000;

/// How far the wall clock may move against the continuous one before a reading is kept,
/// and how much sleep a reading may show before it is taken for one.
const KEPT: i64 = 1_000;

/// One reading of the clocks, each in microseconds: the wall clock since the epoch, the
/// awake clock (which stops while the machine sleeps) and the continuous one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Reading {
    pub wall: i64,
    pub awake: i64,
    pub continuous: i64,
}

/// The clocks as read: the wall clock between two reads of the awake and the continuous
/// ones, each in microseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Read {
    pub wall: i64,
    pub awake: (i64, i64),
    pub continuous: (i64, i64),
}

impl Reading {
    /// The reading `read` makes: the monotonic clocks at the middle of their two reads. None
    /// where those lie more than [`READ_BOUND`] apart, on either clock (Astraeus on
    /// eb9f25ef: a read put off between its wall and monotonic reads pairs times far apart),
    /// and so where a sleep fell between them, which the continuous clock counts.
    pub fn of(read: Read) -> Option<Self> {
        let apart = |(from, to): (i64, i64)| (0..=READ_BOUND).contains(&(to - from));
        if !apart(read.awake) || !apart(read.continuous) {
            return None;
        }
        Some(Self {
            wall: read.wall,
            awake: read.awake.0 + (read.awake.1 - read.awake.0) / 2,
            continuous: read.continuous.0 + (read.continuous.1 - read.continuous.0) / 2,
        })
    }

    /// How far the wall clock is ahead of the continuous one.
    fn ahead(self) -> i64 {
        self.wall - self.continuous
    }

    /// How long the machine has slept, as far as the two monotonic clocks say.
    fn slept(self) -> i64 {
        self.continuous - self.awake
    }

    /// The wall clock's lead, less what a slew may have taken since the continuous
    /// clock's zero: a fall of it from one reading to a later one is a step back.
    fn steady(self) -> i64 {
        self.ahead() + self.continuous * SLEW_PPM / 1_000_000
    }
}

/// What the host saw of the clocks from a pane's open: the readings where they moved
/// against each other ([`KEPT`]), with the one before each, the latest, the highest steady
/// lead so far and the largest fall from it, and whether a gap left it unknown.
#[derive(Debug, Clone)]
pub(crate) struct PaneClock {
    kept: Vec<Reading>,
    last: Option<Reading>,
    peak: i64,
    fall: i64,
    unknown: bool,
}

impl PaneClock {
    /// A pane opens: the clocks read now are its first reading, where they made one. A
    /// pane with none has its clock from its first reading on, and no process begun before
    /// that is proved older.
    pub fn new(first: Option<Reading>) -> Self {
        let mut clock = Self {
            kept: Vec::new(),
            last: None,
            peak: i64::MIN,
            fall: 0,
            unknown: false,
        };
        clock.observe(first);
        clock
    }

    /// The clocks read again: none where they made no reading, which leaves a gap.
    pub fn observe(&mut self, now: Option<Reading>) {
        let Some(now) = now else {
            return;
        };
        let Some(before) = self.last else {
            self.kept.push(now);
            self.last = Some(now);
            self.peak = now.steady();
            return;
        };
        if now.awake < before.awake || now.awake - before.awake > MAX_GAP {
            self.unknown = true;
        }
        self.peak = self.peak.max(now.steady());
        self.fall = self.fall.max(self.peak - now.steady());
        let kept = *self.kept.last().unwrap_or(&before);
        let moved = (now.ahead() - kept.ahead()).abs() > KEPT
            || (now.slept() - before.slept()).abs() > KEPT;
        if moved {
            if self.kept.last() != Some(&before) {
                self.kept.push(before);
            }
            self.kept.push(now);
        }
        self.last = Some(now);
    }

    /// Whether the process that began at `wall` by the wall clock and at `awake` by the
    /// awake one (microseconds) is proved to have begun before `after` (see above).
    pub fn began_before(&self, wall: i64, awake: i64, after: i64) -> bool {
        if self.unknown || self.fall > BUDGET || wall >= after {
            return false;
        }
        // A process begun before the first reading or after the latest has none around it.
        let (Some(first), Some(last)) = (self.kept.first(), self.last.as_ref()) else {
            return false;
        };
        let readings = self.kept.iter().chain(std::iter::once(last));
        let mut around = None;
        let mut previous: Option<&Reading> = None;
        for reading in readings {
            if let Some(previous) = previous {
                if previous.awake <= awake && awake <= reading.awake {
                    around = Some((*previous, *reading));
                    break;
                }
            }
            previous = Some(reading);
        }
        let Some((before, after_it)) = around
            .or_else(|| (self.kept.len() == 1 && awake == first.awake).then_some((*first, *first)))
        else {
            return false;
        };
        // A sleep between the two readings: the process's place on the continuous clock is
        // not known.
        if (after_it.slept() - before.slept()).abs() > KEPT {
            return false;
        }
        let ahead = wall - (awake + before.slept());
        ahead >= before.ahead().min(after_it.ahead()) - PROCESS_NOISE
    }
}

/// The host's watch of the clocks: it reads `clocks` every [`SAMPLE`] on a thread of its
/// own, for each pane's clock that is still kept, and once more at each look.
#[cfg(target_os = "macos")]
pub(crate) struct Watch {
    clocks: fn() -> Read,
    panes: Mutex<Vec<Weak<Mutex<PaneClock>>>>,
}

#[cfg(target_os = "macos")]
impl Watch {
    /// A watch reading `clocks`, its thread started: it ends with the last hold of it.
    pub fn start(clocks: fn() -> Read) -> Arc<Self> {
        let watch = Arc::new(Self {
            clocks,
            panes: Mutex::new(Vec::new()),
        });
        let held = Arc::downgrade(&watch);
        std::thread::Builder::new()
            .name("cf-clock-watch".to_owned())
            .spawn(move || {
                while let Some(watch) = held.upgrade() {
                    watch.read();
                    drop(watch);
                    std::thread::sleep(SAMPLE);
                }
            })
            .ok();
        watch
    }

    /// A pane opens: its clock, read from now on.
    pub fn open(&self) -> Arc<Mutex<PaneClock>> {
        let clock = Arc::new(Mutex::new(PaneClock::new(self.reading())));
        self.panes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Arc::downgrade(&clock));
        clock
    }

    /// `clock` read once more, as a look sees it. The clocks are read under the pane's lock,
    /// so a pane's readings come in their order.
    pub fn now(&self, clock: &Mutex<PaneClock>) -> PaneClock {
        let mut clock = clock.lock().unwrap_or_else(PoisonError::into_inner);
        clock.observe(self.reading());
        clock.clone()
    }

    /// `clock` read once more with the wall clock `by` microseconds off what it says: a
    /// clock set as a test sets it.
    #[cfg(test)]
    pub fn set(&self, clock: &Mutex<PaneClock>, by: i64) {
        let mut clock = clock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut reading = self.reading();
        if let Some(reading) = reading.as_mut() {
            reading.wall += by;
        }
        clock.observe(reading);
    }

    /// The clocks read ([`Reading::of`]): a read put off is made again, three times at most.
    fn reading(&self) -> Option<Reading> {
        (0..3).find_map(|_| Reading::of((self.clocks)()))
    }

    /// Every pane's clock read, and those no pane holds let go of.
    fn read(&self) {
        let mut panes = self.panes.lock().unwrap_or_else(PoisonError::into_inner);
        panes.retain(|pane| match pane.upgrade() {
            Some(clock) => {
                clock
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .observe(self.reading());
                true
            }
            None => false,
        });
    }
}

/// Whether `member` is proved to have begun before `after` (milliseconds since the epoch)
/// by `clock` ([`PaneClock::began_before`]), its awake start read by `awake` (microseconds):
/// a member whose start was not read is not.
pub(crate) fn began_before<'a>(
    clock: &'a PaneClock,
    after: i64,
    awake: impl Fn(u32) -> Option<i64> + 'a,
) -> impl Fn(&Member) -> bool + 'a {
    move |member: &Member| {
        let Ok(wall) = i64::try_from(member.key.began) else {
            return false;
        };
        wall != 0
            && awake(member.key.pid)
                .is_some_and(|start| clock.began_before(wall, start, after.saturating_mul(1_000)))
    }
}

#[cfg(test)]
mod tests;
