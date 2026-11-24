//! The fixed schedules: one for each finding of the four reviews of N17 (T-2132, T-2139, T-2150,
//! T-2160), each named after it. A case runs the real host and readers with the stand-in program
//! and drives the scheduler itself: the test says which actor runs, and for how long, and what each
//! finding needed to happen between two points of the host happens there. The window of a case is
//! the window of the random schedules (`walks`), so a plant that a fixed schedule catches is one
//! the random ones can too.

use std::sync::Arc;
use std::time::Duration;

use cf_harness::{EnterFor, Look, NoLook};
use cf_proto::agents::Harness;

use super::program::Echo;
use super::sched::{self, Actor, Sched, Stands};
use super::world::{State, World};
use crate::arbiter::{ArbiterError, Clock, Why};

/// A window, its service actors, and a script: the engine's part of the case, with the points
/// the test stops it at.
struct Case {
    world: Arc<World>,
    sched: Sched,
    reader: Actor,
    consumer: Actor,
    worker: Actor,
    script: Actor,
}

impl Case {
    fn new(harness: Harness, script: impl FnOnce(Arc<World>) + Send + 'static) -> Self {
        let world = World::new(harness, 1);
        let mut sched = Sched::new();
        let (reading, feeding, working, scripted) = (
            Arc::clone(&world),
            Arc::clone(&world),
            Arc::clone(&world),
            Arc::clone(&world),
        );
        let reader = sched.spawn("reader", move || reading.reader());
        let consumer = sched.spawn("consumer", move || feeding.consumer());
        let worker = sched.spawn("worker", move || working.worker());
        let script = sched.spawn("script", move || script(scripted));
        Self {
            world,
            sched,
            reader,
            consumer,
            worker,
            script,
        }
    }

    /// Runs `who` to the point `name`.
    fn to(&self, who: Actor, name: &'static str) {
        match self.sched.run_to(who, name) {
            Stands::At(_) => {}
            other => panic!(
                "{} did not reach {name}: {other:?}\n{}",
                self.sched.name(who),
                self.sched.trace()
            ),
        }
    }

    /// Steps `who` `times` times, as far as it goes.
    fn steps(&self, who: Actor, times: usize) {
        for _ in 0..times {
            if self.sched.alive(who) {
                self.sched.step(who);
            }
        }
    }

    /// Runs the actors but `held`, in turn, until `done` says so.
    fn run(&self, held: &[Actor], done: impl Fn(&Self) -> bool) {
        for _ in 0..400_000 {
            if done(self) {
                return;
            }
            for actor in [self.script, self.reader, self.consumer, self.worker] {
                if !held.contains(&actor) && self.sched.alive(actor) {
                    self.sched.step(actor);
                }
            }
        }
        panic!("the case does not end:\n{}", self.sched.trace());
    }

    /// Runs every actor until the script stands at the point `name`.
    fn until_script(&self, name: &'static str) {
        self.run(&[], |case| {
            matches!(case.sched.stands(case.script), Stands::At(at) if at == name)
        });
    }

    /// Runs every actor but `held` until the script stands at the point `name`.
    fn until_script_but(&self, name: &'static str, held: &[Actor]) {
        self.run(held, |case| {
            matches!(case.sched.stands(case.script), Stands::At(at) if at == name)
        });
    }

    /// Runs the reader until it has handed on what it counted: the read waits for the consumer.
    fn read_in_hand(&self) {
        self.run(&[self.script, self.consumer, self.worker], |case| {
            matches!(case.sched.stands(case.reader), Stands::Blocked(_)) && case.world.fed_len() > 0
        });
    }

    /// Runs the service actors, the script held, until everything the window printed is drawn.
    fn drawn(&self) {
        self.run(&[self.script], |case| {
            let (read, drawn) = sched::without_points(|| case.world.printed.counts().both());
            read == drawn && case.world.pty.empty() && case.world.jobs.lock().unwrap().is_empty()
        });
    }

    /// Runs every actor until the script ends.
    fn finish(&self) {
        self.run(&[], |case| !case.sched.alive(case.script));
        self.drawn();
    }

    fn taken(&self) -> Vec<u32> {
        self.world.program.lock().unwrap().taken.clone()
    }

    fn held(&self) -> Vec<u32> {
        self.world.program.lock().unwrap().held.clone()
    }

    fn rules(&self) {
        if let Err(broken) = self.world.check() {
            panic!("{broken}\n{}", self.sched.trace());
        }
    }

