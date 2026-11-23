//! D4's P4-B randomized gate: a random schedule of the steps the native queue lives through,
//! with its invariants asserted after every one (T-2140). The steps are enqueue, skip, a writer
//! done, failed or gone, a cut, a page's prepare and activation, a window retired and opened
//! again, a drop's place and its release, a pull, a dismissal and the bounds passing. A schedule
//! is a seed; a failure names its seed and the steps that led to it.
//!
//! The invariants, after every step:
//! 1. within one document, a window's bytes reach its writer in number order;
//! 2. every admitted call ends as written or as a loss (never both, unless the page's own skip
//!    said it may be lost; and at the end, never neither), and every loss is covered by a notice
//!    that is on every pull until its exact id is dismissed (or replaced or folded: the host's
//!    own lineage says to what);
//! 3. a notice's words never change under its id, and an id is never used twice.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::adapter::{Doc, Notice, Rig, Window};
use super::super::*;

/// A small deterministic generator: a schedule is its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound.max(1)
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            Some(&items[self.below(items.len() as u64) as usize])
        }
    }
}

struct Call {
    /// The window and the document's page it was made by, and its number.
    key: PaneKey,
    page: String,
    number: u64,
    /// The host's answer: its completion if it was admitted, or the loss it numbered if refused.
    completion: Option<PageInputCompletion>,
    refused_loss: Option<u64>,
    /// The writer finished it, and said it was written.
    written: bool,
    /// The page said it may be lost (a skip), whatever became of it.
    skipped: bool,
    /// The writer failed it, or vanished with it.
    failed: bool,
    /// A repeat of a number the host had settled: refused, and no call of its own.
    repeat: bool,
}

impl Call {
    fn loss(&self) -> Option<u64> {
        self.refused_loss
            .or_else(|| self.completion.as_ref().and_then(PageInputCompletion::loss))
    }
}

/// What a schedule reached, summed over a run: a gate that never folds or replaces gates nothing.
#[derive(Default, Clone, Copy, Debug)]
struct Reached {
    calls: u64,
    written: u64,
    lost: u64,
    replaced: u64,
    folded: u64,
    dismissed: u64,
    cuts: u64,
}

struct Schedule {
    seed: u64,
    rng: Rng,
    rig: Rig,
    /// The windows that are open, and those that have closed (their writers still hold calls).
    windows: Vec<Window>,
    closed: Vec<Window>,
    next_generation: u64,
    docs: Vec<Doc>,
    current: Option<Doc>,
    prepared: Option<Doc>,
    sessions: u64,
    /// The next number of a window for a document.
    next_number: HashMap<(String, PaneKey), u64>,
    /// The place numbers a drop took and has not used or let go.
    places: Vec<(Doc, PaneKey, u64)>,
    calls: Vec<Call>,
    /// The last number handed to a window's writer, by document.
    handed: HashMap<(String, PaneKey), u64>,
    /// Every notice seen, by id, and the ids the last pull held.
    seen: BTreeMap<u64, Notice>,
    last_pull: BTreeSet<u64>,
    dismissed: HashSet<u64>,
    log: Vec<String>,
    reached: Reached,
}

impl Schedule {
    fn new(seed: u64) -> Self {
        let rig = Rig::new();
        rig.manual_clock();
        let mut schedule = Self {
            seed,
            rng: Rng(seed),
            rig,
            windows: Vec::new(),
            closed: Vec::new(),
            next_generation: 1,
            docs: Vec::new(),
            current: None,
            prepared: None,
            sessions: 0,
            next_number: HashMap::new(),
            places: Vec::new(),
            calls: Vec::new(),
            handed: HashMap::new(),
            seen: BTreeMap::new(),
            last_pull: BTreeSet::new(),
            dismissed: HashSet::new(),
            log: Vec::new(),
            reached: Reached::default(),
        };
        // A small cap on outstanding notices, so that the host folds within a short schedule.
        schedule.rig.notice_cap(4);
        for id in ["W", "X"] {
            schedule.open_window(id);
        }
        schedule.rig.navigate();
        schedule.activate_new();
        schedule
    }

