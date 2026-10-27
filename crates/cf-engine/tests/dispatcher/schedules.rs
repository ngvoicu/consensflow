//! Every schedule of looks a paste into a Devin window can meet, explored: the real
//! dispatcher, driven as `slow_records` drives it, looks at a model of Devin's queue
//! that answers as Devin's adapter would, over schedules generated from seeds and a
//! small grid of the alignments around the hand-over and the hold, and two things hold
//! in each. A schedule that breaks one prints itself, shrunk, as a timeline.
//!
//! The model. A paste given while a turn runs waits in Devin's queue and begins as a
//! turn of its own up to [`Q_MS`] after that turn ends; one given at rest begins at
//! once; one the model drops is never run nor recorded. A turn waits for the one before
//! it, the human's too. The record shows a turn's prompt as the turn begins, up to
//! [`LAG_MS`] late. The adapter's word comes from Devin's store and record, as Devin
//! 3000.11.92 writes them (T-102): the window reads busy while its agent works, and a
//! silent turn for [`TAKEN_IN_MS`] from its beginning (a message of the user's its agent
//! has not answered), at rest after that; `took` is the paste's history row, which the
//! store has from the paste's Enter, a paste the model drops excepted; and `held` is the
//! contract's own false, as Devin's adapter answers it: `took` reaches the engine first.
//! A turn the human begins as a paste goes in, after the readiness looked, is known only
//! when a look reads it.
//!
//! The schedules. Each seed makes looks in stretches of the daemon's second and of slow
//! passes of 5 to 30 s, over 20 to 80 min; the human's turns of a second to 20 min and
//! rests of a second to 10 min, alternating, each holding one look, a few, or lasting what
//! it lasts, beginning and ending a little before or after a look; the first paste dropped
//! half the time and the second a fifth; and up to two sleeps of 31 min to 3 h, from the
//! hand-over, from a look in the hold, over a turn's end, from a look at rest, or anywhere,
//! the turn the window was in ending near the first look after. The grid adds each
//! alignment of the hold's end with the looks after the hand-over, with no sleep or one.
//!
//! What holds. Never twice: once a paste the window did not drop is given, nothing more
//! is pasted. Its record shows it at most [`LAG_MS`] after its turn begins, and the record
//! is read before anything goes again, so a later paste is always a second copy Devin
//! runs: given while the first is queued (within Q of the end of the turn it waits
//! behind), while it runs, or before its record showed it. Never for good: a paste the
//! window dropped is pasted again within [`B_MS`] (41 min) of the end of the last turn
//! that could hold it, wherever the looks found the window at rest at least every
//! [`N_MS`] (5 min) over that time. Main (half an hour from the hand-over) and round 7
//! (from the last look that found it held) paste it again at the first look at rest past
//! half an hour, within 30 min + N of that end; B leaves a fix one more N, to count from
//! the first look that found it held no longer, and a minute.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::rc::Rc;
use std::sync::OnceLock;
use std::thread;

use cf_engine::delivery_text::delivery_text;
use cf_engine::testing::Context;
use cf_harness::records::Role;

const SECOND: i64 = 1_000;
const MINUTE: i64 = 60 * SECOND;
const HOUR: i64 = 60 * MINUTE;
/// The longest Devin takes to begin a paste it queued, once the turn before it ends.
const Q_MS: i64 = 5 * SECOND;
/// The latest a record shows a turn's prompt after the turn begins: under the arrival window.
const LAG_MS: i64 = 29_900;
/// How long from a turn's beginning Devin's adapter reads its window as in that turn, its
/// agent silent as it may be (the adapter's `TAKEN_IN_MS`).
const TAKEN_IN_MS: i64 = 30 * SECOND;
/// How soon a paste given at rest begins.
const AT_ONCE_MS: i64 = 100;
/// How soon the human's turn begins after the turn it waited for.
const FOLLOW_MS: i64 = 200;
/// The looks find the window at rest at least this often…
const N_MS: i64 = 5 * MINUTE;
/// …and a paste the window dropped goes again within this of the end of the last turn
/// that could hold it.
const B_MS: i64 = 30 * MINUTE + 2 * N_MS + MINUTE;
/// A stretch with no look longer than this is a sleep, for the timelines' kinds.
const SLEEP_MS: i64 = 30 * MINUTE;
/// The pastes the dispatcher makes of one message at most (the kit's `max_attempts`).
const ATTEMPTS: usize = 3;
/// The seeds explored, unless `CF_SCHEDULE_SEEDS` names another number (read by this
/// test alone, for a longer run by hand).
const SEEDS: u64 = 400;
/// The looks of a generated schedule, at most.
const LOOKS: usize = 300;
/// How far before or after a look the window changes, where it changes by a look.
const OFFSETS: [i64; 10] = [
    100, 300, 1_000, 2_000, 4_000, 4_900, 5_100, 10_000, 29_000, 31_000,
];
/// The runs one kind of failure may spend on shrinking its timeline.
const SHRINK_RUNS: usize = 150;

/// A seeded die (splitmix64): the same seed, the same schedule.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// From `low` to `high`, both included.
    fn range(&mut self, low: i64, high: i64) -> i64 {
        low + (self.next() % (high - low + 1) as u64) as i64
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    fn pick<T: Copy>(&mut self, from: &[T]) -> T {
        from[self.below(from.len())]
    }

    /// From `low` to `high`, as often a second as a minute as an hour.
    fn spread(&mut self, low: i64, high: i64) -> i64 {
        let unit = (self.next() >> 11) as f64 / (1_u64 << 53) as f64;
        (low as f64 * (high as f64 / low as f64).powf(unit)) as i64
    }
}