    fn end(self) {
        self.world.stop();
        self.run(&[], |case| {
            [case.reader, case.consumer, case.worker, case.script]
                .iter()
                .all(|actor| !case.sched.alive(*actor))
        });
        self.sched.join();
    }
}

fn each_harness(case: impl Fn(Harness)) {
    for harness in [Harness::Claude, Harness::Devin, Harness::Muse] {
        case(harness);
    }
}

/// The result a script keeps for the test.
type Kept<T> = Arc<std::sync::Mutex<Option<T>>>;

fn kept<T>() -> Kept<T> {
    Arc::new(std::sync::Mutex::new(None))
}

/// The window prints a line that changes what its screen shows (no key's echo).
fn prints(world: &World) {
    world.program.lock().unwrap().print_line(4, "something the window said");
}

/// A script: a look, a stop at `case.looked`, the paste of that look, and its answer kept.
fn paste_after_a_look(
    answer: &Kept<Result<(), ArbiterError>>,
) -> impl FnOnce(Arc<World>) + Send + 'static {
    let answer = Arc::clone(answer);
    move |world| {
        let look = world.look().expect("a look");
        sched::at("case.looked");
        *answer.lock().unwrap() = world.paste_on(&look);
    }
}

// ---- T-2132 ---------------------------------------------------------------------------------

/// T-2132: a read the host has counted and the screen has not yet: the Enter does not go, though
/// the consumer has not even taken the screen's lock. It goes once the screen has it.
#[test]
fn t2132_the_enter_while_a_read_waits_for_the_screens_lock() {
    each_harness(|harness| {
        let answer = kept();
        let case = Case::new(harness, paste_after_a_look(&answer));
        case.until_script("case.looked");
        // The paste is in the box, and the Enter waits for the window's output to be drawn.
        case.run(&[], |case| !case.held().is_empty());
        case.world.program.lock().unwrap().repaint();
        case.read_in_hand();
        case.to(case.consumer, "feed.start");
        case.steps(case.worker, 300);
        assert!(case.taken().is_empty(), "{harness:?}: no Enter while a read is not drawn");
        case.rules();
        case.finish();
        assert_eq!(case.taken(), [1], "{harness:?}");
        case.rules();
        case.end();
    });
}

/// T-2132: a window that prints without pause gets its Enter at the cap and its grace, as main's
/// does: not sooner, and not never.
#[test]
fn t2132_the_cap_goes_on_a_feed_that_does_not_end() {
    let answer = kept();
    let case = Case::new(Harness::Claude, paste_after_a_look(&answer));
    case.until_script("case.looked");
    case.run(&[], |case| !case.held().is_empty());
    let pasted = case.world.clock.elapsed();
    case.world.program.lock().unwrap().repaint();
    // The consumer never draws what the reader counts.
    case.run(&[case.consumer], |case| !case.sched.alive(case.script));
    let waited = case.world.clock.elapsed() - pasted;
    assert_eq!(case.taken(), [1]);
    assert!(
        waited >= Duration::from_millis(super::world::ENTER_TIMING.most_ms + 3_000),
        "the Enter went after {waited:?}, before the cap and its grace"
    );
    case.rules();
    case.end();
}

/// T-2132: a key's echo in two reads, the cursor's move and then the key: with the second counted
/// and not drawn, the screen is not the window's and there is no look.
#[test]
fn t2132_a_split_echo_is_no_look() {
    each_harness(|harness| {
        let looked = kept();
        let (answer, world) = (Arc::clone(&looked), ());
        let _ = world;
        let case = Case::new(harness, move |world| {
            world.echo_as([Echo::Split]);
            world.type_keys(b"X");
            sched::wait_until("the key is written", || {
                world.program.lock().unwrap().draft == "X"
            });
            sched::at("case.typed");
            *answer.lock().unwrap() = Some(world.look());
        });
        case.until_script("case.typed");
        // The cursor's move is read and drawn; the key is counted and held before the screen.
        case.run(&[case.script, case.consumer], |case| {
            let (read, drawn) = sched::without_points(|| case.world.printed.counts().both());
            read > drawn && case.world.pty.empty()
        });
        case.to(case.consumer, "feed.start");
        case.run(&[case.consumer], |case| !case.sched.alive(case.script));
        let outcome = looked.lock().unwrap().take().expect("the script looked");
        assert!(
            matches!(outcome, Err(NoLook::Behind)),
            "{harness:?}: a screen that lacks bytes the host read is no look: {outcome:?}"
        );
        case.end();
    });
}

