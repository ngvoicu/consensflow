//! What a test holds of a fake's calls: `hold` in `core-dispatcher.test.mjs`
//! wraps a function of a fake, and each call it picks waits until the test
//! lets it go, then is made, or something else is done in its place
//! (`instead`). The wrapper is an `async` function that returns the fake's own
//! promise, so every call of the method, picked or not, answers two turns
//! after the fake's own would have, and a call picked is made a turn after the
//! test lets it go.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::Value;

use super::executor::Gate;
use crate::runtime::next_turn;

/// One hold: the calls it picks, what lets them go, and what they do then.
struct Held<I> {
    which: Rc<dyn Fn(&Value) -> bool>,
    gate: Gate,
    instead: Option<I>,
}

/// The holds a test put on one method of a fake, in order. `I` is what a call
/// held does once let go in place of what the fake's own would have done, for
/// the methods where a test did something else.
pub struct Holds<I = ()>(RefCell<Vec<Held<I>>>);

impl<I> Default for Holds<I> {
    fn default() -> Self {
        Self(RefCell::new(Vec::new()))
    }
}

impl<I: Clone> Holds<I> {
    /// Holds each call that `which` picks, told the call's arguments as the
    /// Node traces write them, until the gate this answers opens: each is then
    /// made as ever.
    pub fn hold(&self, which: impl Fn(&Value) -> bool + 'static) -> Gate {
        self.add(which, None)
    }

    /// Holds as [`Holds::hold`] does, and each call let go does `instead` in
    /// place of what it would have done.
    pub fn hold_instead(&self, which: impl Fn(&Value) -> bool + 'static, instead: I) -> Gate {
        self.add(which, Some(instead))
    }

    fn add(&self, which: impl Fn(&Value) -> bool + 'static, instead: Option<I>) -> Gate {
        let gate = Gate::default();
        self.0.borrow_mut().push(Held {
            which: Rc::new(which),
            gate: gate.clone(),
            instead,
        });
        gate
    }

    /// What the wrapper makes of a call just made with `asked`.
    pub(super) fn wrap(&self, asked: &Value) -> Wrapped<I> {
        let holds = self.0.borrow();
        if holds.is_empty() {
            return Wrapped::Bare;
        }
        match holds.iter().find(|held| (held.which)(asked)) {
            Some(held) => Wrapped::Held {
                gate: held.gate.clone(),
                instead: held.instead.clone(),
            },
            None => Wrapped::Passed,
        }
    }
}

/// A call as the wrapper of its method handles it.
pub(super) enum Wrapped<I> {
    /// No hold on the method: the fake's own call.
    Bare,
    /// A hold on the method that picks other calls.
    Passed,
    /// A call a hold picks.
    Held { gate: Gate, instead: Option<I> },
}

impl<I: Clone> Wrapped<I> {
    /// A call held waits for the test to let it go, and goes on the turn after
    /// it does, as the `await` of the test's promise did (a turn after the
    /// call where it was let go already): what it does in place of the
    /// fake's own, if anything.
    pub(super) async fn before(&self) -> Option<I> {
        let Wrapped::Held { gate, instead } = self else {
            return None;
        };
        let open = gate.is_open();
        gate.wait().await;
        if open {
            next_turn().await;
        }
        instead.clone()
    }

