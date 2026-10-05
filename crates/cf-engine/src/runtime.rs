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
//! - A participant is let go a few turns after its work ended, and a waiter
//!   learns of it a few turns after that ([`Hold`]): `#exclusive` and `#act`
//!   begin the work inside promises that settle, and clear `running` and
//!   `acting`, only after the work's own promise did.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::{poll_fn, Future};
use std::pin::{pin, Pin};
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use futures_util::stream::{FuturesUnordered, StreamExt};

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
pub async fn begin<T: 'static>(
    spawn: &dyn Spawn,
    work: impl Future<Output = T> + 'static,
) -> Begun<T> {
    let answer = Rc::new(Answer::default());
    let mut whole: LocalWork = Box::pin({
        let answer = Rc::clone(&answer);
        async move { answer.set(work.await) }
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

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let value = self.answer.value.borrow_mut().take();
        match value {
            Some(value) => Poll::Ready(value),
            None => {
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
    waker: RefCell<Option<Waker>>,
}

impl<T> Default for Answer<T> {
    fn default() -> Self {
        Self {
            value: RefCell::new(None),
            ended: Cell::new(false),
            waker: RefCell::new(None),
        }
    }
}

impl<T> Answer<T> {
    fn set(&self, value: T) {
        *self.value.borrow_mut() = Some(value);
        self.ended.set(true);
        let waker = self.waker.borrow_mut().take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
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

/// Turns a participant stays held after its work ended (`Hold::lasting`).
/// `#exclusive` and `#act` (`src/core/dispatcher.js`) begin the work in
/// `begin(work)`, an `async` function that adopts the work's promise, and
/// clear `running` or `acting` in the `finally` of an `async` function that
/// awaits `begin`'s: adopting a promise takes a turn to ask it and a turn
/// after it settled, and the `finally` goes on a turn after that. A promise
/// settled already when asked, as that of work that waited on nothing was,
/// costs one turn more.
const LET_GO_AFTER_WAIT: usize = 2;
const LET_GO_AFTER_START: usize = 3;

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
            // `await (runtime.running ?? runtime.acting).catch(() => {})` in
            // `#exclusive`: the `catch` made a promise of its own, so the
            // waiter goes on a turn after the one it was woken in.
            next_turn().await;
            hold.handed.set(false);
            hold.lasting(&hold.running, work).await
        })
        .await
    }

    /// Begins `work` in place, holding the participant from its first wait
    /// until it ends.
    async fn hold_for<T: 'static>(
        self: &Rc<Self>,
        spawn: &dyn Spawn,
        work: impl Future<Output = T> + 'static,
    ) -> Begun<T> {
        let hold = Rc::clone(self);
        begin(
            spawn,
            async move { hold.lasting(&hold.running, work).await },
        )
        .await
    }

    /// Begins `work` apart from the pass, holding the participant until it
    /// ends (`#act`): a launch or a delivery a step started.
    pub async fn act(self: &Rc<Self>, spawn: &dyn Spawn, work: impl Future<Output = ()> + 'static) {
        let hold = Rc::clone(self);
        drop(begin(spawn, async move { hold.lasting(&hold.acting, work).await }).await);
    }

    /// Runs `work` as `#exclusive` and `#act` ran it: its start in place, as
    /// JavaScript ran it, with nothing held until it is done; then `held` is
    /// set, until the work has ended and the promises it was begun in have
    /// settled ([`LET_GO_AFTER_WAIT`], [`LET_GO_AFTER_START`]). Then the
    /// participant is passed on.
    async fn lasting<T>(&self, held: &Cell<bool>, work: impl Future<Output = T>) -> T {
        let mut work = pin!(work);
        let mut started = false;
        let mut at_once = false;
        let answer = poll_fn(|context| {
            let polled = work.as_mut().poll(context);
            if !started {
                started = true;
                at_once = polled.is_ready();
                held.set(true);
            }
            polled
        })
        .await;
        let turns = if at_once {
            LET_GO_AFTER_START
        } else {
            LET_GO_AFTER_WAIT
        };
        for _ in 0..turns {
            next_turn().await;
        }
        held.set(false);
        self.pass_on();
        answer
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
#[cfg(test)]
mod turns;
