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
    /// until it ends.
    async fn hold_for<T: 'static>(
        self: &Rc<Self>,
        spawn: &dyn Spawn,
        work: impl Future<Output = T> + 'static,
    ) -> Begun<T> {
        let hold = Rc::clone(self);
        let begun = begin(spawn, async move {
            let answer = work.await;
            hold.running.set(false);
            hold.pass_on();
            answer
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
    /// held until it ends, and the participant passed on.
    async fn run_held<T>(&self, work: impl Future<Output = T>) -> T {
        let mut work = pin!(work);
        let answer = match poll_fn(|context| Poll::Ready(work.as_mut().poll(context))).await {
            Poll::Ready(answer) => answer,
            Poll::Pending => {
                self.running.set(true);
                work.await
            }
        };
        self.running.set(false);
        self.pass_on();
        answer
    }

    /// Begins `work` apart from the pass, holding the participant until it
    /// ends (`#act`): a launch or a delivery a step started.
    pub async fn act(self: &Rc<Self>, spawn: &dyn Spawn, work: impl Future<Output = ()> + 'static) {
        let hold = Rc::clone(self);
        let begun = begin(spawn, async move {
            work.await;
            hold.acting.set(false);
            hold.pass_on();
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
