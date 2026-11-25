//! The fixed schedules: one for each finding of the four reviews of N17 (T-2132, T-2139, T-2150,
//! T-2160), each named after it. A case runs the real host and readers with the stand-in program
//! and drives the scheduler itself: the test says which actor runs, and for how long, and what each
//! finding needed to happen between two points of the host happens there. The window of a case is
//! the window of the random schedules (`walks`), so a plant that a fixed schedule catches is one
//! the random ones can too.

use std::sync::Arc;
use std::time::Duration;

use cf_harness::{EnterFor, NoLook};
use cf_proto::agents::Harness;

use super::program::Echo;
use super::sched::{self, Actor, Sched, Stands};
use super::world::{State, World};
use crate::arbiter::{ArbiterError, Why};

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
        self.run(
            &[],
            |case| matches!(case.sched.stands(case.script), Stands::At(at) if at == name),
        );
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
    world
        .program
        .lock()
        .unwrap()
        .print_line(4, "something the window said");
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
        assert!(
            case.taken().is_empty(),
            "{harness:?}: no Enter while a read is not drawn"
        );
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
        let answer = Arc::clone(&looked);
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
    let refused = answer
        .lock()
        .unwrap()
        .take()
        .expect("a paste was asked")
        .unwrap_err();
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
    // The reader's first lock is its gate's; the next is the activity time's, taken once the
    // pty has given its bytes.
    case.to(case.reader, "lock");
    case.steps(case.reader, 1);
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
        case.run(&[], |case| {
            matches!(case.sched.stands(case.worker), Stands::At("writer.locked"))
        });
        case.world.program.lock().unwrap().repaint();
        case.to(case.reader, "took.after");
        case.steps(case.worker, 200);
        assert!(
            case.taken().is_empty(),
            "{harness:?}: the read counted before the write holds the Enter"
        );
        case.rules();
        case.finish();
        assert_eq!(case.taken(), [1]);
        case.end();
    });
}

/// T-2233: the window printed after the look and the host has not read it, or has read it and
/// not drawn it: the paste on that look is refused, to be looked at again, with the person's
/// count at zero.
#[test]
fn t2233_printed_since_the_look_a_paste() {
    each_harness(|harness| {
        for read in [false, true] {
            let answer = kept();
            let case = Case::new(harness, paste_after_a_look(&answer));
            case.until_script("case.looked");
            prints(&case.world);
            let held = if read {
                case.read_in_hand();
                vec![case.consumer]
            } else {
                vec![case.reader, case.consumer]
            };
            case.run(&held, |case| !case.sched.alive(case.script));
            case.finish();
            let outcome = answer.lock().unwrap().take().expect("asked");
            assert!(
                matches!(outcome, Err(ArbiterError::LookAgain(Why::OutputPending))),
                "{harness:?}, read {read}: {outcome:?}"
            );
            assert!(case.held().is_empty(), "{harness:?}, read {read}");
            case.rules();
            case.end();
        }
    });
}