    fn open_window(&mut self, id: &str) {
        let generation = self.next_generation;
        self.next_generation += 1;
        let window = self.rig.window(id, generation);
        self.windows.push(window);
    }

    fn fail(&self, what: &str) -> ! {
        panic!(
            "schedule seed {:#x} broke: {what}\nsteps:\n  {}",
            self.seed,
            self.log.join("\n  ")
        );
    }

    fn prepare(&mut self) -> Doc {
        self.sessions += 1;
        let doc = self.rig.prepare(&format!("s{}:1", self.sessions));
        self.docs.push(doc.clone());
        doc
    }

    fn activate_new(&mut self) {
        let doc = self.prepare();
        self.rig.activate(&doc).unwrap();
        self.current = Some(doc);
        self.prepared = None;
    }

    fn some_window(&mut self) -> Option<usize> {
        if self.windows.is_empty() {
            None
        } else {
            Some(self.rng.below(self.windows.len() as u64) as usize)
        }
    }

    /// Any document ever made: usually the current one, sometimes a stale one.
    fn some_doc(&mut self) -> Option<Doc> {
        if self.rng.below(5) == 0 {
            self.rng.pick(&self.docs).cloned()
        } else {
            self.current.clone().or_else(|| self.rng.pick(&self.docs).cloned())
        }
    }

    /// The number the next call of `doc` takes in `key`, and sometimes a gap or a repeat.
    fn number_for(&mut self, doc: &Doc, key: &PaneKey) -> u64 {
        let slot = self
            .next_number
            .entry((doc.page.clone(), key.clone()))
            .or_insert(1);
        let next = *slot;
        let roll = self.rng.below(10);
        let number = match roll {
            0 | 1 => next + 1 + self.rng.below(3),
            2 if next > 1 => 1 + self.rng.below(next - 1),
            _ => next,
        };
        let slot = self
            .next_number
            .get_mut(&(doc.page.clone(), key.clone()))
            .expect("the slot");
        *slot = (*slot).max(number + 1);
        number
    }

    fn step(&mut self) {
        let roll = self.rng.below(100);
        match roll {
            0..=29 => self.enqueue(),
            30..=37 => self.skip(),
            38..=52 => self.writer(Writer::Done),
            53..=57 => self.writer(Writer::Failed),
            58..=60 => self.writer(Writer::Gone),
            61..=65 => self.cut(),
            66..=68 => self.prepare_step(),
            69..=72 => self.activate_step(),
            73..=75 => self.retire(),
            76..=78 => self.reopen(),
            79..=83 => self.place(),
            84..=87 => self.release(),
            88..=92 => self.pull_step(),
            93..=96 => self.dismiss(),
            _ => self.bounds(),
        }
        self.check();
    }

    fn enqueue(&mut self) {
        let (Some(doc), Some(at)) = (self.some_doc(), self.some_window()) else {
            return;
        };
        let key = self.windows[at].key.clone();
        let number = self.number_for(&doc, &key);
        self.make_call(&doc, &key, number, at);
    }

    fn make_call(&mut self, doc: &Doc, key: &PaneKey, number: u64, at: usize) {
        let index = self.calls.len();
        let bytes = format!("<{index}>").into_bytes();
        // The page never sends a number twice; one that does is a repeat of the first, and owes
        // nothing of its own.
        let repeat = self
            .calls
            .iter()
            .any(|call| {
                call.completion.is_some()
                    && call.key == *key
                    && call.page == doc.page
                    && call.number == number
            });
        self.log.push(format!(
            "enqueue call {index}: window {key:?} number {number} by {} (epoch {:?})",
            doc.page, doc.epoch
        ));
        let result = self.rig.input(doc, &self.windows[at], number, &bytes);
        let mut call = Call {
            key: key.clone(),
            page: doc.page.clone(),
            number,
            completion: None,
            refused_loss: None,
            written: false,
            skipped: false,
            failed: false,
            repeat,
        };
        match result {
            Ok(ticket) if !repeat => {
                // The completion is only asked of the current page; the document that made the
                // call is current here, or the call would have been refused.
                call.completion = Some(self.rig.completion(doc, &ticket));
            }
            Ok(_) => {}
            // A repeat or a late original of a number the host has settled is no call of its
            // own: that number's call, skip or give-up has its loss.
            Err((error, None)) if error == INPUT_SEQUENCE_STALE => call.repeat = true,
            Err((_, loss)) => call.refused_loss = loss.filter(|_| !repeat),
        }
        self.calls.push(call);
    }