/// A turn the human begins in the window, when nothing holds it back.
#[derive(Clone, Copy, Debug)]
struct Planned {
    start: i64,
    end: i64,
    /// Its agent says nothing: it reads at rest once its prompt is old.
    silent: bool,
}

/// How long a paste's own turn runs.
#[derive(Clone, Copy, Debug)]
enum Length {
    For(i64),
    /// Until a little before the `look`th look after it begins.
    Before {
        look: usize,
        by: i64,
    },
}

/// What the window does with one paste.
#[derive(Clone, Copy, Debug)]
struct Handling {
    dropped: bool,
    /// How long after the turn it waits behind ends it begins.
    latency: i64,
    length: Length,
    silent: bool,
    /// How late its record shows it, after its turn ends.
    lag: i64,
}

/// A schedule: when the dispatcher looks, from the first look; the human's turns; and
/// what the window does with each paste, in order.
#[derive(Clone, Debug)]
struct Schedule {
    name: String,
    /// Made by the seeded generator, not the grid.
    seeded: bool,
    looks: Vec<i64>,
    turns: Vec<Planned>,
    handling: Vec<Handling>,
    /// A turn of the human's that begins as the first paste goes in, after the readiness
    /// looked and before the paste went in (the paste waits behind it): its end.
    race: Option<i64>,
    /// The human switches the window to a new conversation (`/new`) at the look after the
    /// first that found the paste taken in: Devin switches once the turn at work ends, and
    /// a paste queued behind it is taken in, runs and is answered in the conversation it
    /// went into (T-107, probed live), whose record, no window showing it, reads at rest
    /// but while it runs.
    switch: bool,
}

/// A turn the window ran, or runs, or will run: the human's, or a paste's.
#[derive(Clone, Copy, Debug)]
struct Turn {
    start: i64,
    end: i64,
    silent: bool,
    paste: Option<usize>,
}

/// A paste the dispatcher made.
#[derive(Clone, Copy, Debug)]
struct Paste {
    at: i64,
    dropped: bool,
    /// The end of the turn it waited behind, where it waited.
    behind: Option<i64>,
    /// Its own turn, in the window's.
    turn: Option<usize>,
    /// When its record shows it.
    shown: Option<i64>,
}

/// What a look at the window finds, as Devin's adapter reports it.
#[derive(Clone, Copy, Debug)]
struct Seen {
    busy: bool,
    /// The store has the paste's history row (`Window::took`).
    sent: bool,
    shown: bool,
}

/// Devin's window, as the model has it.
struct Window {
    looks: Vec<i64>,
    planned: Vec<Planned>,
    next: usize,
    handling: Vec<Handling>,
    turns: Vec<Turn>,
    pastes: Vec<Paste>,
    /// The end of the turn that begins as the first paste goes in (`Schedule::race`).
    race: Option<i64>,
}

impl Window {
    fn new(schedule: &Schedule) -> Self {
        Self {
            looks: schedule.looks.clone(),
            planned: schedule.turns.clone(),
            next: 0,
            handling: schedule.handling.clone(),
            turns: Vec::new(),
            pastes: Vec::new(),
            race: schedule.race,
        }
    }

    /// The human's turns begun by `at`, each after the one before it.
    fn begin_turns(&mut self, at: i64) {
        while let Some(planned) = self.planned.get(self.next) {
            let after = self
                .turns
                .last()
                .map_or(i64::MIN, |turn| turn.end + FOLLOW_MS);
            let start = planned.start.max(after);
            if start > at {
                return;
            }
            self.turns.push(Turn {
                start,
                end: start + planned.end - planned.start,
                silent: planned.silent,
                paste: None,
            });
            self.next += 1;
        }
    }

    /// A look at `at`: the record and the store as they are.
    fn look(&mut self, at: i64) -> Seen {
        self.begin_turns(at);
        let busy = self.turns.iter().any(|turn| {
            turn.start <= at && at < turn.end && (!turn.silent || at - turn.start < TAKEN_IN_MS)
        });
        let shown = self
            .pastes
            .iter()
            .any(|paste| paste.shown.is_some_and(|shown| shown <= at));
        // The store has the last paste's history row from its Enter on, one dropped excepted.
        let took = self
            .pastes
            .last()
            .is_some_and(|paste| !paste.dropped && paste.at < at);
        Seen {
            busy,
            sent: took,
            shown,
        }
    }

    /// Whether a paste's own turn runs at `at`: its record, read with no window open,
    /// reads at work.
    fn runs_a_paste(&self, at: i64) -> bool {
        self.turns
            .iter()
            .any(|turn| turn.paste.is_some() && turn.start <= at && at < turn.end)
    }

    /// The dispatcher pastes at `at`.
    fn paste(&mut self, at: i64) {
        let index = self.pastes.len();
        let handling = self.handling[index.min(self.handling.len() - 1)];
        // The human's turn that began between the readiness and the paste, at rest: no
        // look saw it when the paste went in, and the paste waits behind it.
        if let Some(end) = self.race.take() {
            if self.turns.last().is_none_or(|turn| turn.end <= at) {
                self.turns.push(Turn {
                    start: at + 1,
                    end,
                    silent: true,
                    paste: None,
                });
            }
        }
        let behind = self
            .turns
            .last()
            .map(|turn| turn.end)
            .filter(|end| *end > at);
        let start = behind.map_or(at + AT_ONCE_MS, |end| end + handling.latency);
        let end = match handling.length {
            Length::For(length) => start + length,
            Length::Before { look, by } => {
                let after = self.looks.partition_point(|time| *time <= start);
                self.looks
                    .get(after + look - 1)
                    .map_or(start + MINUTE, |time| time - by)
                    .max(start + AT_ONCE_MS)
            }
        };
        let turn = (!handling.dropped).then(|| {
            self.turns.push(Turn {
                start,
                end,
                silent: handling.silent,
                paste: Some(index),
            });
            self.turns.len() - 1
        });
        self.pastes.push(Paste {
            at,
            dropped: handling.dropped,
            behind,
            turn,
            shown: turn.map(|_| start + handling.lag),
        });
    }
}