    /// The wrapper returns the fake's promise from an `async` function: its
    /// own settles two turns after the fake's, for every call of the method,
    /// picked or not.
    pub(super) async fn after(&self) {
        if !matches!(self, Wrapped::Bare) {
            next_turn().await;
            next_turn().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use serde_json::json;

    use super::*;
    use crate::runtime::{Executor, Spawn};

    /// What a test's pieces of work heard, in order.
    type Log = Rc<RefCell<Vec<String>>>;

    /// A tick of work of its own at each of the next `turns` turns, to count
    /// them by.
    fn ticking(executor: &Executor, log: &Log, turns: usize) {
        let ticks = Rc::clone(log);
        executor.spawn(Box::pin(async move {
            for turn in 0..turns {
                ticks.borrow_mut().push(turn.to_string());
                next_turn().await;
            }
        }));
    }

    #[test]
    fn a_hold_picks_the_calls_its_function_says_and_a_method_with_none_is_bare() {
        let holds: Holds<String> = Holds::default();
        assert!(matches!(holds.wrap(&json!([{ "to": "a" }])), Wrapped::Bare));
        holds.hold_instead(|args| args[0]["to"] == "a", "failed".to_owned());
        assert!(matches!(
            holds.wrap(&json!([{ "to": "a" }])),
            Wrapped::Held { instead: Some(reason), .. } if reason == "failed"
        ));
        assert!(matches!(
            holds.wrap(&json!([{ "to": "b" }])),
            Wrapped::Passed
        ));
    }

    #[test]
    fn a_call_held_goes_on_the_turn_after_the_gate_opens_and_one_let_go_already_waits_a_turn() {
        let executor = Executor::strict();
        let holds = Rc::new(Holds::<()>::default());
        let gate = holds.hold(|_| true);
        let log = Log::default();
        let (asking, heard) = (Rc::clone(&holds), Rc::clone(&log));
        executor.spawn(Box::pin(async move {
            asking.wrap(&json!([])).before().await;
            heard.borrow_mut().push("went on".to_owned());
        }));
        executor.drain();
        assert!(log.borrow().is_empty(), "it waits for the test");
        gate.open();
        ticking(&executor, &log, 3);
        executor.drain();
        assert_eq!(*log.borrow(), ["went on", "0", "1", "2"]);

        // The gate is open already: the `await` still takes its turn.
        log.borrow_mut().clear();
        let (asking, heard) = (Rc::clone(&holds), Rc::clone(&log));
        executor.spawn(Box::pin(async move {
            asking.wrap(&json!([])).before().await;
            heard.borrow_mut().push("went on".to_owned());
        }));
        ticking(&executor, &log, 3);
        executor.drain();
        assert_eq!(*log.borrow(), ["0", "went on", "1", "2"]);
    }

    #[test]
    fn the_wrapper_answers_two_turns_after_the_fakes_own_whichever_call_it_picked() {
        let executor = Executor::strict();
        let (bare, wrapped) = (Holds::<()>::default(), Holds::<()>::default());
        wrapped.hold(|args| args[0] == "held");
        let log = Log::default();
        for (holds, asked, name) in [
            (&bare, "any", "bare"),
            (&wrapped, "other", "passed"),
            (&wrapped, "held", "held"),
        ] {
            let (call, heard) = (holds.wrap(&json!([asked])), Rc::clone(&log));
            executor.spawn(Box::pin(async move {
                call.after().await;
                heard.borrow_mut().push(name.to_owned());
            }));
        }
        ticking(&executor, &log, 3);
        executor.drain();
        assert_eq!(
            *log.borrow(),
            ["bare", "0", "1", "passed", "held", "2"],
            "no turn for the fake's own, two for the wrapper's"
        );
    }

    #[test]
    fn what_a_call_held_does_in_place_is_handed_back_once_it_is_let_go() {
        let executor = Executor::strict();
        let holds = Rc::new(Holds::<String>::default());
        let gate = holds.hold_instead(|_| true, "native-named".to_owned());
        let named = Rc::new(RefCell::new(None));
        let (asking, kept) = (Rc::clone(&holds), Rc::clone(&named));
        executor.spawn(Box::pin(async move {
            *kept.borrow_mut() = Some(asking.wrap(&json!([])).before().await);
        }));
        executor.drain();
        assert_eq!(*named.borrow(), None, "held");
        gate.open();
        executor.drain();
        assert_eq!(*named.borrow(), Some(Some("native-named".to_owned())));
    }
}