    fn skip(&mut self) {
        let (Some(doc), Some(at)) = (self.some_doc(), self.some_window()) else {
            return;
        };
        let key = self.windows[at].key.clone();
        let upper = self
            .next_number
            .get(&(doc.page.clone(), key.clone()))
            .copied()
            .unwrap_or(1)
            + 2;
        let number = 1 + self.rng.below(upper);
        self.log.push(format!("skip {key:?} number {number} by {}", doc.page));
        let result = self.rig.skip(&doc, &self.windows[at], number);
        let current = self.current.as_ref().is_some_and(|c| c.page == doc.page);
        if current && doc.epoch == self.rig.host_epoch() {
            if let Err(error) = &result {
                if error != SKIP_PENDING {
                    self.fail(&format!("the current page's skip was refused: {error}"));
                }
            }
        }
        for call in &mut self.calls {
            if call.key == key && call.page == doc.page && call.number == number {
                call.skipped = true;
            }
        }
    }

    fn writer(&mut self, how: Writer) {
        let Some(at) = self.some_window() else {
            return;
        };
        let count = self.windows.len();
        // A closed window's writer may finish too: its queue still holds calls.
        let closed = self.closed.len();
        let pick = self.rng.below((count + closed) as u64) as usize;
        let window = if pick < count {
            &self.windows[at]
        } else {
            &self.closed[pick - count]
        };
        let Some(job) = self.rig.take(window) else {
            return;
        };
        self.log.push(format!("writer {how:?} for {:?}", window.key));
        let key = window.key.clone();
        let index = parse(job.bytes());
        let (page, number) = {
            let call = &self.calls[index];
            (call.page.clone(), call.number)
        };
        // 1. Within one document a window's bytes reach its writer in number order.
        let last = self.handed.entry((page, key)).or_insert(0);
        if number <= *last {
            let last = *last;
            self.fail(&format!("call {index} (number {number}) reached the writer after number {last}"));
        }
        *last = number;
        match how {
            Writer::Done => {
                self.calls[index].written = true;
                job.written();
            }
            Writer::Failed => {
                self.calls[index].failed = true;
                let error = if self.rng.below(2) == 0 {
                    InputError::Uncertain("the write failed".into())
                } else {
                    InputError::FailedWrite("the window broke".into())
                };
                job.failed(error);
            }
            Writer::Gone => {
                self.calls[index].failed = true;
                job.vanishes();
            }
        }
        if self.calls[index].failed && self.calls[index].loss().is_none() {
            self.fail(&format!("call {index} failed at its writer and has no loss"));
        }
    }

    fn cut(&mut self) {
        self.reached.cuts += 1;
        self.log.push("cut".into());
        self.rig.navigate();
        self.current = None;
        self.prepared = None;
        self.places.clear();
        // Most reloads bring a page that activates before anything else happens.
        if self.rng.below(10) < 7 {
            self.log.push("the new page prepares and activates".into());
            self.activate_new();
        }
    }

    fn prepare_step(&mut self) {
        if self.current.is_some() || self.prepared.is_some() {
            return;
        }
        self.log.push("prepare".into());
        let doc = self.prepare();
        self.prepared = Some(doc);
    }

    fn activate_step(&mut self) {
        match (self.prepared.take(), self.current.is_some()) {
            (Some(doc), false) => {
                self.log.push(format!("activate {}", doc.page));
                self.rig.activate(&doc).unwrap();
                self.current = Some(doc);
            }
            (Some(doc), true) => self.prepared = Some(doc),
            (None, _) => {
                // A stale or made-up activation seizes nothing.
                if let Some(old) = self.rng.pick(&self.docs).cloned() {
                    let live = self.current.as_ref().is_some_and(|c| c.page == old.page);
                    if !live && self.prepared.as_ref().map(|p| &p.page) != Some(&old.page) {
                        self.log.push(format!("stale activation of {}", old.page));
                        if self.rig.activate(&old).is_ok() && old.epoch != self.rig.host_epoch() {
                            self.fail("a stale activation was accepted");
                        }
                    }
                }
            }
        }
    }