/// One look of a run, as the adapter reported it.
#[derive(Clone, Copy, Debug)]
struct Row {
    at: i64,
    seen: Seen,
}

/// A schedule run: its looks, what the window did, and where the message ended.
#[derive(Clone, Debug)]
struct Outcome {
    schedule: Schedule,
    rows: Vec<Row>,
    turns: Vec<Turn>,
    pastes: Vec<Paste>,
    state: String,
    /// When the human switched the window to a new conversation, where it did.
    switched: Option<i64>,
}

/// The dispatcher looks at the window at each of `schedule`'s looks, a note from zeus to
/// the chief on its way.
fn run(schedule: &Schedule) -> Outcome {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let window = Rc::new(RefCell::new(Window::new(schedule)));
    let now = Rc::new(Cell::new(0));
    let (pasting, at) = (Rc::clone(&window), Rc::clone(&now));
    let adapter = Rc::downgrade(&context.adapter);
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(move |taking| {
        pasting.borrow_mut().paste(at.get());
        // The adapter's word on the paste as it goes in: its history row a moment behind,
        // not taken yet.
        if let Some(adapter) = adapter.upgrade() {
            adapter.with("chief", |agent| agent.took = false);
        }
        Box::pin(async move { Ok(taking()) })
    }));
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project.id, "zeus", "chief", "hello");
    let item = context
        .adapter
        .item(Role::User, &delivery_text(&context.message(note.id), &[]));
    let (mut clock, mut recorded) = (0, false);
    let mut rows = Vec::new();
    let mut state = note.state;
    // The conversation the window was switched away from, and whether a look found the
    // paste put to the agent.
    let (mut left, mut taken, mut switched) = (None, false, None);
    for &look in &schedule.looks {
        context.advance(look - clock);
        clock = look;
        now.set(look);
        if schedule.switch && taken && left.is_none() {
            left = Some(context.adapter.with("chief", |agent| agent.native.clone()));
            context.adapter.switch_to("chief", "native-new");
            switched = Some(look);
        }
        let seen = window.borrow_mut().look(look);
        let running = window.borrow().runs_a_paste(look);
        context.adapter.with("chief", |agent| {
            agent.settled = !seen.busy;
            agent.took = seen.sent;
            match &left {
                // The paste runs in the conversation it went into, and its record shows it.
                Some(left) => {
                    if seen.shown && !recorded {
                        agent
                            .records
                            .entry(left.clone())
                            .or_default()
                            .push(item.clone());
                    }
                    agent.records_settled.insert(left.clone(), !running);
                }
                None => {
                    if seen.shown && !recorded {
                        agent.items.push(item.clone());
                    }
                }
            }
        });
        recorded |= seen.shown;
        // Taken in: its own turn has begun.
        taken |= window
            .borrow()
            .pastes
            .last()
            .and_then(|paste| paste.turn)
            .is_some_and(|turn| window.borrow().turns[turn].start <= look);
        context.pass().unwrap();
        rows.push(Row { at: look, seen });
        state = context.message(note.id).state;
        if state != "queued" && state != "delivering" {
            break;
        }
    }
    let window = window.borrow();
    assert_eq!(
        context.message(note.id).attempts as usize,
        window.pastes.len(),
        "every paste the window was given is the note's ({})",
        schedule.name
    );
    Outcome {
        schedule: schedule.clone(),
        rows,
        turns: window.turns.clone(),
        pastes: window.pastes.clone(),
        state,
        switched,
    }
}

/// How the window's turns and rests fall among the looks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    /// Each turn and each rest holds one look.
    OneLook,
    /// Each holds a few.
    Steady,
    /// Each lasts what it lasts.
    Free,
    /// Any of those, each time.
    Mixed,
}

/// The looks of a generated schedule: stretches of the daemon's second, and of slow
/// passes of 5 to 30 s.
fn looks(dice: &mut Dice) -> Vec<i64> {
    let span = dice.range(20, 80) * MINUTE;
    let mut looks = vec![0];
    let mut at = 0;
    while at < span && looks.len() < LOOKS {
        if dice.chance(25) {
            for _ in 0..dice.range(3, 30) {
                at += SECOND;
                looks.push(at);
            }
        } else {
            let low = dice.range(5, 30);
            let high = dice.range(low, 30);
            for _ in 0..dice.range(3, 40) {
                at += dice.range(low * SECOND, high * SECOND);
                looks.push(at);
            }
        }
    }
    looks
}

