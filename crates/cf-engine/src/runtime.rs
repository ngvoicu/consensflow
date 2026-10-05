//! How the engine's work runs, as Node's ran on its one thread
//! (`#exclusive`, `#act` and `begin`, `src/core/dispatcher.js`):
//!
//! - A piece of work is begun where JavaScript called it ([`begin`]): what it
//!   does before its first wait is done in place, so a launch takes its
//!   message, draws its id and counts its generation inside the pass that
//!   started it, before the next participant's step.
//! - What is left of it goes on as work of its own ([`Spawn`]), which nobody
//!   needs to wait for, and which no caller going away stops.
//! - Each participant is held by one piece of work at a time ([`Hold`]): a
//!   step or a human's operation, and a launch or a delivery going on apart
//!   from the pass. A pass that finds it held moves on; a human's operation
//!   waits its turn, first come, first served, and the participant is handed
//!   straight to it, so nothing that comes later takes it first.
//! - Work begun together is awaited together ([`all`]), its first failure
//!   answered as soon as it comes.
//! - What an `async` function answers reaches its caller a turn after it was
//!   made, in JavaScript, even when it waited on nothing. Where that order
//!   decides what one participant's step sees of another's, the call is made
//!   [`returning`] a turn too: a pass's steps cross-read the ledger (a note
//!   one step writes is the chief's next delivery in the same pass), so a
//!   step that took fewer turns than Node's would read it before it was there.
//! - A piece of work that panics is what one that threw was: its participant
//!   is let go of as it unwinds (`finally`, `#exclusive` and `#act`), and
//!   whoever waits for its answer ends with an error, not for ever. The panic
//!   itself goes on unwinding where the work runs, for whoever runs it to
//!   write down.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::{poll_fn, Future};
use std::panic::{resume_unwind, AssertUnwindSafe};
use std::pin::{pin, Pin};
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use futures_util::stream::{FuturesUnordered, StreamExt};
use futures_util::FutureExt;

/// A piece of work on the engine's thread.
pub type LocalWork = Pin<Box<dyn Future<Output = ()>>>;

/// Where work that goes on apart from its caller runs to its end.
pub trait Spawn {
    fn spawn(&self, work: LocalWork);
}

/// Tokio's local tasks, on the daemon's `LocalSet`. A task that panics is
/// told by the panic hook, as any panic is.
pub struct LocalSpawn;

impl Spawn for LocalSpawn {
    fn spawn(&self, work: LocalWork) {
        drop(tokio::task::spawn_local(work));
    }
}

/// Begins `work` where JavaScript called it (`begin`): polled once here, so
/// what it does before its first wait is done now; what is left, if
/// anything, goes on as work of its own. Its answer comes through the
/// [`Begun`] returned, which may be dropped: the work goes on all the same.
///
/// A panic in `work` before its first wait unwinds out of this call, to the
/// caller, as a throw did; one after it unwinds the work of its own, which is
/// told to whoever spawned it, and the [`Begun`] ends its waiter with a panic
/// of its own.
pub async fn begin<T: 'static>(
    spawn: &dyn Spawn,
    work: impl Future<Output = T> + 'static,
) -> Begun<T> {
    let answer = Rc::new(Answer::default());
    let mut whole: LocalWork = Box::pin({
        let answer = Rc::clone(&answer);
        async move {
            match AssertUnwindSafe(work).catch_unwind().await {
                Ok(value) => answer.set(value),
                Err(panic) => {
                    // Whoever waits for the answer must not wait for ever.
                    answer.fail(&panic_words(panic.as_ref()));
                    resume_unwind(panic);
                }
            }
        }
    });
    // Polled with the caller's waker: whatever it waits on now is waited on
    // again, with the work's own waker, at the work's first poll there.
    let waits = poll_fn(|cx| Poll::Ready(whole.as_mut().poll(cx).is_pending())).await;
    if waits {
        spawn.spawn(whole);
    }
    Begun { answer }
}

/// The answer of a piece of work begun, once it has one.
pub struct Begun<T> {
    answer: Rc<Answer<T>>,
}

impl<T> Begun<T> {
    /// Whether the work has ended.
    pub fn ended(&self) -> bool {
        self.answer.ended.get()
    }
}

impl<T> Future for Begun<T> {
    type Output = T;