    fn retire(&mut self) {
        if self.windows.len() < 2 {
            return;
        }
        let at = self.rng.below(self.windows.len() as u64) as usize;
        let window = self.windows.remove(at);
        self.log.push(format!("retire {:?}", window.key));
        self.rig.retire(&window);
        self.closed.push(window);
    }

    fn reopen(&mut self) {
        if self.windows.len() >= 3 {
            return;
        }
        let id = if self.rng.below(2) == 0 { "W" } else { "X" };
        self.log.push(format!("open {id} again"));
        self.open_window(id);
    }

    fn place(&mut self) {
        let (Some(doc), Some(at)) = (self.current.clone(), self.some_window()) else {
            return;
        };
        let key = self.windows[at].key.clone();
        let number = self.number_for(&doc, &key);
        self.log.push(format!("place {number} in {key:?}"));
        if self.rig.place(&doc, &self.windows[at], number).is_ok() {
            self.places.push((doc, key, number));
        }
    }

    fn release(&mut self) {
        let Some(at) = self.some_window() else {
            return;
        };
        let mut used_place = None;
        if !self.places.is_empty() && self.rng.below(2) == 0 {
            let which = self.rng.below(self.places.len() as u64) as usize;
            used_place = Some(self.places.remove(which));
        }
        match used_place {
            Some((doc, key, number)) => {
                let Some(window) = self.windows.iter().position(|w| w.key == key) else {
                    return;
                };
                if self.rng.below(2) == 0 {
                    self.log.push(format!("release place {number} of {key:?}"));
                    let _ = self.rig.release(&doc, &self.windows[window], number);
                } else {
                    self.log.push(format!("use place {number} of {key:?}"));
                    self.make_call(&doc, &key, number, window);
                }
            }
            None => {
                let Some(doc) = self.some_doc() else { return };
                let number = 1 + self.rng.below(6);
                self.log.push(format!("release number {number} by {}", doc.page));
                let result = self.rig.release(&doc, &self.windows[at], number);
                let live = self.current.as_ref().is_some_and(|c| c.page == doc.page)
                    && doc.epoch == self.rig.host_epoch();
                if !live && result.is_ok() {
                    self.fail("a departed page's release was accepted");
                }
            }
        }
    }

    fn bounds(&mut self) {
        self.log.push("bounds pass".into());
        self.rig.bounds_pass();
    }

    fn pull_step(&mut self) {
        self.log.push("pull".into());
    }

    fn dismiss(&mut self) {
        let Some(doc) = self.some_doc() else { return };
        let live = self.current.as_ref().is_some_and(|c| c.page == doc.page)
            && doc.epoch == self.rig.host_epoch();
        let Ok(before) = self.rig.pull(&doc).map(|list| list.into_iter().map(|n| n.id).collect::<Vec<_>>())
        else {
            if live {
                self.fail("the current page could not pull");
            }
            // A stale page's dismissal is refused.
            if let Some(id) = self.rng.pick(&self.seen.keys().copied().collect::<Vec<_>>()).copied() {
                self.log.push(format!("dismiss {id} by stale {}", doc.page));
                if self.rig.dismiss(&doc, id).is_ok() {
                    self.fail("a stale page dismissed a notice");
                }
            }
            return;
        };
        let id = match self.rng.below(6) {
            0 => self.rng.next() % 5000 + 10_000_000,
            1 if !self.seen.is_empty() => {
                let all: Vec<u64> = self.seen.keys().copied().collect();
                *self.rng.pick(&all).unwrap_or(&0)
            }
            _ => match self.rng.pick(&before) {
                Some(id) => *id,
                None => return,
            },
        };
        self.log.push(format!("dismiss {id}"));
        match self.rig.dismiss(&doc, id) {
            Ok(removed) => {
                if removed != before.contains(&id) {
                    self.fail(&format!(
                        "dismiss {id} said removed={removed} but it was outstanding={}",
                        before.contains(&id)
                    ));
                }
                if removed {
                    self.dismissed.insert(id);
                    self.reached.dismissed += 1;
                }
            }
            Err(error) => {
                if live {
                    self.fail(&format!("the current page's dismissal was refused: {error}"));
                }
            }
        }
    }