/// Where a turn or a rest that begins at `at` ends.
fn boundary(dice: &mut Dice, looks: &[i64], at: i64, turn: bool, style: Style) -> i64 {
    let style = match style {
        Style::Mixed => dice.pick(&[Style::OneLook, Style::Steady, Style::Free]),
        style => style,
    };
    let first = looks.partition_point(|look| *look <= at);
    let anchored = match style {
        Style::OneLook => looks.get(first + 1).map(|after| {
            let room = after - looks[first] - 100;
            let by = dice.pick(&OFFSETS).min(room).max(100);
            if dice.chance(50) {
                looks[first] + by
            } else {
                after - by
            }
        }),
        Style::Steady => looks
            .get(first + dice.pick(&[0, 1, 1, 2, 3, 5]))
            .map(|look| {
                let by = dice.pick(&OFFSETS);
                if dice.chance(50) {
                    look - by
                } else {
                    look + by
                }
            }),
        Style::Free | Style::Mixed => None,
    };
    anchored
        .unwrap_or_else(|| {
            at + if turn {
                dice.spread(SECOND, 20 * MINUTE)
            } else {
                dice.spread(SECOND, 10 * MINUTE)
            }
        })
        .max(at + 100)
}

/// The human's turns over `looks`, the window in a turn at the first look or at rest.
fn turns(dice: &mut Dice, looks: &[i64]) -> Vec<Planned> {
    let style = dice.pick(&[Style::OneLook, Style::Steady, Style::Free, Style::Mixed]);
    let last = *looks.last().unwrap();
    let mut turns = Vec::new();
    let mut at = -dice.spread(SECOND, 10 * MINUTE);
    let mut turn = dice.chance(50);
    while at <= last {
        let next = boundary(dice, looks, at, turn, style);
        if turn {
            // The turn the window is in at the first look holds the paste where it is silent.
            let silent = dice.chance(if turns.is_empty() { 60 } else { 40 });
            turns.push(Planned {
                start: at,
                end: next,
                silent,
            });
        }
        turn = !turn;
        at = next;
    }
    turns
}

/// What the window does with each paste: the first dropped half the time, the second a
/// fifth, the last never.
fn handling(dice: &mut Dice) -> Vec<Handling> {
    (0..ATTEMPTS)
        .map(|attempt| Handling {
            dropped: match attempt {
                0 => dice.chance(50),
                1 => dice.chance(20),
                _ => false,
            },
            latency: dice.pick(&[100, 500, 1_000, 2_500, 4_000, Q_MS, Q_MS]),
            length: match dice.below(3) {
                0 => Length::For(dice.pick(&[1_000, 3_000, 8_000, 20_000])),
                1 => Length::For(dice.spread(SECOND, 20 * MINUTE)),
                _ => Length::Before {
                    look: dice.pick(&[1, 1, 2, 3]),
                    by: dice.pick(&OFFSETS),
                },
            },
            silent: dice.chance(40),
            lag: dice.pick(&[0, 1_000, 10_000, 20_000, 29_000, LAG_MS]),
        })
        .collect()
}

/// Where the first paste is expected, without it: the first look that finds the window at
/// rest, and when the turn that would hold it ends.
fn foreseen(looks: &[i64], turns: &[Planned], handling: &[Handling]) -> Option<(usize, i64)> {
    let mut window = Window::new(&Schedule {
        name: String::new(),
        seeded: true,
        looks: looks.to_vec(),
        turns: turns.to_vec(),
        handling: handling.to_vec(),
        race: None,
        switch: false,
    });
    for (index, &at) in looks.iter().enumerate() {
        if window.look(at).busy {
            continue;
        }
        let running = window.turns.last().filter(|turn| turn.end > at);
        let held_until = match running {
            Some(turn) => turn.end,
            None if !handling[0].dropped => {
                window.paste(at);
                window.turns.last().map_or(at, |turn| turn.end)
            }
            None => turns
                .iter()
                .find(|turn| turn.start > at)
                .map_or(at, |turn| turn.end),
        };
        return Some((index, held_until));
    }
    None
}

/// Looks stop for 31 min to 3 h, after a look chosen by phase: the hand-over, before any
/// look could find the paste held; a look in the hold; the last look before a turn's end;
/// a look at rest; any look. What the window does after a moment in the gap waits with
/// the looks, so a turn that held across it ends as near the next look as it would have.
fn sleep(dice: &mut Dice, looks: &mut [i64], turns: &mut [Planned], handling: &[Handling]) {
    let Some((handed, held_until)) = foreseen(looks, turns, handling).filter(|_| looks.len() > 1)
    else {
        return;
    };
    let last = looks.len() - 2;
    let phase = dice.below(7);
    let index = match phase {
        0..=2 => handed,
        3 => {
            let held = looks.partition_point(|look| *look < held_until);
            let first = (handed + 1).min(held.saturating_sub(1)).max(handed);
            dice.range(first as i64, held.saturating_sub(1).max(first) as i64) as usize
        }
        4 if !turns.is_empty() => {
            let turn = dice.pick(turns);
            looks
                .partition_point(|look| *look < turn.end)
                .saturating_sub(1)
        }
        5 => {
            let rests: Vec<usize> = (0..looks.len())
                .filter(|index| {
                    !turns
                        .iter()
                        .any(|turn| turn.start <= looks[*index] && looks[*index] < turn.end)
                })
                .collect();
            if rests.is_empty() {
                dice.below(looks.len())
            } else {
                dice.pick(&rests)
            }
        }
        _ => dice.below(looks.len()),
    }
    .min(last);
    let length = dice.spread(31 * MINUTE, 3 * HOUR);
    let split = dice.range(looks[index] + 1, looks[index + 1] - 1);
    for look in &mut looks[index + 1..] {
        *look += length;
    }
    for turn in turns.iter_mut() {
        if turn.start > split {
            turn.start += length;
        }
        if turn.end > split {
            turn.end += length;
        }
    }
    // The turn the window was in when the looks stopped ends a little before or after the
    // first look after: always from the hand-over or the hold, the turn that holds the paste.
    let (slept_at, wake) = (looks[index], looks[index + 1]);
    if phase <= 3 || dice.chance(50) {
        if let Some(turn) = turns
            .iter_mut()
            .find(|turn| turn.start <= slept_at && slept_at < turn.end)
        {
            let by = if dice.chance(70) {
                dice.pick(&OFFSETS[..5])
            } else {
                dice.pick(&OFFSETS)
            };
            turn.end = if dice.chance(50) {
                wake - by
            } else {
                wake + by
            }
            .max(slept_at + 100);
        }
    }
}