/// T-2132: a panic in the feed ends the pane's generation: nothing is admitted against its screen.
#[test]
fn t2132_a_feed_that_panics_admits_nothing() {
    let answer = kept();
    let case = Case::new(Harness::Claude, paste_after_a_look(&answer));
    case.until_script("case.looked");
    case.world.printed.counts().break_feed();
    case.finish();
    let refused = answer.lock().unwrap().take().expect("a paste was asked").unwrap_err();
    assert!(matches!(refused, ArbiterError::InputFailed), "{refused:?}");
    assert!(case.held().is_empty() && case.taken().is_empty());
    case.end();
}

// ---- T-2139 ---------------------------------------------------------------------------------

/// T-2139: a read is counted the moment the pty gives it, before the activity lock: with the
/// reader stopped at that lock, the host already says output is pending.
#[test]
fn t2139_a_read_is_counted_before_the_activity_lock() {
    let case = Case::new(Harness::Claude, |_| {});
    case.world.program.lock().unwrap().repaint();
    // The reader's first lock is the activity time's, taken once the pty has given its bytes.
    case.to(case.reader, "lock");
    assert!(
        sched::without_points(|| case.world.printed.counts().pending()),
        "the count moves before the lock of the activity time"
    );
    case.end();
}

/// T-2139: the Enter's last check is made with the writer held, where it is written: a read
/// counted after the wait ended and before the write holds the Enter back.
#[test]
fn t2139_the_enter_after_the_final_check_at_the_writer() {
    each_harness(|harness| {
        let answer = kept();
        let case = Case::new(harness, paste_after_a_look(&answer));
        case.until_script("case.looked");
        case.run(&[], |case| !case.held().is_empty());
        // The Enter's wait is over: the worker stands at the writer for the Enter.
        case.run(&[], |case| matches!(case.sched.stands(case.worker), Stands::At("writer.locked")));
        case.world.program.lock().unwrap().repaint();
        case.to(case.reader, "took.after");
        case.steps(case.worker, 200);
        assert!(case.taken().is_empty(), "{harness:?}: the read counted before the write holds the Enter");
        case.rules();
        case.finish();
        assert_eq!(case.taken(), [1]);
        case.end();
    });
}

/// T-2139: an answer read after the first read of a key's echo (the cursor's move to where it is,
/// the box empty) and before the second (the key) is a stale one: the paste on it is refused while
/// the key is pending, and again once it is drawn, and the child gets the key and nothing more.
#[test]
fn t2139_a_look_read_before_the_echo_admits_no_paste_over_a_pending_or_a_drawn_x() {
    each_harness(|harness| {
        let (pending, drawn) = (kept(), kept());
        let (first, second) = (Arc::clone(&pending), Arc::clone(&drawn));
        let case = Case::new(harness, move |world| {
            // The window's echo comes late (the checks assume it comes within a second).
            world.program.lock().unwrap().late = true;
            world.echo_as([Echo::Prefix]);
            world.type_keys(b"X");
            sched::wait_until("the key is written", || {
                world.program.lock().unwrap().draft == "X"
            });
            sched::at("case.typed");
            // The window's first read of its echo comes a moment after the key.
            crate::arbiter::Clock::rest(&*world.clock, Duration::from_millis(20));
            sched::at("case.answered");
            crate::arbiter::Clock::rest(&*world.clock, Duration::from_millis(1_500));
            let look = world.look().expect("a look before the key's echo is read");
            sched::at("case.looked");
            *first.lock().unwrap() = world.paste_on(&look);
            sched::at("case.pending");
            *second.lock().unwrap() = world.paste_on(&look);
        });
        // The window printed the cursor's move (read and drawn), and the key's own read is in the
        // reader's hand, not counted.
        case.until_script_but("case.answered", &[case.reader, case.consumer]);
        case.run(&[case.script, case.consumer, case.worker], |case| case.world.fed_len() > 0);
        case.to(case.reader, "took.before");
        case.run(&[case.script, case.reader], |case| {
            let (read, drawn) = sched::without_points(|| case.world.printed.counts().both());
            read == drawn
                && case.world.fed_len() == 0
                && matches!(case.sched.stands(case.consumer), Stands::Blocked(_))
        });
        // Time passes; the look is taken then, with the key's read not yet counted.
        case.until_script_but("case.looked", &[case.reader, case.consumer]);
        // The key's read is counted now, and not yet drawn.
        case.run(&[case.script, case.consumer, case.worker], |case| case.world.fed_len() > 0);
        case.to(case.consumer, "feed.start");
        case.run(&[case.consumer], |case| {
            matches!(case.sched.stands(case.script), Stands::At("case.pending"))
        });
        // Drawn: the second paste is on the same look.
        case.drawn();
        case.finish();
        let refused = |kept: &Kept<Result<(), ArbiterError>>| kept.lock().unwrap().take();
        let first = refused(&pending).expect("asked");
        assert!(
            matches!(first, Err(ArbiterError::LookAgain(Why::OutputPending | Why::BoxChanged)) | Err(ArbiterError::Unsent)),
            "{harness:?}: {first:?}"
        );
        let second = refused(&drawn).expect("asked");
        assert!(
            matches!(second, Err(ArbiterError::LookAgain(_)) | Err(ArbiterError::Unsent)),
            "{harness:?}: {second:?}"
        );
        assert!(case.held().is_empty(), "{harness:?}: nothing pasted over the key");
        case.rules();
        case.end();
    });
}