    /// The work's answer; a panic of this waiter's own where the work panicked,
    /// which says what the work's did.
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let value = self.answer.value.borrow_mut().take();
        match value {
            Some(value) => Poll::Ready(value),
            None => {
                if let Some(words) = self.answer.failed.borrow().as_deref() {
                    panic!("the work waited for panicked: {words}");
                }
                *self.answer.waker.borrow_mut() = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Where a piece of work's answer waits for whoever wants it.
struct Answer<T> {
    value: RefCell<Option<T>>,
    ended: Cell<bool>,
    /// What the work's panic said, once it panicked.
    failed: RefCell<Option<String>>,
    waker: RefCell<Option<Waker>>,
}

impl<T> Default for Answer<T> {
    fn default() -> Self {
        Self {
            value: RefCell::new(None),
            ended: Cell::new(false),
            failed: RefCell::new(None),
            waker: RefCell::new(None),
        }
    }
}

impl<T> Answer<T> {
    fn set(&self, value: T) {
        *self.value.borrow_mut() = Some(value);
        self.ended.set(true);
        self.wake();
    }

    /// The work panicked: its waiter is woken, to end with an error.
    fn fail(&self, words: &str) {
        *self.failed.borrow_mut() = Some(words.to_owned());
        self.wake();
    }

    fn wake(&self) {
        let waker = self.waker.borrow_mut().take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

/// What a panic said: its words when it had them.
fn panic_words(panic: &(dyn Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|words| (*words).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no words".to_owned())
}

/// The answers of work begun, in the order begun, as `Promise.all` gives
/// them: the first failure as soon as it comes, whatever earlier work still
/// waits on, the rest going on as the work of its own each is.
pub async fn all<T, E>(begun: Vec<Begun<Result<T, E>>>) -> Result<Vec<T>, E> {
    let mut answers: Vec<Option<T>> = std::iter::repeat_with(|| None).take(begun.len()).collect();
    let mut pending: FuturesUnordered<_> = begun
        .into_iter()
        .enumerate()
        .map(|(at, answer)| async move { (at, answer.await) })
        .collect();
    while let Some((at, answer)) = pending.next().await {
        answers[at] = Some(answer?);
    }
    Ok(answers.into_iter().flatten().collect())
}

/// Runs `work`, then waits a turn: what an `async` function answers reaches
/// its caller in JavaScript a turn after it was made, even when the function
/// waited on nothing, which `await` of a call that returned at once still
/// costs. What was woken before goes ahead of the caller.
pub(crate) async fn returning<T>(work: impl Future<Output = T>) -> T {
    let answer = work.await;
    next_turn().await;
    answer
}

/// A wait of one turn: the work goes behind every piece of work woken before
/// it, as an `await` of a JavaScript promise already kept did, and before
/// whatever the system has still to say (a frame to read), as a microtask
/// did.
pub fn next_turn() -> NextTurn {
    NextTurn { waited: false }
}

/// A wait of one turn ([`next_turn`]).
pub struct NextTurn {
    waited: bool,
}

impl Future for NextTurn {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        if self.waited {
            Poll::Ready(())
        } else {
            self.waited = true;
            context.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

/// Who holds a participant now (`runtime.running` and `runtime.acting`), and
/// who waits for it.
#[derive(Default)]
pub struct Hold {
    /// A step or a human's operation.
    running: Cell<bool>,
    /// A launch or a delivery going on apart from the pass.
    acting: Cell<bool>,
    /// Handed to the first waiter, which has not run yet: no pass takes the
    /// participant meanwhile.
    handed: Cell<bool>,
    waiting: RefCell<VecDeque<Rc<Turn>>>,
}

impl Hold {
    /// Whether some work holds the participant now.
    pub fn held(&self) -> bool {
        self.running.get() || self.acting.get() || self.handed.get()
    }

    /// Runs `work` for this participant if nothing holds it now
    /// (`#exclusive` from a pass): none when it is held, as a pass that
    /// finds it busy moves on. The work is begun in place, and holds the
    /// participant until it ends.
    pub async fn try_exclusive<T: 'static>(
        self: &Rc<Self>,
        spawn: &dyn Spawn,
        work: impl Future<Output = T> + 'static,
    ) -> Option<Begun<T>> {
        if self.held() {
            return None;
        }
        Some(self.hold_for(spawn, work).await)
    }

    /// Runs `work` for this participant once its turn comes (`#exclusive`
    /// with `wait`): at once if nothing holds it, begun in place; else its
    /// place in the queue is taken now, first come, first served, and the
    /// work begins when the participant is handed to it. Its answer comes
    /// through the [`Begun`] either way, which the caller may await with
    /// others: every waiter it makes is in the queue before it awaits any.
    pub async fn exclusive<T: 'static>(
        self: &Rc<Self>,
        spawn: &dyn Spawn,
        work: impl Future<Output = T> + 'static,
    ) -> Begun<T> {
        if !self.held() {
            return self.hold_for(spawn, work).await;
        }
        let waiting = Waiting::new(self);
        let hold = Rc::clone(self);
        begin(spawn, async move {
            waiting.await;
            hold.handed.set(false);
            hold.run_held(work).await
        })
        .await
    }

    /// Begins `work` in place, holding the participant from its first wait
    /// until it ends, however it ends.
    async fn hold_for<T: 'static>(
        self: &Rc<Self>,
        spawn: &dyn Spawn,
        work: impl Future<Output = T> + 'static,
    ) -> Begun<T> {
        let hold = Rc::clone(self);
        let begun = begin(spawn, async move {
            let _release = Release::running(&hold);
            work.await
        })
        .await;
        // Held from here, as JavaScript held it once the work's start was
        // done; one that ended there holds nothing.
        if !begun.ended() {
            self.running.set(true);
        }
        begun
    }

    /// Runs `work`, handed the participant, in the waiter's own work: its
    /// start as JavaScript ran it, nothing held until it first waits; then
    /// held until it ends, and the participant passed on. Each wait of the
    /// work is the waiter's, so what was begun meanwhile goes first.
    async fn run_held<T>(self: &Rc<Self>, work: impl Future<Output = T>) -> T {
        let _release = Release::running(self);
        let mut work = pin!(work);
        poll_fn(|context| {
            let polled = work.as_mut().poll(context);
            if polled.is_pending() {
                self.running.set(true);
            }
            polled
        })
        .await
    }

    /// Begins `work` apart from the pass, holding the participant until it
    /// ends (`#act`): a launch or a delivery a step started.
    pub async fn act(self: &Rc<Self>, spawn: &dyn Spawn, work: impl Future<Output = ()> + 'static) {
        let hold = Rc::clone(self);
        let begun = begin(spawn, async move {
            let _release = Release::acting(&hold);
            work.await;
        })
        .await;
        if !begun.ended() {
            self.acting.set(true);
        }
    }

    /// Hands the participant to the first waiter, once nothing holds it.
    fn pass_on(&self) {
        if self.held() {
            return;
        }
        let first = self.waiting.borrow_mut().pop_front();
        if let Some(turn) = first {
            self.handed.set(true);
            turn.give();
        }
    }
}

/// What lets go of a participant when the work that holds it ends: how it
/// ends does not matter, as Node's `finally` let go of it whether the work
/// answered or threw. A panic that unwinds through the work drops this too,
/// so a participant is never held for good by work that is gone.
struct Release {
    hold: Rc<Hold>,
    /// Whether it is a launch or a delivery going on apart that is let go of.
    acting: bool,
}

impl Release {
    /// For a step or a human's operation.
    fn running(hold: &Rc<Hold>) -> Self {
        Self {
            hold: Rc::clone(hold),
            acting: false,
        }
    }

    /// For a launch or a delivery going on apart from the pass.
    fn acting(hold: &Rc<Hold>) -> Self {
        Self {
            hold: Rc::clone(hold),
            acting: true,
        }
    }
}

impl Drop for Release {
    fn drop(&mut self) {
        if self.acting {
            self.hold.acting.set(false);
        } else {
            self.hold.running.set(false);
        }
        self.hold.pass_on();
    }
}

/// A waiter's turn: given once the participant is handed to it.
#[derive(Default)]
struct Turn {
    given: Cell<bool>,
    waker: RefCell<Option<Waker>>,
}

impl Turn {
    fn give(&self) {
        self.given.set(true);
        let waker = self.waker.borrow_mut().take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

/// A human's operation waiting its turn for a participant. One that goes
/// away before it runs leaves the queue, or passes on the turn it was given.
struct Waiting {
    hold: Rc<Hold>,
    turn: Rc<Turn>,
    taken: bool,
}

impl Waiting {
    fn new(hold: &Rc<Hold>) -> Self {
        let turn = Rc::new(Turn::default());
        hold.waiting.borrow_mut().push_back(Rc::clone(&turn));
        Self {
            hold: Rc::clone(hold),
            turn,
            taken: false,
        }
    }
}

impl Future for Waiting {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        if this.turn.given.get() {
            this.taken = true;
            return Poll::Ready(());
        }
        *this.turn.waker.borrow_mut() = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        if self.taken {
            return;
        }
        if self.turn.given.get() {
            self.hold.handed.set(false);
            self.hold.pass_on();
        } else {
            self.hold
                .waiting
                .borrow_mut()
                .retain(|turn| !Rc::ptr_eq(turn, &self.turn));
        }
    }
}

#[cfg(test)]
mod tests;