/// The schedule of `seed`.
fn generated(seed: u64) -> Schedule {
    let mut dice = Dice(seed);
    let mut looks = looks(&mut dice);
    let mut turns = turns(&mut dice, &looks);
    let handling = handling(&mut dice);
    for _ in 0..dice.pick(&[0, 1, 1, 1, 2]) {
        sleep(&mut dice, &mut looks, &mut turns, &handling);
    }
    Schedule {
        name: format!("seed {seed}"),
        seeded: true,
        looks,
        turns,
        handling,
        race: None,
        switch: false,
    }
}

/// Where the grid's looks sleep.
#[derive(Clone, Copy, Debug)]
enum Sleep {
    None,
    /// From the hand-over, before any look could find the paste held.
    AtTheHandOver,
    /// From the last look of the hold.
    InTheHold,
}

/// What the window does after the hold, in the grid.
#[derive(Clone, Copy, Debug)]
enum After {
    /// It rests, looked at every 15 s.
    Rests,
    /// Turns of 20 s and rests of 10 s, looked at every 15 s: one look a rest.
    OneLookRests,
    /// `slow_records`' short turns: rests of 20 s looked at every 5 s, turns of 10 s.
    ShortTurns,
}

/// What the paste meets as it goes in, in the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Meets {
    /// The window at rest.
    Rest,
    /// A silent turn the paste waits behind.
    Queued,
    /// A turn the human began between the window's readiness and the paste, which no
    /// look had seen when the paste went in (`Schedule::race`).
    Racing,
}

/// The grid: the window at rest at the hand-over, in a silent turn the paste waits
/// behind, or in one the human began as the paste went in; looks every second or every
/// 20 s in the hold; no sleep, or one from the hand-over or from the hold's last look;
/// and the paste run, with Devin's latest start and a prompt record or its earliest and
/// the latest record, the turn that holds it ending 0.3 s, 4.9 s or 20 s before the next
/// look, and the human's `/new` once the paste is taken or none; or dropped, followed by
/// rest, one look a rest, or short turns. A turn begun as
/// the paste went in that no look sees before it ends, a sleep from the hand-over, is the
/// reviews' (2026-10-18): the look that wakes finds the paste taken (`took`).
fn grid() -> Vec<Schedule> {
    let mut all = Vec::new();
    for meets in [Meets::Rest, Meets::Queued, Meets::Racing] {
        for every in [SECOND, 20 * SECOND] {
            for sleep in [Sleep::None, Sleep::AtTheHandOver, Sleep::InTheHold] {
                for before in [300, 4_900, 20_000] {
                    for handling in [(false, Q_MS, 0), (false, 100, LAG_MS)] {
                        let plain = cell(meets, every, sleep, before, handling, After::Rests);
                        all.push(Schedule {
                            name: format!("{}, and /new once it is taken", plain.name),
                            switch: true,
                            ..plain.clone()
                        });
                        all.push(plain);
                    }
                }
                // A dropped paste's runs are long: one alignment of the hold's end each.
                for after in [After::Rests, After::OneLookRests, After::ShortTurns] {
                    all.push(cell(meets, every, sleep, 4_900, (true, Q_MS, 0), after));
                }
            }
        }
    }
    all
}