/// T-2139: the recovery Enter (`pane.enter`) is never written while output is pending: it waits,
/// and at the grace it is refused; on a look taken once the screen has what was read, it goes.
#[test]
fn t2139_the_recovery_enter_while_pending() {
    each_harness(|harness| {
        let (refused, goes) = (kept(), kept());
        let (first, second) = (Arc::clone(&refused), Arc::clone(&goes));
        let case = Case::new(harness, move |world| {
            // A message sits in the box whose Enter the window dropped.
            world.program.lock().unwrap().swallow = 1;
            world.pass(false);
            crate::arbiter::Clock::rest(&*world.clock, Duration::from_secs(4));
            let look = world.look().expect("a look");
            sched::at("case.looked");
            *first.lock().unwrap() = world.enter_on(&look, EnterFor::PasteStillThere);
            sched::at("case.entered");
            let look = world.look().expect("a look once what was read is drawn");
            *second.lock().unwrap() = world.enter_on(&look, EnterFor::PasteStillThere);
        });
        case.until_script("case.looked");
        assert_eq!(case.held(), [1], "{harness:?}: the message sits in the box");
        // A byte is read and never drawn: the Enter waits the grace out and is refused.
        case.world.program.lock().unwrap().repaint();
        case.read_in_hand();
        case.run(&[case.consumer], |case| {
            matches!(case.sched.stands(case.script), Stands::At("case.entered"))
        });
        assert!(case.taken().is_empty(), "{harness:?}: no Enter was written while a byte was pending");
        let first = refused.lock().unwrap().take();
        assert!(
            matches!(first, Some(Err(ArbiterError::LookAgain(Why::OutputPending)))),
            "{harness:?}: {first:?}"
        );
        // Drawn: a look taken now licenses it.
        case.drawn();
        case.finish();
        let second = goes.lock().unwrap().take();
        assert!(matches!(second, Some(Ok(()))), "{harness:?}: the Enter goes once it is drawn: {second:?}");
        assert_eq!(case.taken(), [1], "{harness:?}");
        case.rules();
        case.end();
    });
}

/// T-2139: a doubled paste: the first Enter went at the cap of a window that never stopped
/// printing, and the engine, finding the message taken, pastes it no more.
#[test]
fn t2139_the_cap_never_doubles_a_paste() {
    let case = Case::new(Harness::Claude, |world| {
        world.pass(false);
        sched::at("case.pasted");
        for _ in 0..3 {
            crate::arbiter::Clock::rest(&*world.clock, Duration::from_secs(3));
            world.pass(false);
        }
        sched::at("case.passed");
    });
    // The paste is in the box; then the window prints without pause, and nothing is drawn.
    case.run(&[], |case| !case.held().is_empty());
    case.world.program.lock().unwrap().flood(50);
    case.run(&[case.consumer], |case| {
        matches!(case.sched.stands(case.script), Stands::At("case.passed"))
    });
    assert_eq!(case.taken(), [1], "taken once, at the cap");
    assert!(case.held().is_empty());
    let msgs = case.world.msgs.lock().unwrap().clone();
    assert_eq!(msgs[0].state, State::Done, "{msgs:?}");
    case.rules();
    case.end();
}

