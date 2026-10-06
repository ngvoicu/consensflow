//! The engine's own chains, hung on timers and on the records' answers: two
//! workers' looks, one of which finds its answer (its task is collected and
//! its window killed) and the other confirms a delivery. The first chain is
//! longer, so that the two run together, a turn of each, show in the order of
//! what they do: the events come in another order than when each runs whole.
//! Node ran each look's continuations as a callback of its own, the one
//! worker's before the other's, and the kit's run of the same callbacks says
//! what that left ([`super::reference`]); the tests give the daemon the two
//! looks' timers elapsing in one moment, and the two looks' answers sent
//! together by the records' worker, and hold it to that.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use cf_engine::runtime::begin;
use cf_engine::seams::Adapters;
use cf_harness::contract::Work;
use cf_harness::seams::Time;

use super::looking::{looking, Looks};
use super::reference::{looks_apart, looks_together};
use super::rig::{Rig, Wiring};
use super::standing::{look, Standing};
use super::{scene, settle};
use crate::seams::{DaemonRecords, DaemonTime};

/// A rig whose engine sleeps on the daemon's time and whose adapters look at
/// their windows once `looks` says they may, the engine's time handed to
/// `looks`' own wait through the cell.
fn timed(looks: &Rc<Looks>, clock: &Rc<RefCell<Option<Rc<dyn Time>>>>) -> Wiring {
    let (looks, clock) = (Rc::clone(looks), Rc::clone(clock));
    Wiring::new()
        .time(|spawn| Rc::new(DaemonTime::new(Rc::clone(spawn))))
        .adapters(move |_, time, fakes: Rc<dyn Adapters>| {
            *clock.borrow_mut() = Some(Rc::clone(time));
            looking(fakes, looks)
        })
}

/// Zeus's task is working and his window has the answer to it; hera's window
/// is open and her message waits to be confirmed: the setup of two looks,
/// which are not yet asked.
async fn two_workers_with_a_look_each(rig: &mut Rig) -> i64 {
    let project = rig.open_project(&["zeus", "hera"]).await;
    rig.give(project.id, "zeus", "Parser");
    rig.passes(2).await;
    rig.give(project.id, "hera", "Lexer");
    rig.passes(1).await;
    assert_eq!(rig.task_state(project.id, 1), "working");
    assert_eq!(rig.task_state(project.id, 2), "queued");
    project.id
}

/// What the engine's two looks end in, in the order the kit ran them apart.
fn assert_ran_apart(rig: &Rig, project: i64) {
    assert_eq!(
        rig.task_state(project, 1),
        "done",
        "zeus's task was collected"
    );
    assert_eq!(
        rig.task_state(project, 2),
        "working",
        "hera's delivery was confirmed"
    );
    assert_ne!(looks_apart(), looks_together(), "the two orders differ");
    assert_eq!(rig.events(), looks_apart());
}

#[tokio::test(start_paused = true)]
async fn two_looks_whose_timers_elapse_together_are_two_callbacks_in_the_order_the_kit_ran_them() {
    scene(async {
        let clock: Rc<RefCell<Option<Rc<dyn Time>>>> = Rc::default();
        let waiting = Rc::clone(&clock);
        // Zeus's look waits for a timer a millisecond before hera's.
        let looks = Looks::new(move |handle| {
            let time = waiting.borrow().clone().expect("the engine's time");
            let wait = Duration::from_millis(match handle {
                "zeus" => 10,
                _ => 11,
            });
            let sleeping: Work<'static, ()> = match handle {
                "chief" => Box::pin(async {}),
                _ => Box::pin(async move { time.sleep(wait).await }),
            };
            sleeping
        });
        let mut rig = Rig::wired(timed(&looks, &clock)).await;
        let project = two_workers_with_a_look_each(&mut rig).await;
        looks.arm();
        rig.adapter.answer("zeus", "Parser done");
        rig.mark();
        let passing = rig.pass().await;
        settle().await;
        // Both timers elapse in the same moment, tokio wakes both, and the
        // daemon runs each as a callback.
        tokio::time::advance(Duration::from_millis(20)).await;
        rig.serve_until(|| passing.ended()).await;
        rig.quiet().await;
        assert_ran_apart(&rig, project);
    })
    .await;
}

/// A rig whose adapters' looks wait for the daemon's records, whose worker is
/// the stand-in the test lets answer, once the looks are armed.
async fn records_looked_at() -> (Rig, Rc<Standing>, Rc<Looks>) {
    let standing = Rc::new(Standing::default());
    let records: Rc<RefCell<Option<Rc<DaemonRecords>>>> = Rc::default();
    let asking = Rc::clone(&records);
    let looks = Looks::new(move |handle| {
        let records = asking.borrow().clone().expect("the records");
        let waiting: Work<'static, ()> = match handle {
            "chief" => Box::pin(async {}),
            _ => Box::pin(async move { look(&records).await }),
        };
        waiting
    });
    let (worker, wired) = (Rc::clone(&standing), Rc::clone(&looks));
    let wiring = Wiring::new().adapters(move |spawn, _, fakes: Rc<dyn Adapters>| {
        *records.borrow_mut() = Some(worker.over(spawn));
        looking(fakes, wired)
    });
    (Rig::wired(wiring).await, standing, looks)
}

#[tokio::test]
async fn two_looks_the_records_answer_together_are_two_callbacks_in_the_order_the_kit_ran_them() {
    scene(async {
        let (mut rig, standing, looks) = records_looked_at().await;
        let project = two_workers_with_a_look_each(&mut rig).await;
        looks.arm();
        rig.adapter.answer("zeus", "Parser done");
        rig.mark();
        let passing = rig.pass().await;
        settle().await;
        // Zeus's look was asked before hera's, and the worker answers both
        // before the engine's thread looks at either.
        standing.answer(2);
        rig.serve_until(|| passing.ended()).await;
        rig.quiet().await;
        assert_ran_apart(&rig, project);
    })
    .await;
}

#[tokio::test]
async fn two_looks_answered_before_the_executors_first_poll_of_the_pass_are_two_callbacks_too() {
    scene(async {
        let (mut rig, standing, looks) = records_looked_at().await;
        let project = two_workers_with_a_look_each(&mut rig).await;
        looks.arm();
        rig.adapter.answer("zeus", "Parser done");
        rig.mark();
        // The pass is begun where the loop begins it: both looks are asked in
        // its first part, and the executor has not polled it since. The
        // worker's answers are there when it does, and each look's
        // continuation is the callback of its relay all the same, as the kit
        // ran them.
        let dispatcher = Rc::clone(&rig.dispatcher);
        let passing = begin(&*rig.pieces.spawn, async move {
            dispatcher.pass().await.map_err(|failed| failed.to_string())
        })
        .await;
        standing.answer(2);
        rig.pieces.spawn.drain();
        rig.serve_until(|| passing.ended()).await;
        rig.quiet().await;
        assert_ran_apart(&rig, project);
    })
    .await;
}