/// One schedule of the grid.
fn cell(
    meets: Meets,
    every: i64,
    sleep: Sleep,
    before: i64,
    (dropped, latency, lag): (bool, i64, i64),
    after: After,
) -> Schedule {
    let mut looks = Vec::new();
    let mut turns = Vec::new();
    let mut race = None;
    let queued = meets == Meets::Queued;
    // Before the hand-over: a silent turn whose prompt the first look read 30 s before,
    // or rest, looked at every 10 s.
    let handed = if queued { 30 * SECOND } else { 0 };
    let mut at = 0;
    while at <= handed {
        looks.push(at);
        at += 10 * SECOND;
    }
    at = handed;
    // The hold: two minutes of looks, or none after a sleep from the hand-over.
    if !matches!(sleep, Sleep::AtTheHandOver) {
        while at < handed + 2 * MINUTE {
            at += every;
            looks.push(at);
        }
    }
    if !matches!(sleep, Sleep::None) {
        at += 31 * MINUTE;
    } else {
        at += every;
    }
    looks.push(at);
    // The turn that holds the paste ends `before` that look: the silent one it waits
    // behind, the one begun as it went in, its own, or the human's next one, which holds a
    // paste it dropped.
    let held_until = at - before;
    let length = if meets == Meets::Racing {
        race = Some(held_until);
        Length::For(3_000)
    } else if queued || dropped {
        let start = if queued {
            -90 * SECOND
        } else {
            handed + AT_ONCE_MS + 1_000
        };
        turns.push(Planned {
            start,
            end: held_until,
            silent: true,
        });
        Length::For(3_000)
    } else {
        Length::For(held_until - handed - AT_ONCE_MS)
    };
    let end = at + if dropped { 45 * MINUTE } else { 5 * MINUTE };
    let mut turn = false;
    while at < end {
        match (after, turn) {
            (After::Rests, _) => {
                at += 15 * SECOND;
                looks.push(at);
            }
            (After::OneLookRests, false) => {
                turns.push(Planned {
                    start: at + 5 * SECOND,
                    end: at + 25 * SECOND,
                    silent: false,
                });
                at += 15 * SECOND;
                looks.push(at);
            }
            (After::OneLookRests, true) => {
                at += 15 * SECOND;
                looks.push(at);
            }
            (After::ShortTurns, false) => {
                for _ in 0..4 {
                    at += 5 * SECOND;
                    looks.push(at);
                }
                turns.push(Planned {
                    start: at + 100,
                    end: at + 10 * SECOND + 100,
                    silent: false,
                });
            }
            (After::ShortTurns, true) => {
                at += 10 * SECOND;
                looks.push(at);
            }
        }
        turn = !turn;
    }
    let first = Handling {
        dropped,
        latency,
        length,
        silent: true,
        lag,
    };
    let then = Handling {
        dropped: false,
        latency: Q_MS,
        length: Length::For(3_000),
        silent: false,
        lag: 0,
    };
    Schedule {
        name: format!(
            "grid: {}, looks every {} s, sleep {sleep:?}, the hold ends {} s before a look, \
             {}, then {after:?}",
            match meets {
                Meets::Rest => "at rest",
                Meets::Queued => "queued behind a silent turn",
                Meets::Racing => "behind a turn begun as it went in",
            },
            every / SECOND,
            before as f64 / 1_000.0,
            if dropped {
                "dropped".to_owned()
            } else {
                format!(
                    "run {} s after, record {} s late",
                    latency as f64 / 1_000.0,
                    lag as f64 / 1_000.0
                )
            },
        ),
        seeded: false,
        looks,
        turns,
        handling: vec![first, then, then],
        race,
        switch: false,
    }
}