/// T-2139: the pair of counts is sampled once, the drawn count first and the read count after: a
/// byte read between the two loads makes the window pending, and never the reverse.
#[test]
fn t2139_the_counts_are_sampled_in_one_order() {
    let looked = kept();
    let answer = Arc::clone(&looked);
    let case = Case::new(Harness::Claude, move |world| {
        *answer.lock().unwrap() = Some(world.look());
    });
    case.until_script("both.drawn");
    // A read comes between the two loads.
    case.world.program.lock().unwrap().repaint();
    case.to(case.reader, "took.after");
    case.run(&[case.consumer], |case| !case.sched.alive(case.script));
    let outcome = looked.lock().unwrap().take().expect("a look was taken");
    assert!(
        matches!(outcome, Err(NoLook::Behind)),
        "a byte read after the drawn count was loaded makes it pending: {outcome:?}"
    );
    case.end();
}

// ---- T-2150 ---------------------------------------------------------------------------------

/// T-2150: a window that repaints at rest (colour resets and the cursor put where it is) leaves the
/// look good: the paste goes in, with no refusal.
#[test]
fn t2150_a_repaint_at_rest_leaves_the_look_good() {
    each_harness(|harness| {
        let answer = kept();
        let case = Case::new(harness, paste_after_a_look(&answer));
        case.until_script("case.looked");
        {
            let program = case.world.program.lock().unwrap();
            (0..5).for_each(|_| program.repaint());
        }
        // The repaints are drawn at rest, and the paste is asked after them.
        case.drawn();
        case.finish();
        let outcome = answer.lock().unwrap().take().expect("a paste was asked");
        assert!(outcome.is_ok(), "{harness:?}: {outcome:?}");
        assert_eq!(case.taken(), [1]);
        case.rules();
        case.end();
    });
}

// ---- T-2160 ---------------------------------------------------------------------------------

/// T-2160: the human typed X, the window drew it faint, and the reader read the box empty; the echo
/// then moved only the cursor. The cursor is in `seen`: the paste is refused, and the window gets
/// the key and nothing more.
#[test]
fn t2160_a_cursor_only_echo_after_the_look() {
    let answer = kept();
    let case = Case::new(Harness::Claude, {
        let answer = Arc::clone(&answer);
        move |world| {
            world.echo_as([Echo::Silent]);
            world.type_keys(b"X");
            sched::wait_until("the key is written", || world.program.lock().unwrap().draft == "X");
            sched::at("case.typed");
            // The window answers the key a moment after it: the box reads empty over the count.
            crate::arbiter::Clock::rest(&*world.clock, Duration::from_millis(20));
            sched::at("case.faint");
            crate::arbiter::Clock::rest(&*world.clock, Duration::from_millis(1_500));
            let look = world.look().expect("a look");
            sched::at("case.looked");
            *answer.lock().unwrap() = world.paste_on(&look);
        }
    });
    // The faint X is drawn, and nothing else of the echo: the box reads empty.
    case.until_script("case.faint");
    case.world.program.lock().unwrap().faint_key('X');
    case.drawn();
    case.until_script("case.looked");
    // The echo's cursor move comes after the look, and is drawn.
    case.world.program.lock().unwrap().cursor_past_key();
    case.drawn();
    case.finish();
    let outcome = answer.lock().unwrap().take().expect("a paste was asked");
    assert!(
        matches!(outcome, Err(ArbiterError::LookAgain(Why::BoxChanged)) | Err(ArbiterError::Unsent)),
        "{outcome:?}"
    );
    assert!(case.held().is_empty(), "the window got the key and nothing more");
    case.rules();
    case.end();
}

/// T-2160: a key typed and erased again since the look: the screen shows what it showed, the human
/// holds no unsent text, and still the paste is refused: their key's echo may not be drawn yet.
#[test]
fn t2160_a_key_typed_and_erased_since_the_look() {
    each_harness(|harness| {
        let answer = kept();
        let case = Case::new(harness, paste_after_a_look(&answer));
        case.until_script("case.looked");
        case.world.echo_as([Echo::Silent]);
        case.world.type_keys(b"x");
        case.world.type_keys(&[0x7f]);
        case.run(&[case.script], |case| case.world.jobs.lock().unwrap().is_empty());
        case.drawn();
        case.finish();
        let outcome = answer.lock().unwrap().take().expect("asked");
        assert!(matches!(outcome, Err(ArbiterError::Unsent)), "{harness:?}: {outcome:?}");
        assert!(case.held().is_empty(), "{harness:?}");
        case.rules();
        case.end();
    });
}

