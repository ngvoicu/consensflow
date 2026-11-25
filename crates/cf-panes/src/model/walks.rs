//! The random schedules: a seed makes a scenario (what the human types, what the window prints,
//! which looks fail, how many messages are sent) and the choice of who runs at every point, and a
//! schedule replays from its seed. `CF_MODEL_WALKS` raises the count (2,000 a harness by default),
//! `CF_MODEL_FROM` moves where the seeds begin, and `CF_MODEL_SEED` replays one.

// A test of the model: its seeds and counts are read from the environment it is run in.
#![allow(clippy::disallowed_methods)]

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use cf_proto::agents::Harness;

use super::program::Echo;
use super::sched::{Actor, Sched, Stands};
use super::world::World;

pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub(crate) fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// What the human does, in order.
#[derive(Clone, Debug)]
enum Person {
    /// A key, echoed in a style.
    Key(char, Echo),
    Erase,
    /// A line and its Enter in one burst.
    Line(String),
    /// Ctrl+W, Ctrl+U or an arrow: keys the count cannot follow, or that clear.
    Control(&'static [u8]),
    /// A dialog of the window's own is shown, and the person answers it with a key.
    Answer(u64),
    Pause(u64),
}

/// What the window prints and does on its own, in order.
#[derive(Clone, Debug)]
enum Feed {
    Repaint(usize),
    Line(u16, String),
    Wrap,
    Resize(u16),
    Scroll,
    Flood(usize),
    /// The window drops the next Enter: it came before it had read the paste.
    Swallow,
    /// The window stops taking its input, and goes on printing.
    Stall,
    /// It takes all the input that waits.
    Resume,
    Pause(u64),
}

struct Plan {
    messages: u32,
    person: Vec<Person>,
    feed: Vec<Feed>,
    blind: Vec<bool>,
}

impl Plan {
    fn of(rng: &mut Rng) -> Self {
        let messages = 1 + rng.below(3) as u32;
        let mut person = Vec::new();
        for _ in 0..rng.below(24) {
            person.push(match rng.below(13) {
                0..=4 => Person::Key(
                    char::from(b'a' + rng.below(26) as u8),
                    [Echo::Whole, Echo::Split, Echo::CursorOnly, Echo::Prefix][rng.below(4)],
                ),
                5..=6 => Person::Erase,
                7 => Person::Line("a line".to_owned()),
                8..=9 => Person::Control([&b"\x17"[..], b"\x15", b"\x1b[D"][rng.below(3)]),
                10 => Person::Answer(20 + rng.below(3_000) as u64),
                _ => Person::Pause(200 + rng.below(4_000) as u64),
            });
        }
        let mut feed = Vec::new();
        for _ in 0..rng.below(24) {
            feed.push(match rng.below(14) {
                0..=2 => Feed::Repaint(1 + rng.below(6)),
                3 => Feed::Line(
                    2 + rng.below(14) as u16,
                    "output ".repeat(1 + rng.below(14)),
                ),
                4 => Feed::Wrap,
                5 => Feed::Resize(70 + rng.below(40) as u16),
                6 => Feed::Scroll,
                7 => Feed::Flood(2 + rng.below(30)),
                8 => Feed::Swallow,
                9 => Feed::Stall,
                10 => Feed::Resume,
                _ => Feed::Pause(100 + rng.below(3_000) as u64),
            });
        }
        let blind = (0..40).map(|_| rng.below(6) == 0).collect();
        Self {
            messages,
            person,
            feed,
            blind,
        }
    }
}

/// How many steps a schedule may take before it is said not to end.
const LIMIT: usize = 60_000;

/// Runs the schedule of `seed` on `harness`: whether every rule held, or what broke and the steps
/// that led to it.
pub(crate) fn walk(harness: Harness, seed: u64) -> Result<(), String> {
    let mut rng = Rng::new(seed);
    let plan = Plan::of(&mut rng);
    let world = World::new(harness, plan.messages);
    let mut sched = Sched::new();
    let mut services = Vec::new();
    let (reading, feeding, working) = (Arc::clone(&world), Arc::clone(&world), Arc::clone(&world));
    services.push(sched.spawn("reader", move || reading.reader()));
    services.push(sched.spawn("consumer", move || feeding.consumer()));
    services.push(sched.spawn("worker", move || working.worker()));
    let (typing, printing, engine) = (Arc::clone(&world), Arc::clone(&world), Arc::clone(&world));
    let person = plan.person.clone();
    let human = sched.spawn("person", move || human(&typing, &person));
    let feed = plan.feed.clone();
    let window = sched.spawn("program", move || window(&printing, &feed));
    let blind = plan.blind.clone();
    let engine_actor = sched.spawn("engine", move || engine.engine(blind));
    let producers = [human, window];

    let mut steps = 0;
    let mut stuck = 0;
    let outcome = loop {
        let alive: Vec<Actor> = sched
            .actors()
            .into_iter()
            .filter(|actor| sched.alive(*actor))
            .collect();
        if alive.is_empty() {
            break Ok(());
        }
        if producers.iter().all(|actor| !sched.alive(*actor)) {
            world.settle();
        }
        if !sched.alive(engine_actor) && !world.settled.load(Ordering::SeqCst) {
            world.settle();
        }
        if !sched.alive(engine_actor) {
            world.stop();
        }
        let ready: Vec<Actor> = alive
            .iter()
            .copied()
            .filter(|actor| matches!(sched.stands(*actor), Stands::At(_)))
            .collect();
        let pool = if ready.is_empty() || rng.below(8) == 0 {
            &alive
        } else {
            &ready
        };
        let pick = pool[rng.below(pool.len())];
        let before = sched.stands(pick);
        let after = sched.step(pick);
        stuck = if matches!((&before, &after), (Stands::Blocked(_), Stands::Blocked(_)))
            && ready.is_empty()
        {
            stuck + 1
        } else {
            0
        };
        if let Err(broken) = world.check() {
            break Err(broken);
        }
        steps += 1;
        if steps > LIMIT || stuck > 5_000 {
            break Err(format!(
                "the schedule does not end after {steps} steps ({stuck} waiting)"
            ));
        }
    };
    let outcome = outcome.and_then(|()| world.finish());
    let trace = sched.trace();
    world.stop();
    if outcome.is_err() {
        // Leave the actors to be dropped with the process: they hold nothing that matters.
        std::mem::forget(sched);
    } else {
        sched.join();
    }
    outcome.map_err(|broken| {
        let tail: Vec<&str> = match std::env::var("CF_MODEL_HEAD").ok().and_then(|n| n.parse().ok()) {
            Some(count) => trace.lines().take(count).collect(),
            None => trace.lines().rev().take(40).collect(),
        };
        format!(
            "harness {harness:?}, seed {seed}: {broken}\nthe engine's passes:\n{}\nthe last steps, newest first:\n{}",
            world.log.lock().unwrap().iter().rev().take(25).cloned().collect::<Vec<_>>().join("\n"),
            tail.join("\n")
        )
    })
}

/// What the human does: each step a job for the pane's worker, the window's echo of it chosen.
fn human(world: &Arc<World>, steps: &[Person]) {
    for step in steps {
        match step {
            Person::Key(key, echo) => {
                world.echo_as([*echo]);
                world.type_keys(key.to_string().as_bytes());
            }
            Person::Erase => world.type_keys(&[0x7f]),
            Person::Line(line) => world.type_keys(format!("{line}\r").as_bytes()),
            Person::Control(keys) => world.type_keys(keys),
            Person::Answer(after) => {
                world.program.lock().unwrap().show_dialog();
                crate::arbiter::Clock::rest(&*world.clock, Duration::from_millis(*after));
                world.type_keys(b"2");
            }
            Person::Pause(millis) => {
                crate::arbiter::Clock::rest(&*world.clock, Duration::from_millis(*millis));
            }
        }
        super::sched::at("person.step");
    }
}

/// What the window does on its own. The program's lock is never held across a point: the
/// scheduler reads the truth between two steps.
fn window(world: &Arc<World>, steps: &[Feed]) {
    for step in steps {
        match step {
            Feed::Repaint(times) => {
                let program = world.program.lock().unwrap();
                (0..*times).for_each(|_| program.repaint());
            }
            Feed::Line(row, text) => world.program.lock().unwrap().print_line(*row, text),
            Feed::Wrap => world
                .program
                .lock()
                .unwrap()
                .print_line(10, &"w".repeat(120)),
            Feed::Resize(cols) => {
                world.screen.resize(24, *cols);
                world.program.lock().unwrap().redraw();
            }
            Feed::Scroll => world.program.lock().unwrap().scroll(),
            Feed::Flood(reads) => world.program.lock().unwrap().flood(*reads),
            Feed::Swallow => world.program.lock().unwrap().swallow += 1,
            Feed::Stall => world.program.lock().unwrap().stall(),
            Feed::Resume => world.program.lock().unwrap().resume(),
            Feed::Pause(millis) => {
                crate::arbiter::Clock::rest(&*world.clock, Duration::from_millis(*millis));
            }
        }
        super::sched::at("window.step");
    }
    // A window that fell behind goes on in the end.
    world.program.lock().unwrap().resume();
}

fn walks(harness: Harness) {
    if let Some(seed) = std::env::var("CF_MODEL_SEED")
        .ok()
        .and_then(|seed| seed.parse::<u64>().ok())
    {
        if let Err(broken) = walk(harness, seed) {
            panic!("{broken}");
        }
        return;
    }
    let count = std::env::var("CF_MODEL_WALKS")
        .ok()
        .and_then(|count| count.parse::<u64>().ok())
        .unwrap_or(2_000);
    let from = std::env::var("CF_MODEL_FROM")
        .ok()
        .and_then(|from| from.parse::<u64>().ok())
        .unwrap_or(1);
    let mut failed = Vec::new();
    for seed in from..from + count {
        if let Err(broken) = walk(harness, seed) {
            failed.push(broken);
            if failed.len() >= 3 {
                break;
            }
        }
    }
    assert!(
        failed.is_empty(),
        "{} of the schedules from seed {from} broke a rule:\n{}",
        failed.len(),
        failed.join("\n\n")
    );
}

#[test]
fn random_schedules_hold_the_rules_in_claude_code() {
    walks(Harness::Claude);
}

#[test]
fn random_schedules_hold_the_rules_in_devin() {
    walks(Harness::Devin);
}

#[test]
fn random_schedules_hold_the_rules_in_muse_code() {
    walks(Harness::Muse);
}