/// T-2160: the digest of what the screen shows and the check that the pane is current are made
/// with the screen's lock held, both: a feed that drew the window's output between them would
/// leave a digest of the screen before it and a pane that reads as current.
#[test]
fn t2160_the_digest_and_the_pending_check_are_under_one_lock() {
    each_harness(|harness| {
        let answer = kept();
        let case = Case::new(harness, paste_after_a_look(&answer));
        case.until_script("case.looked");
        // The host has made the digest of the screen as the look saw it, and not yet asked
        // whether the pane is current.
        case.run(&[], |case| {
            matches!(case.sched.stands(case.worker), Stands::At("admit.seen"))
        });
        // Now the window prints, and what it printed is read and offered to the screen.
        prints(&case.world);
        for _ in 0..300 {
            case.steps(case.reader, 1);
            case.steps(case.consumer, 1);
        }
        case.finish();
        let outcome = answer.lock().unwrap().take().expect("asked");
        assert!(
            matches!(outcome, Err(ArbiterError::LookAgain(_))),
            "{harness:?}: {outcome:?}"
        );
        assert!(case.held().is_empty(), "{harness:?}");
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
        assert!(
            case.taken().is_empty(),
            "{harness:?}: no Enter was written while a byte was pending"
        );
        let first = refused.lock().unwrap().take();
        assert!(
            matches!(
                first,
                Some(Err(ArbiterError::LookAgain(Why::OutputPending)))
            ),
            "{harness:?}: {first:?}"
        );
        // Drawn: a look taken now licenses it.
        case.drawn();
        case.finish();
        let second = goes.lock().unwrap().take();
        assert!(
            matches!(second, Some(Ok(()))),
            "{harness:?}: the Enter goes once it is drawn: {second:?}"
        );
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

/// T-2233: bytes in hand. A reader stopped between its read and its count holds the gate: a
/// look waits for it, and then says the output is pending.
#[test]
fn t2233_a_look_waits_for_bytes_in_hand() {
    each_harness(|harness| {
        let looked = kept();
        let answer = Arc::clone(&looked);
        let case = Case::new(harness, move |world| {
            sched::at("case.looking");
            *answer.lock().unwrap() = Some(world.look());
        });
        case.to(case.script, "case.looking");
        case.world.program.lock().unwrap().repaint();
        case.to(case.reader, "took.before");
        // The look is asked; it cannot answer while the reader holds what it read.
        case.steps(case.script, 50);
        assert!(
            looked.lock().unwrap().is_none(),
            "{harness:?}: answered with bytes in hand"
        );
        case.run(&[case.consumer], |case| !case.sched.alive(case.script));
        let outcome = looked.lock().unwrap().take().expect("a look was taken");
        assert!(
            matches!(outcome, Err(NoLook::Behind)),
            "{harness:?}: {outcome:?}"
        );
        case.end();
    });
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

/// T-2160: after the look the window moves only its cursor. The cursor is in `seen`: the paste
/// is refused.
#[test]
fn t2160_a_cursor_only_echo_after_the_look() {
    each_harness(|harness| {
        let answer = kept();
        let case = Case::new(harness, paste_after_a_look(&answer));
        case.until_script("case.looked");
        case.world.program.lock().unwrap().move_cursor(5);
        case.drawn();
        case.finish();
        let outcome = answer.lock().unwrap().take().expect("a paste was asked");
        assert!(
            matches!(outcome, Err(ArbiterError::LookAgain(Why::BoxChanged))),
            "{harness:?}: {outcome:?}"
        );
        assert!(case.held().is_empty(), "{harness:?}");
        case.rules();
        case.end();
    });
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
        // Both keys are written and counted, and the erase's echo is drawn.
        // Both keys are written whole, and the worker waits for more.
        case.run(&[case.script], |case| {
            case.world.arbiter.typed_count(&case.world.key) >= Some(2)
                && matches!(case.sched.stands(case.worker), Stands::Blocked("a job"))
        });
        case.drawn();
        case.finish();
        let outcome = answer.lock().unwrap().take().expect("asked");
        assert!(
            matches!(outcome, Err(ArbiterError::Unsent)),
            "{harness:?}: {outcome:?}"
        );
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
        case.run(&[case.consumer], |case| {
            matches!(case.sched.stands(case.worker), Stands::Blocked(_))
        });
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
            "wrap" => case
                .world
                .program
                .lock()
                .unwrap()
                .print_line(10, &"w".repeat(120)),
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
        assert_eq!(
            case.held(),
            [1],
            "{harness:?}: not pasted again over the look that was behind"
        );
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

// ---- T-2230 ---------------------------------------------------------------------------------

/// A script: the person types `X` into a window that does not draw it, the window prints
/// something else (`said`), and `after` later a look is taken and its paste asked.
fn paste_after_an_undrawn_x(
    said: bool,
    after: Duration,
    answers: &Kept<Vec<Option<Result<(), ArbiterError>>>>,
) -> impl FnOnce(Arc<World>) + Send + 'static {
    let answers = Arc::clone(answers);
    move |world| {
        world.echo_as([Echo::Silent]);
        world.type_keys(b"X");
        sched::wait_until("the key is written", || {
            world.program.lock().unwrap().draft == "X"
        });
        crate::arbiter::Clock::rest(&*world.clock, Duration::from_millis(50));
        if said {
            world.program.lock().unwrap().colour_reset();
        }
        let mut asked = Vec::new();
        for wait in [after, Duration::from_millis(4_600)] {
            crate::arbiter::Clock::rest(&*world.clock, wait);
            sched::at("case.looking");
            asked.push(world.look().ok().and_then(|look| world.paste_on(&look)));
        }
        *answers.lock().unwrap() = Some(asked);
    }
}

/// T-2230 3: the person's `X` is in the window's box and not drawn, and the window printed a
/// colour reset since: no length of time and no other output sets their key aside. Nothing is
/// pasted, at 1.4 s nor at 6 s.
#[test]
fn t2230_a_late_echo_beside_other_output_holds_the_paste() {
    each_harness(|harness| {
        let answers = kept();
        let case = Case::new(
            harness,
            paste_after_an_undrawn_x(true, Duration::from_millis(1_350), &answers),
        );
        case.finish();
        case.rules();
        assert!(
            case.held().is_empty(),
            "{harness:?}: pasted over the person's X"
        );
        case.end();
    });
}

/// T-2230 3: the same with no output since the key at all, at 5.2 s and past it.
#[test]
fn t2230_a_silent_late_echo_holds_the_paste() {
    each_harness(|harness| {
        let answers = kept();
        let case = Case::new(
            harness,
            paste_after_an_undrawn_x(false, Duration::from_millis(5_200), &answers),
        );
        case.finish();
        case.rules();
        assert!(
            case.held().is_empty(),
            "{harness:?}: pasted over the person's X"
        );
        case.end();
    });
}

// ---- T-2233: the amendment's schedules ------------------------------------------------------

/// What a script does with the person's keys, and then takes a pass of the engine.
fn types(world: &World, keys: &[u8]) {
    let before = world.arbiter.snapshot(&world.key).expect("the pane").typed;
    world.type_keys(keys);
    sched::wait_until("the key is counted", || {
        world
            .arbiter
            .snapshot(&world.key)
            .is_ok_and(|now| now.typed > before)
    });
}

/// A moment past the second a paste waits after the person's last key.
fn a_while(world: &World) {
    crate::arbiter::Clock::rest(&*world.clock, Duration::from_millis(1_500));
}

/// Runs a case whose script is `script`, to its end, and gives what the program holds and took.
fn run_script(harness: Harness, script: impl FnOnce(Arc<World>) + Send + 'static) -> Case {
    let case = Case::new(harness, script);
    case.finish();
    case
}

/// Everything the window shows once its output is drawn.
fn settled(world: &World) {
    sched::wait_until("what the window printed is drawn", || {
        sched::without_points(|| {
            let (read, drawn) = world.printed.counts().both();
            read == drawn && world.pty.empty()
        })
    });
}

#[test]
fn t2233_erased_and_seen_the_message_is_taken() {
    each_harness(|harness| {
        let case = run_script(harness, |world| {
            types(&world, b"a");
            types(&world, b"b");
            settled(&world);
            types(&world, b"\x17");
            settled(&world);
            a_while(&world);
            world.pass(false);
        });
        assert_eq!(
            case.taken(),
            [1],
            "{harness:?}: {:?}",
            case.world.log.lock().unwrap()
        );
        case.rules();
        case.end();
    });
}

#[test]
fn t2233_a_character_after_the_erase_holds() {
    each_harness(|harness| {
        let case = run_script(harness, |world| {
            types(&world, b"a");
            types(&world, b"b");
            settled(&world);
            types(&world, b"\x17");
            settled(&world);
            world.program.lock().unwrap().stall();
            types(&world, b"X");
            a_while(&world);
            world.pass(false);
            world.program.lock().unwrap().resume();
        });
        case.rules();
        assert!(
            case.held().is_empty() && case.taken().is_empty(),
            "{harness:?}"
        );
        case.end();
    });
}

#[test]
fn t2233_a_dialog_answered_the_message_is_taken() {
    each_harness(|harness| {
        let case = run_script(harness, |world| {
            world.program.lock().unwrap().show_dialog();
            settled(&world);
            types(&world, b"2");
            settled(&world);
            a_while(&world);
            world.pass(false);
        });
        assert_eq!(
            case.taken(),
            [1],
            "{harness:?}: {:?}",
            case.world.log.lock().unwrap()
        );
        case.rules();
        case.end();
    });
}

// ---- Typed elsewhere, a screen for each key (T-2255) --------------------------------------------

/// Types `keys` one at a time into a picker, each drawn before the next.
fn filters(world: &World, keys: &[u8]) {
    for key in keys {
        types(world, &[*key]);
        settled(world);
    }
}

#[test]
fn t2255_a_picker_filtered_key_by_key_and_left_with_escape_the_message_is_taken() {
    each_harness(|harness| {
        let case = run_script(harness, |world| {
            world.program.lock().unwrap().show_picker();
            settled(&world);
            filters(&world, b"zqv");
            filters(&world, b"\x1b");
            a_while(&world);
            world.pass(false);
        });
        assert_eq!(
            case.taken(),
            [1],
            "{harness:?}: {:?}",
            case.world.log.lock().unwrap()
        );
        case.rules();
        case.end();
    });
}

#[test]
fn t2255_a_key_at_the_box_after_a_dialog_holds() {
    each_harness(|harness| {
        let case = run_script(harness, |world| {
            world.program.lock().unwrap().show_dialog();
            settled(&world);
            types(&world, b"2");
            settled(&world);
            // The `x` goes into the box and is not drawn.
            world.echo_as([Echo::Silent]);
            types(&world, b"x");
            a_while(&world);
            world.pass(false);
        });
        case.rules();
        assert!(
            case.held().is_empty() && case.taken().is_empty(),
            "{harness:?}"
        );
        case.end();
    });
}

/// `z` into a picker, then `after` typed while the picker's frame of the `z` waits at the
/// terminal; the case then runs `then`.
fn keys_with_the_pickers_frame_at_the_terminal(
    harness: Harness,
    after: &'static [u8],
    then: impl FnOnce(&World) + Send + 'static,
) -> Case {
    let case = Case::new(harness, move |world| {
        world.program.lock().unwrap().show_picker();
        settled(&world);
        sched::at("case.shown");
        types(&world, b"z");
        types(&world, after);
        sched::at("case.typed");
        settled(&world);
        then(&world);
        settled(&world);
        a_while(&world);
        world.pass(false);
    });
    case.until_script("case.shown");
    case.run(&[case.reader], |case| {
        matches!(case.sched.stands(case.script), Stands::At("case.typed"))
    });
    case.finish();
    case
}

#[test]
fn t2255_a_last_key_typed_with_the_pickers_frame_at_the_terminal_holds() {
    each_harness(|harness| {
        // The digit takes a choice and ends the picker; no key follows it.
        let case = keys_with_the_pickers_frame_at_the_terminal(harness, b"1", |_| {});
        case.rules();
        assert!(
            case.held().is_empty() && case.taken().is_empty(),
            "{harness:?}"
        );
        case.end();
    });
}

#[test]
fn t2255_the_screen_at_the_next_key_stands_for_a_last_one_that_was_behind() {
    each_harness(|harness| {
        let case = keys_with_the_pickers_frame_at_the_terminal(harness, b"v", |world| {
            types(world, b"\x1b");
        });
        assert_eq!(
            case.taken(),
            [1],
            "{harness:?}: {:?}",
            case.world.log.lock().unwrap()
        );
        case.rules();
        case.end();
    });
}

#[test]
fn t2255_more_keys_than_screens_are_kept_of_holds() {
    each_harness(|harness| {
        let case = run_script(harness, |world| {
            world.program.lock().unwrap().show_picker();
            settled(&world);
            filters(&world, &[b'z'; 33]);
            filters(&world, b"\x1b");
            a_while(&world);
            world.pass(false);
        });
        case.rules();
        assert!(
            case.held().is_empty() && case.taken().is_empty(),
            "{harness:?}"
        );
        case.end();
    });
}

/// A window that draws a dialog's end, and the key typed ahead of it a moment later: a look
/// between the two reads the box empty.
#[test]
fn t2255_a_dialogs_end_and_the_key_typed_ahead_drawn_in_two_frames_holds() {
    each_harness(|harness| {
        let case = run_script(harness, |world| {
            world.program.lock().unwrap().show_dialog();
            settled(&world);
            world.program.lock().unwrap().stall();
            types(&world, b"2");
            types(&world, b"x");
            a_while(&world);
            // The `2` is taken and the dialog's end drawn; the `x` follows.
            world.program.lock().unwrap().resume_taking(1);
            settled(&world);
            crate::arbiter::Clock::rest(&*world.clock, Duration::from_millis(20));
            world.pass(false);
            world.program.lock().unwrap().resume();
            settled(&world);
            a_while(&world);
            world.pass(false);
        });
        case.rules();
        assert!(
            case.held().is_empty() && case.taken().is_empty(),
            "{harness:?}"
        );
        case.end();
    });
}

#[test]
fn t2233_a_screen_behind_at_the_key_is_no_evidence() {
    each_harness(|harness| {
        let case = Case::new(harness, |world| {
            world.program.lock().unwrap().show_dialog();
            settled(&world);
            sched::at("case.shown");
            world.program.lock().unwrap().close_dialog();
            world.echo_as([Echo::Silent]);
            world.type_keys(b"X");
            sched::wait_until("the key is counted", || {
                world
                    .arbiter
                    .snapshot(&world.key)
                    .is_ok_and(|now| now.typed >= 1)
            });
            sched::at("case.typed");
            settled(&world);
            a_while(&world);
            world.pass(false);
        });
        case.until_script("case.shown");
        // The dialog's end waits at the terminal while the key is written.
        case.run(&[case.reader], |case| {
            matches!(case.sched.stands(case.script), Stands::At("case.typed"))
        });
        case.finish();
        case.rules();
        assert!(
            case.held().is_empty() && case.taken().is_empty(),
            "{harness:?}"
        );
        case.end();
    });
}

#[test]
fn t2233_a_paste_naming_evidence_the_host_lacks_is_refused() {
    each_harness(|harness| {
        // A character alone keeps no screen over a key that added none, and a character more
        // than the screens kept leaves "typed elsewhere" one without a screen.
        for by in [
            cf_proto::panes::Evidence::Erased,
            cf_proto::panes::Evidence::Elsewhere,
        ] {
            let answer = kept();
            let kept_answer = Arc::clone(&answer);
            let case = run_script(harness, move |world| {
                if by == cf_proto::panes::Evidence::Erased {
                    types(&world, b"X");
                } else {
                    world.program.lock().unwrap().show_picker();
                    settled(&world);
                    filters(&world, &[b'z'; 33]);
                }
                settled(&world);
                a_while(&world);
                let look = world.look().expect("a look");
                *kept_answer.lock().unwrap() = Some(world.paste_saying(&look, by));
            });
            let outcome = answer.lock().unwrap().take().expect("asked");
            assert!(
                matches!(outcome, Err(ArbiterError::Unsent)),
                "{harness:?} {by:?}: {outcome:?}"
            );
            assert!(case.held().is_empty(), "{harness:?} {by:?}");
            case.rules();
            case.end();
        }
    });
}

/// Our paste's echo waits at the terminal while the page holds its output back: no look, so
/// nothing is pasted again, and nothing is pressed.
#[test]
fn t2233_the_room_gives_no_look_and_nothing_goes_again() {
    each_harness(|harness| {
        let case = Case::new(harness, |world| {
            world.program.lock().unwrap().swallow = 1;
            world.pass(false);
            sched::at("case.pasted");
            crate::arbiter::Clock::rest(&*world.clock, Duration::from_secs(70));
            for _ in 0..3 {
                world.pass(false);
                crate::arbiter::Clock::rest(&*world.clock, Duration::from_secs(70));
            }
        });
        // The reader is held at the page's room: what the window prints stays at its terminal.
        case.run(&[], |case| {
            matches!(case.sched.stands(case.script), Stands::At("case.pasted"))
        });
        case.drawn();
        case.world.program.lock().unwrap().repaint();
        case.run(&[case.reader], |case| !case.sched.alive(case.script));
        assert_eq!(
            case.held(),
            [1],
            "{harness:?}: pasted again over a look that was none"
        );
        case.rules();
        case.end();
    });
}

#[test]
fn t2233_printed_since_the_look_pane_enter_waits_and_is_refused() {
    each_harness(|harness| {
        let answer = kept();
        let kept_answer = Arc::clone(&answer);
        let case = Case::new(harness, move |world| {
            world.program.lock().unwrap().swallow = 1;
            world.pass(false);
            crate::arbiter::Clock::rest(&*world.clock, Duration::from_secs(4));
            let look = world.look().expect("a look");
            sched::at("case.looked");
            *kept_answer.lock().unwrap() = world.enter_on(&look, EnterFor::PasteStillThere);
        });
        case.until_script("case.looked");
        case.drawn();
        case.world.program.lock().unwrap().repaint();
        case.run(&[case.reader], |case| !case.sched.alive(case.script));
        let outcome = answer.lock().unwrap().take().expect("asked");
        assert!(
            matches!(outcome, Err(ArbiterError::LookAgain(Why::OutputPending))),
            "{harness:?}: {outcome:?}"
        );
        assert_eq!(case.held(), [1], "{harness:?}: no Enter was written");
        case.rules();
        case.end();
    });
}

/// A paste's own Enter waits while the terminal holds bytes, and goes at the cap and 3 s.
#[test]
fn t2233_the_first_enter_waits_for_the_terminal_and_goes_at_the_cap() {
    each_harness(|harness| {
        let answer = kept();
        let case = Case::new(harness, paste_after_a_look(&answer));
        case.until_script("case.looked");
        case.run(&[], |case| !case.held().is_empty());
        let pasted = case.world.clock.elapsed();
        // The echo of the paste and a repaint wait at the terminal, never read.
        case.world.program.lock().unwrap().repaint();
        case.run(&[case.reader], |case| !case.sched.alive(case.script));
        let waited = case.world.clock.elapsed() - pasted;
        assert_eq!(case.taken(), [1], "{harness:?}");
        assert!(
            waited >= Duration::from_millis(super::world::ENTER_TIMING.most_ms + 3_000),
            "{harness:?}: the Enter went after {waited:?}, before the cap and its grace"
        );
        case.rules();
        case.end();
    });
}

// ---- The pinned limits (the design, "What the checks assume of a window") ---------------------

/// A window that stalls twice between two of the person's keys: `ab`, Ctrl+W, `X`, an arrow.
/// At the arrow the screen still shows `ab`; then it draws the erase and stalls again before
/// the `X`. No evidence closes this: the paste goes over the `X`. The test pins the limit.
#[test]
fn the_limit_an_erase_drawn_late_and_a_character_after_it_stalled_again() {
    each_harness(|harness| {
        let case = run_script(harness, |world| {
            types(&world, b"a");
            types(&world, b"b");
            settled(&world);
            world.program.lock().unwrap().stall();
            types(&world, b"\x17");
            types(&world, b"X");
            types(&world, b"\x1b[D");
            // The erase is taken and drawn; the `X` and the arrow wait again.
            world.program.lock().unwrap().resume_taking(1);
            settled(&world);
            a_while(&world);
            world.pass(false);
            world.program.lock().unwrap().resume();
        });
        assert!(
            case.world.check().is_err(),
            "{harness:?}: the limit is closed: take it out of the design and the limit list"
        );
        case.end();
    });
}

/// A dialog answered with Enter and one character typed before the window drew the dialog's
/// end, that one character then undrawn: the paste goes over it. The test pins the limit.
#[test]
fn the_limit_a_character_typed_before_the_dialogs_end_was_drawn() {
    each_harness(|harness| {
        let case = run_script(harness, |world| {
            world.program.lock().unwrap().show_dialog();
            settled(&world);
            world.program.lock().unwrap().stall();
            types(&world, b"\r");
            types(&world, b"x");
            // The Enter is taken: the dialog ends. The `x` waits.
            world.program.lock().unwrap().resume_taking(1);
            settled(&world);
            a_while(&world);
            world.pass(false);
            world.program.lock().unwrap().resume();
        });
        assert!(
            case.world.check().is_err(),
            "{harness:?}: the limit is closed: take it out of the design and the limit list"
        );
        case.end();
    });
}

/// A dialog answered with a digit and one character typed while the window is stalled on it,
/// the character then never drawn: the paste goes over it (T-2255: a picker that ignores keys
/// shows the same screens). The test pins the limit.
#[test]
fn the_limit_two_keys_typed_into_a_stalled_dialog() {
    each_harness(|harness| {
        let case = run_script(harness, |world| {
            world.program.lock().unwrap().show_dialog();
            settled(&world);
            world.program.lock().unwrap().stall();
            types(&world, b"2");
            // The `x` is not drawn.
            world.echo_as([Echo::Silent]);
            types(&world, b"x");
            world.program.lock().unwrap().resume();
            settled(&world);
            a_while(&world);
            world.pass(false);
        });
        assert!(
            case.world.check().is_err(),
            "{harness:?}: the limit is closed: take it out of the design and the limit list"
        );
        case.end();
    });
}