    /// The notice that stands for loss number `loss` now: where it was made, then every
    /// replacement or fold the host recorded.
    fn standing(&self, loss: u64) -> Option<u64> {
        let (covers, lineage) = self.rig.ledger();
        let mut id = *covers.get(&loss)?;
        let mut steps = 0;
        while let Some(next) = lineage.get(&id) {
            id = *next;
            steps += 1;
            assert!(steps < 100_000, "a notice lineage loops");
        }
        Some(id)
    }

    fn check(&mut self) {
        let Some(doc) = self.current.clone() else {
            return;
        };
        let owed = match self.rig.pull(&doc) {
            Ok(owed) => owed,
            Err(error) => self.fail(&format!("the current page could not pull: {error}")),
        };
        let ids: BTreeSet<u64> = owed.iter().map(|notice| notice.id).collect();
        // 3. Words never change under an id, and an id is never used for two notices.
        for notice in &owed {
            match self.seen.get(&notice.id) {
                Some(first) if first != notice => self.fail(&format!(
                    "notice {} changed: first {first:?}, now {notice:?}",
                    notice.id
                )),
                Some(_) => {}
                None => {
                    if let Some(highest) = self.seen.keys().next_back() {
                        if notice.id < *highest && self.last_pull.contains(&notice.id) {
                            self.fail("an id reappeared");
                        }
                    }
                    self.seen.insert(notice.id, notice.clone());
                }
            }
        }
        // A notice goes only by its exact id being dismissed, or by its replacement or fold.
        let (_, lineage) = self.rig.ledger();
        for gone in self.last_pull.difference(&ids) {
            if !self.dismissed.contains(gone) && !lineage.contains_key(gone) {
                self.fail(&format!("notice {gone} vanished without a dismissal"));
            }
        }
        let windows = owed
            .iter()
            .filter(|notice| matches!(notice.body, NoticeBody::Window { .. }))
            .count();
        if windows > MAX_OUTSTANDING_NOTICES {
            self.fail(&format!("{windows} outstanding notices"));
        }
        self.last_pull = ids.clone();
        // 2. A call is written or lost, never both (but for a skip); each loss has its notice.
        for index in 0..self.calls.len() {
            let (loss, written, skipped) = {
                let call = &self.calls[index];
                (call.loss(), call.written, call.skipped)
            };
            let Some(loss) = loss else { continue };
            if written && !skipped {
                self.fail(&format!("call {index} was written and has loss {loss}"));
            }
            match self.standing(loss) {
                None => self.fail(&format!("call {index}'s loss {loss} made no notice")),
                Some(id) => {
                    if !ids.contains(&id) && !self.dismissed.contains(&id) {
                        self.fail(&format!(
                            "call {index}'s loss {loss} stands as notice {id}, which is neither on the pull nor dismissed"
                        ));
                    }
                }
            }
        }
    }

    /// The end: every bound passes, every writer finishes, the page reloads. No call is left
    /// neither written nor lost.
    fn settle(&mut self) {
        self.log.push("settle".into());
        if self.current.is_none() {
            self.rig.navigate();
            self.activate_new();
        }
        self.rig.bounds_pass();
        self.drain();
        self.rig.navigate();
        self.current = None;
        self.activate_new();
        self.drain();
        self.check();
        let (_, lineage) = self.rig.ledger();
        self.reached.replaced = lineage.len() as u64;
        self.reached.folded = self
            .seen
            .values()
            .filter(|notice| matches!(notice.body, NoticeBody::Folded { .. }))
            .count() as u64;
        for call in &self.calls {
            if call.completion.is_some() || call.refused_loss.is_some() {
                self.reached.calls += 1;
                if call.written {
                    self.reached.written += 1;
                }
                if call.loss().is_some() {
                    self.reached.lost += 1;
                }
            }
        }
        for (index, call) in self.calls.iter().enumerate() {
            let ended = call.written || call.loss().is_some();
            if call.completion.is_some() && !ended {
                self.fail(&format!(
                    "call {index} (window {:?}, number {}) ended neither written nor lost",
                    call.key, call.number
                ));
            }
            if call.refused_loss.is_none() && call.completion.is_none() && !call.repeat {
                self.fail(&format!("call {index} was refused with no loss number"));
            }
        }
    }