/// T-2160: an echo drawn while the paste is admitted: the feed holds the screen's lock, and the
/// host's check of the look waits for it; what it finds then is not the look's box.
#[test]
fn t2160_an_echo_drawn_during_admission() {
    each_harness(|harness| {
        let answer = kept();
        let case = Case::new(harness, paste_after_a_look(&answer));
        case.until_script("case.looked");
        prints(&case.world);
        case.read_in_hand();
        // The feed holds the screen's lock, the read counted and not drawn.
        case.to(case.consumer, "feed.locked");
        case.run(&[case.consumer], |case| matches!(case.sched.stands(case.worker), Stands::Blocked(_)));
        case.finish();
        let outcome = answer.lock().unwrap().take().expect("asked");
        assert!(
            matches!(outcome, Err(ArbiterError::LookAgain(_))),
            "{harness:?}: {outcome:?}"
        );
        assert!(case.held().is_empty(), "{harness:?}");
        case.end();
    });
}

/// T-2160: a wrap, a scroll and a resize change a line the reader would read: the paste is
/// refused; a resize that changes none is admitted.
#[test]
fn t2160_a_wrap_a_scroll_and_a_resize() {
    for change in ["wrap", "scroll", "resize"] {
        let answer = kept();
        let case = Case::new(Harness::Claude, paste_after_a_look(&answer));
        case.until_script("case.looked");
        match change {
            "wrap" => case.world.program.lock().unwrap().print_line(10, &"w".repeat(120)),
            "scroll" => case.world.program.lock().unwrap().scroll(),
            _ => {
                // A window that is resized draws its rules to the new width.
                case.world.screen.resize(24, 30);
                let mut program = case.world.program.lock().unwrap();
                program.rule = 30;
                program.redraw();
            }
        }
        case.drawn();
        case.finish();
        let outcome = answer.lock().unwrap().take().expect("asked");
        assert!(
            matches!(outcome, Err(ArbiterError::LookAgain(_))),
            "{change}: {outcome:?}"
        );
        assert!(case.held().is_empty(), "{change}");
        case.end();
    }
    // A resize that moves no line is no change.
    let answer = kept();
    let case = Case::new(Harness::Claude, paste_after_a_look(&answer));
    case.until_script("case.looked");
    case.world.screen.resize(24, 80);
    case.finish();
    let outcome = answer.lock().unwrap().take().expect("asked");
    assert!(matches!(outcome, Ok(())), "{outcome:?}");
    case.end();
}

/// T-2160: the arrival's look is the only look: a message the window never took, its box read at
/// 6 over 5 (no look), is pasted no more; at the next look the box holds it, and one Enter takes
/// it: one hand-over in all.
#[test]
fn t2160_the_arrivals_old_look_pastes_nothing_again() {
    each_harness(|harness| {
        let case = Case::new(harness, |world| {
            world.program.lock().unwrap().swallow = 1;
            world.pass(false);
            crate::arbiter::Clock::rest(&*world.clock, Duration::from_secs(70));
            sched::at("case.late");
            // A read in flight: no look, so nothing goes and its rest begins again.
            world.pass(false);
            sched::at("case.behind");
            crate::arbiter::Clock::rest(&*world.clock, Duration::from_secs(5));
            world.pass(false);
            crate::arbiter::Clock::rest(&*world.clock, Duration::from_secs(5));
            world.pass(false);
        });
        case.until_script("case.late");
        case.world.program.lock().unwrap().repaint();
        case.to(case.reader, "took.after");
        case.run(&[case.consumer], |case| {
            matches!(case.sched.stands(case.script), Stands::At("case.behind"))
        });
        assert_eq!(case.held(), [1], "{harness:?}: not pasted again over the look that was behind");
        case.finish();
        assert_eq!(case.taken(), [1], "{harness:?}: one Enter took it");
        case.rules();
        case.end();
    });
}

/// T-2160: a look that fails (the host did not answer) presses nothing and pastes nothing.
#[test]
fn t2160_a_failed_look_presses_nothing() {
    let case = Case::new(Harness::Claude, |world| {
        world.program.lock().unwrap().swallow = 1;
        world.pass(false);
        crate::arbiter::Clock::rest(&*world.clock, Duration::from_secs(70));
        for _ in 0..5 {
            world.pass(true);
            crate::arbiter::Clock::rest(&*world.clock, Duration::from_secs(3));
        }
    });
    case.finish();
    assert_eq!(case.held(), [1], "the message waits in its box, untouched");
    assert!(case.taken().is_empty());
    let msgs = case.world.msgs.lock().unwrap().clone();
    assert!(matches!(msgs[0].state, State::Pasted(_)), "{msgs:?}");
    case.end();
}