/// Every schedule explored, run once for both tests, on a few threads.
fn explored() -> &'static [Outcome] {
    static EXPLORED: OnceLock<Vec<Outcome>> = OnceLock::new();
    EXPLORED.get_or_init(|| {
        let seeds = cf_base::env::Env::from_process()
            .text("CF_SCHEDULE_SEEDS")
            .and_then(|seeds| seeds.parse().ok())
            .unwrap_or(SEEDS);
        let schedules: Vec<Schedule> = (0..seeds).map(generated).chain(grid()).collect();
        let threads = thread::available_parallelism().map_or(1, |count| count.get().min(4));
        let schedules = &schedules;
        thread::scope(|scope| {
            let running: Vec<_> = (0..threads)
                .map(|part| {
                    scope.spawn(move || {
                        schedules
                            .iter()
                            .skip(part)
                            .step_by(threads)
                            .map(run)
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            running
                .into_iter()
                .flat_map(|part| part.join().unwrap())
                .collect()
        })
    })
}

/// A schedule that breaks an invariant: its kind, and what happened.
struct Breach {
    kind: String,
    what: String,
}

/// Never twice: once a paste the window did not drop is given, nothing more is pasted.
fn twice(outcome: &Outcome) -> Option<Breach> {
    let first = outcome.pastes.iter().position(|paste| !paste.dropped)?;
    let landed = outcome.pastes[first];
    let again = *outcome.pastes.get(first + 1)?;
    let look = outcome.rows.iter().position(|row| row.at == again.at)?;
    let slept = look > 0 && again.at - outcome.rows[look - 1].at > SLEEP_MS;
    let taken_before = outcome.rows[..look]
        .iter()
        .any(|row| row.at > landed.at && row.seen.sent);
    let looks = match (slept, taken_before) {
        (true, false) => "a sleep from before any look found the paste taken",
        (true, true) => "a sleep after a look found the paste taken",
        (false, _) => "no sleep",
    };
    let turn = outcome.turns[landed.turn?];
    let (state, doing) = if again.at < turn.start {
        (
            "queued",
            format!(
                "was queued still (behind a turn that ended at {}, it began at {})",
                clock(landed.behind.unwrap_or(landed.at), &outcome.pastes),
                clock(turn.start, &outcome.pastes)
            ),
        )
    } else if again.at < turn.end {
        ("running", "was running".to_owned())
    } else {
        (
            "run, its record late",
            format!(
                "had run (until {}, {:.1} s before) and its record showed it only at {}",
                clock(turn.end, &outcome.pastes),
                (again.at - turn.end) as f64 / 1_000.0,
                clock(landed.shown?, &outcome.pastes)
            ),
        )
    };
    Some(Breach {
        kind: format!("{looks}; the first paste {state}"),
        what: format!(
            "paste {} went in at {} while paste {}, which the window did not drop, {doing}: \
             Devin runs the message twice",
            first + 2,
            clock(again.at, &outcome.pastes),
            first + 1
        ),
    })
}

/// When nothing can hold `paste` any more: the end of the first turn that ends after it,
/// or the paste itself where no turn does.
fn held_until(outcome: &Outcome, paste: &Paste) -> i64 {
    outcome
        .turns
        .iter()
        .find(|turn| turn.end > paste.at)
        .map_or(paste.at, |turn| turn.end)
}

/// Never for good: a dropped paste goes again within B of the end of the last turn that
/// could hold it, wherever the looks find the window at rest at least every N.
fn for_good(outcome: &Outcome) -> Option<Breach> {
    for (index, paste) in outcome.pastes.iter().enumerate() {
        if !paste.dropped {
            continue;
        }
        let again = outcome.pastes.get(index + 1).map(|paste| paste.at);
        let from = held_until(outcome, paste);
        let rests: Vec<&Row> = outcome
            .rows
            .iter()
            .filter(|row| row.at >= from && again.is_none_or(|again| row.at < again))
            .filter(|row| !row.seen.busy)
            .collect();
        let (mut start, mut last) = (from, from);
        for row in rests {
            if row.at - last > N_MS {
                start = row.at;
            }
            last = row.at;
            if row.at - start < B_MS {
                continue;
            }
            let span: Vec<&Row> = outcome
                .rows
                .iter()
                .filter(|seen| seen.at >= start && seen.at <= row.at)
                .collect();
            // The rest between two looks in a row that found the window at rest.
            let rest = |row: &Row| !row.seen.busy;
            let between: i64 = span
                .windows(2)
                .filter(|pair| rest(pair[0]) && rest(pair[1]))
                .map(|pair| pair[1].at - pair[0].at)
                .sum();
            let kind = if between < MINUTE {
                "busy and at-rest looks alternating, one look a rest"
            } else {
                "rests that two looks in a row saw, under half an hour of them"
            };
            return Some(Breach {
                kind: kind.to_owned(),
                what: format!(
                    "paste {} was dropped and nothing could hold it after {}; the looks found the \
                     window at rest at least every {} min from {} to {} ({} s of it between two \
                     looks in a row at rest), and it was not pasted again within {} min",
                    index + 1,
                    clock(from, &outcome.pastes),
                    N_MS / MINUTE,
                    clock(start, &outcome.pastes),
                    clock(row.at, &outcome.pastes),
                    between / SECOND,
                    B_MS / MINUTE
                ),
            });
        }
    }
    None
}

/// Never stuck: a message whose paste the record showed before the last look is
/// confirmed, whatever conversation the window shows by then (the reviews of 2026-10-18:
/// after a `/new`, the window was read for a paste its harness had queued in the
/// conversation it left, and the queue waited behind it for good).
fn stuck(outcome: &Outcome) -> Option<Breach> {
    let last = outcome.rows.last()?.at;
    let (index, paste) = outcome
        .pastes
        .iter()
        .enumerate()
        .find(|(_, paste)| paste.shown.is_some_and(|shown| shown < last))?;
    if outcome.state == "delivered" {
        return None;
    }
    let switched = if outcome.schedule.switch {
        "the window switched to another conversation"
    } else {
        "the window on its conversation"
    };
    Some(Breach {
        kind: format!("{switched}; the record showed the paste"),
        what: format!(
            "the record showed paste {} at {}, and the note was not confirmed by the last look, \
             at {}",
            index + 1,
            clock(paste.shown?, &outcome.pastes),
            clock(last, &outcome.pastes)
        ),
    })
}

/// `at`, from the hand-over.
fn clock(at: i64, pastes: &[Paste]) -> String {
    let at = at - pastes.first().map_or(0, |paste| paste.at);
    let sign = if at < 0 { '-' } else { '+' };
    let (ms, s) = (at.abs() % 1_000, at.abs() / 1_000);
    format!(
        "{sign}{}:{:02}:{:02}.{}",
        s / 3_600,
        s / 60 % 60,
        s % 60,
        ms / 100
    )
}

/// What `outcome` looked like: each look, each turn's start and end, each record, from
/// the hand-over.
fn timeline(outcome: &Outcome) -> String {
    let pastes = &outcome.pastes;
    let end = outcome.rows.last().map_or(0, |row| row.at) + Q_MS + LAG_MS;
    let mut lines: Vec<(i64, u8, String)> = Vec::new();
    let mut human = 0;
    for turn in &outcome.turns {
        let name = match turn.paste {
            Some(paste) => format!("paste {}'s own turn", paste + 1),
            None => {
                human += 1;
                format!("the human's turn {human}")
            }
        };
        let quiet = if turn.silent {
            ", its agent silent"
        } else {
            ""
        };
        lines.push((turn.start, 0, format!("{name} begins{quiet}")));
        lines.push((turn.end, 0, format!("{name} ends")));
    }
    if let Some(at) = outcome.switched {
        lines.push((at, 0, "the human runs /new in the window".to_owned()));
    }
    for (index, paste) in pastes.iter().enumerate() {
        if let Some(shown) = paste.shown {
            lines.push((shown, 0, format!("the record shows paste {}", index + 1)));
        }
    }
    for row in &outcome.rows {
        let mut said = format!(
            "look: {}{}{}",
            if row.seen.busy { "busy" } else { "at rest" },
            if row.seen.sent { ", taken" } else { "" },
            if row.seen.shown { ", shown" } else { "" }
        );
        for (index, paste) in pastes
            .iter()
            .enumerate()
            .filter(|(_, paste)| paste.at == row.at)
        {
            let how = if paste.dropped {
                "the window drops it".to_owned()
            } else if let Some(behind) = paste.behind {
                format!(
                    "queued behind the turn that ends at {}",
                    clock(behind, pastes)
                )
            } else {
                "Devin runs it at once".to_owned()
            };
            let _ = write!(said, "  → paste {} ({how})", index + 1);
        }
        lines.push((row.at, 1, said));
    }
    lines.retain(|(at, _, _)| *at <= end);
    lines.sort_by_key(|(at, order, _)| (*at, *order));
    let mut text = String::new();
    let mut before: Option<i64> = None;
    for (at, order, line) in lines {
        if order == 1 {
            if let Some(gap) = before
                .map(|before| at - before)
                .filter(|gap| *gap > SLEEP_MS)
            {
                let _ = writeln!(text, "{:>14}  (no look for {} min)", "", gap / MINUTE);
            }
            before = Some(at);
        }
        let _ = writeln!(text, "{:>14}  {line}", clock(at, pastes));
    }
    text
}

/// Whether each stretch between two looks of `schedule` is a pass, of 30 s at most, or a
/// sleep, of 31 min at least, as the generator makes them.
fn passes_and_sleeps(schedule: &Schedule) -> bool {
    schedule
        .looks
        .windows(2)
        .all(|pair| pair[1] - pair[0] <= 30 * SECOND || pair[1] - pair[0] >= 31 * MINUTE)
}

/// Whether `schedule` is any schedule at all: one look at least.
fn any(schedule: &Schedule) -> bool {
    !schedule.looks.is_empty()
}

/// `schedule` with fewer looks and turns, breaking `check` the same way and still
/// `fitting`: the turns no look falls in taken out, then chunks of the looks and of the
/// turns while the breach stays, within the runs it may spend.
fn shrink(
    schedule: &Schedule,
    check: fn(&Outcome) -> Option<Breach>,
    kind: &str,
    fitting: fn(&Schedule) -> bool,
) -> (Schedule, Outcome) {
    let keeps = |outcome: &Outcome| check(outcome).is_some_and(|breach| breach.kind == kind);
    // The turns no look falls in, all taken out at once.
    let unseen = |best: &mut Schedule, outcome: &mut Outcome| {
        let mut seen = best.clone();
        seen.turns.retain(|turn| {
            best.looks
                .iter()
                .any(|look| turn.start <= *look && *look < turn.end)
        });
        if seen.turns.len() < best.turns.len() {
            let tried = run(&seen);
            if keeps(&tried) {
                *best = seen;
                *outcome = tried;
            }
        }
    };
    let mut best = schedule.clone();
    let mut outcome = run(&best);
    let mut runs = 1;
    let mut progress = true;
    while progress && runs < SHRINK_RUNS {
        progress = false;
        for looks in [true, false] {
            if !looks {
                unseen(&mut best, &mut outcome);
                runs += 1;
            }
            let length = |schedule: &Schedule| {
                if looks {
                    schedule.looks.len()
                } else {
                    schedule.turns.len()
                }
            };
            let mut size = length(&best).div_ceil(2);
            while size >= 1 && runs < SHRINK_RUNS {
                let mut start = 0;
                while start < length(&best) && runs < SHRINK_RUNS {
                    let mut candidate = best.clone();
                    let cut = start..(start + size).min(length(&candidate));
                    if looks {
                        candidate.looks.drain(cut);
                    } else {
                        candidate.turns.drain(cut);
                    }
                    if !fitting(&candidate) {
                        start += size;
                        continue;
                    }
                    runs += 1;
                    let tried = run(&candidate);
                    if keeps(&tried) {
                        best = candidate;
                        outcome = tried;
                        progress = true;
                    } else {
                        start += size;
                    }
                }
                size /= 2;
            }
        }
    }
    unseen(&mut best, &mut outcome);
    (best, outcome)
}

/// Every schedule explored keeps `check`, or the test fails with one timeline of each
/// kind of breach, shrunk to schedules still `fitting`, the generator's own where it
/// found one.
fn holds(name: &str, check: fn(&Outcome) -> Option<Breach>, fitting: fn(&Schedule) -> bool) {
    let outcomes = explored();
    let mut kinds: BTreeMap<String, Vec<&Outcome>> = BTreeMap::new();
    for outcome in outcomes {
        if let Some(breach) = check(outcome) {
            kinds.entry(breach.kind).or_default().push(outcome);
        }
    }
    if kinds.is_empty() {
        return;
    }
    let mut text = format!(
        "{name}: {} of {} schedules break it\n",
        kinds.values().map(Vec::len).sum::<usize>(),
        outcomes.len()
    );
    for (kind, broken) in kinds {
        let seeded = broken
            .iter()
            .filter(|outcome| outcome.schedule.seeded)
            .count();
        let example = broken
            .iter()
            .filter(|outcome| outcome.schedule.seeded || seeded == 0)
            .min_by_key(|outcome| outcome.rows.len())
            .unwrap();
        let (shrunk, outcome) = shrink(&example.schedule, check, &kind, fitting);
        let breach = check(&outcome).unwrap();
        let _ = write!(
            text,
            "\n{kind}: {} schedules ({seeded} of the generator's, {} of the grid's). {}, \
             shrunk from {} looks and {} turns to {} and {}:\n{}✗ {} (the note is {} at the \
             last look)\n",
            broken.len(),
            broken.len() - seeded,
            example.schedule.name,
            example.schedule.looks.len(),
            example.schedule.turns.len(),
            shrunk.looks.len(),
            shrunk.turns.len(),
            timeline(&outcome),
            breach.what,
            outcome.state
        );
    }
    panic!("{text}");
}

#[test]
fn never_pastes_again_a_message_whose_paste_the_window_did_not_drop_whatever_the_looks() {
    // A pass longer than the arrival window would be no schedule the generator makes.
    holds("never twice", twice, passes_and_sleeps);
}

#[test]
fn confirms_a_message_whose_record_showed_its_paste_whatever_conversation_its_window_shows_by_then()
{
    holds("never stuck", stuck, any);
}

#[test]
fn pastes_again_within_41_minutes_a_paste_the_window_dropped_where_it_rests_every_5_minutes() {
    // The looks at rest every N are the invariant's own condition: a timeline shrinks to
    // looks minutes apart.
    holds("never for good", for_good, any);
}