    fn drain(&mut self) {
        loop {
            let mut any = false;
            let count = self.windows.len() + self.closed.len();
            for at in 0..count {
                let window = if at < self.windows.len() {
                    &self.windows[at]
                } else {
                    &self.closed[at - self.windows.len()]
                };
                if let Some(job) = self.rig.take(window) {
                    any = true;
                    let key = window.key.clone();
                    let index = parse(job.bytes());
                    let (page, number) = {
                        let call = &self.calls[index];
                        (call.page.clone(), call.number)
                    };
                    let last = self.handed.entry((page, key)).or_insert(0);
                    if number <= *last {
                        let last = *last;
                        self.fail(&format!(
                            "call {index} (number {number}) reached the writer after number {last}"
                        ));
                    }
                    *last = number;
                    self.calls[index].written = true;
                    job.written();
                }
            }
            if !any {
                break;
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Writer {
    Done,
    Failed,
    Gone,
}

/// The call a job carries: its bytes are `<index>`.
fn parse(bytes: &[u8]) -> usize {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.strip_prefix('<'))
        .and_then(|text| text.strip_suffix('>'))
        .and_then(|text| text.parse().ok())
        .expect("a scheduled call's bytes name it")
}

/// One schedule: `steps` random steps, then the end.
fn run(seed: u64, steps: usize) -> Reached {
    let mut schedule = Schedule::new(seed);
    for _ in 0..steps {
        schedule.step();
    }
    schedule.settle();
    schedule.reached
}

/// The gate. `CF_SCHEDULES` schedules (2,000 by default, 10,000 or more for the gate), from the
/// seeds `CF_SCHEDULE_FIRST_SEED` onwards, 40 to 120 steps each. Every seed run is appended to
/// `CF_SCHEDULE_LOG` if it names a file; a failure is reported with its seed.
#[test]
#[allow(clippy::disallowed_methods, reason = "a gate run names its count and seeds in the environment")]
fn a_randomized_schedule_over_the_native_queue_holds_its_invariants() {
    let count: u64 = std::env::var("CF_SCHEDULES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2000);
    let first: u64 = std::env::var("CF_SCHEDULE_FIRST_SEED")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    let log = std::env::var("CF_SCHEDULE_LOG").ok();
    let mut failures = Vec::new();
    let mut total = Reached::default();
    for seed in first..first + count {
        let steps = 40 + (seed.wrapping_mul(2_654_435_761) % 81) as usize;
        let outcome = std::panic::catch_unwind(|| run(seed, steps));
        if let Ok(reached) = &outcome {
            total.calls += reached.calls;
            total.written += reached.written;
            total.lost += reached.lost;
            total.replaced += reached.replaced;
            total.folded += reached.folded;
            total.dismissed += reached.dismissed;
            total.cuts += reached.cuts;
        }
        if let Some(path) = &log {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(file, "{seed} {steps} {}", if outcome.is_ok() { "ok" } else { "FAIL" });
            }
        }
        if let Err(panic) = outcome {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                .unwrap_or_default();
            failures.push((seed, message));
            if failures.len() >= 5 {
                break;
            }
        }
    }
    eprintln!("schedules {first}..{}: reached {total:?}", first + count);
    if count >= 100 && failures.is_empty() {
        assert!(
            total.written > 0 && total.lost > 0 && total.replaced > 0 && total.folded > 0 && total.dismissed > 0,
            "the schedules never reached what they gate: {total:?}"
        );
    }
    assert!(
        failures.is_empty(),
        "{} of the schedules broke; the first:\n{}",
        failures.len(),
        failures
            .iter()
            .map(|(seed, message)| format!("seed {seed}: {message}"))
            .collect::<Vec<_>>()
            .join("\n---\n")
    );
}
