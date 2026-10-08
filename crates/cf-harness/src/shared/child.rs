//! Ending a child process as OpenCode's throwaway server is ended: asked to
//! end, forced once it has had long enough, and given up on, in a sentence of
//! its own, when it will not close at all.

use crate::seams::processes::{Child, Ending};
use crate::seams::{arm, Time};

/// How long a child asked to end has to close before it is forced to.
const FORCE_AFTER_MS: u64 = 2000;

/// How long past that it has to close, forced, before it is given up on.
const SETTLE_MS: u64 = 2000;

/// Asks `child` to end unless it has exited already, and waits until it has
/// closed: forced at two seconds if it has not closed by then, and failed at
/// four, both counted from the ask. Each call goes through all of it again,
/// so a child that failed to stop is asked, forced and waited for once more.
pub(crate) async fn stop(time: &dyn Time, child: &dyn Child) -> Result<(), String> {
    if !child.exited() {
        child.terminate(Ending::Asked);
    }
    let forcing = arm(time, FORCE_AFTER_MS);
    let giving_up = arm(time, FORCE_AFTER_MS + SETTLE_MS);
    if forcing.bound(child.closed()).await.is_some() {
        return Ok(());
    }
    child.terminate(Ending::Forced);
    giving_up
        .bound(child.closed())
        .await
        .ok_or_else(|| "opencode session failed to stop the server".to_owned())
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::task::Poll;

    use cf_base::env::Env;

    use super::*;
    use crate::contract::Work;
    use crate::seams::processes::{Processes, Program, Streams};
    use crate::testing::{ChildScript, Driver, Ends, ManualTime, ScriptedProcesses};

    /// A scripted child that ends as `ends` says.
    fn scripted(ends: Ends) -> Rc<dyn Child> {
        let processes = ScriptedProcesses::default();
        processes.child(
            "opencode",
            ChildScript {
                lines: Vec::new(),
                ends,
            },
        );
        let program = Program {
            executable: PathBuf::from("opencode"),
            args: Vec::new(),
            cwd: None,
            env: Env::default(),
        };
        Rc::from(processes.spawn(program, Streams::Quiet).unwrap())
    }

    /// A child that writes down how it is ended, and closes once forced.
    #[derive(Default)]
    struct Recording {
        asked: RefCell<Vec<Ending>>,
        exited: Cell<bool>,
        closed: Cell<bool>,
    }

    impl Child for Recording {
        fn write_line<'a>(&'a self, _line: &'a str) -> Work<'a, Result<(), String>> {
            Box::pin(async { Ok(()) })
        }

        fn read_line(&self, _limit: usize) -> Work<'_, Result<Option<String>, String>> {
            Box::pin(async { Ok(None) })
        }

        fn exited(&self) -> bool {
            self.exited.get()
        }

        fn closed(&self) -> Work<'_, ()> {
            Box::pin(std::future::poll_fn(|_| {
                if self.closed.get() {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }))
        }

        fn terminate(&self, how: Ending) {
            self.asked.borrow_mut().push(how);
            if how == Ending::Forced {
                self.closed.set(true);
            }
        }
    }

    /// A stop of `child` begun as work `id` on a clock that began at 0.
    fn begin(
        driver: &mut Driver<Result<(), String>>,
        time: &Rc<ManualTime>,
        child: &Rc<dyn Child>,
        id: usize,
    ) {
        let (clock, stopped) = (Rc::clone(time), Rc::clone(child));
        driver.begin(id, async move { stop(&*clock, &*stopped).await });
    }

    #[test]
    fn a_child_that_ends_when_asked_is_stopped_at_once_and_its_timers_forgotten() {
        let child = scripted(Ends::Asked);
        let time = Rc::new(ManualTime::new(0));
        let mut driver = Driver::default();
        begin(&mut driver, &time, &child, 0);
        assert_eq!(driver.run(), [(0, Ok(()))]);
        assert!(child.exited());
        assert!(time.waits(0).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_goes_on_when_asked_is_forced_at_two_seconds() {
        let child = scripted(Ends::Forced);
        let time = Rc::new(ManualTime::new(0));
        let mut driver = Driver::default();
        begin(&mut driver, &time, &child, 0);
        assert!(driver.run().is_empty());
        assert_eq!(
            time.waits(0),
            [2000, 4000],
            "forcing first, giving up second"
        );
        assert!(!child.exited(), "asked, it goes on");
        assert!(time.fire_next(2000));
        assert_eq!(driver.run(), [(0, Ok(()))]);
        assert!(child.exited());
        assert!(time.waits(0).is_empty(), "both timers go with the stop");
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_never_ends_is_given_up_on_at_four_seconds_in_a_sentence_of_its_own() {
        let child = scripted(Ends::Never);
        let time = Rc::new(ManualTime::new(0));
        let mut driver = Driver::default();
        begin(&mut driver, &time, &child, 0);
        assert!(driver.run().is_empty());
        assert!(time.fire_next(2000));
        assert!(driver.run().is_empty(), "forced, and still there");
        assert_eq!(time.waits(0), [2000], "only the giving up is left");
        assert!(!time.fire_next(3999));
        assert!(time.fire_next(4000));
        assert_eq!(
            driver.run(),
            [(
                0,
                Err("opencode session failed to stop the server".to_owned())
            )]
        );
    }

    #[test]
    fn a_child_that_has_exited_is_not_asked_to_end_but_is_waited_for_until_it_closes() {
        let recording = Rc::new(Recording::default());
        recording.exited.set(true);
        let child: Rc<dyn Child> = Rc::clone(&recording) as Rc<dyn Child>;
        let time = Rc::new(ManualTime::new(0));
        let mut driver = Driver::default();
        begin(&mut driver, &time, &child, 0);
        assert!(driver.run().is_empty());
        assert_eq!(*recording.asked.borrow(), []);
        assert!(time.fire_next(2000));
        assert_eq!(driver.run(), [(0, Ok(()))]);
        assert_eq!(*recording.asked.borrow(), [Ending::Forced]);
    }

    #[test]
    fn a_child_that_closes_before_its_time_is_not_forced() {
        let recording = Rc::new(Recording::default());
        let child: Rc<dyn Child> = Rc::clone(&recording) as Rc<dyn Child>;
        let time = Rc::new(ManualTime::new(0));
        let mut driver = Driver::default();
        begin(&mut driver, &time, &child, 0);
        assert!(driver.run().is_empty());
        recording.closed.set(true);
        assert_eq!(driver.run(), [(0, Ok(()))]);
        assert_eq!(*recording.asked.borrow(), [Ending::Asked]);
        assert!(!time.fire_next(10_000), "nothing is left to fire");
    }

    #[test]
    fn a_second_stop_asks_forces_and_waits_all_over_again() {
        let recording = Rc::new(Recording::default());
        let child: Rc<dyn Child> = Rc::clone(&recording) as Rc<dyn Child>;
        let time = Rc::new(ManualTime::new(0));
        let mut driver = Driver::default();
        begin(&mut driver, &time, &child, 0);
        assert!(driver.run().is_empty());
        assert!(time.fire_next(2000));
        assert_eq!(driver.run(), [(0, Ok(()))]);
        recording.closed.set(false);
        begin(&mut driver, &time, &child, 1);
        assert!(driver.run().is_empty());
        assert!(time.fire_next(4000));
        assert_eq!(driver.run(), [(1, Ok(()))]);
        assert_eq!(
            *recording.asked.borrow(),
            [Ending::Asked, Ending::Forced, Ending::Asked, Ending::Forced]
        );
    }
}
