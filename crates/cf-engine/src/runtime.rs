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

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::{poll_fn, Future};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

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

    /// Runs `work` for this participant once nothing else holds it
    /// (`#exclusive`): none when it is held and `wait` is false, as a pass
    /// that finds it busy moves on; with `wait`, once its turn comes. The
    /// work is begun in place, and holds the participant until it ends.
    pub async fn exclusive<T: 'static>(
        self: &Rc<Self>,
        spawn: &dyn Spawn,
        work: impl Future<Output = T> + 'static,
        wait: bool,
    ) -> Option<Begun<T>> {
        if self.held() {
            if !wait {
                return None;
            }
            Waiting::new(self).await;
            self.handed.set(false);
        }
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
        Some(begun)
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
