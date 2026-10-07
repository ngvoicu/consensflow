//! A hold on the writer of a pair's connection to Codex's server: while the
//! test keeps it shut, the sink the writer writes to takes no frame, whatever
//! the system's sockets would have taken (how much a socket holds unread is the
//! system's: Windows' loopback holds far more than macOS's), and once it is
//! opened the writer goes on as it was.

use std::cell::{Cell, RefCell};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use futures_util::Sink;
use tokio_tungstenite::tungstenite::Message;

/// What the test does to the hold, and sees of it.
#[derive(Default)]
pub(crate) struct Hold {
    shut: Cell<bool>,
    /// Whether the writer is waiting at the hold, a frame in hand.
    holding: Cell<bool>,
    waiting: RefCell<Option<Waker>>,
}

impl Hold {
    /// From now on the sink takes no frame.
    pub(crate) fn shut(&self) {
        self.shut.set(true);
    }

    /// The sink takes frames again, the one the writer is held on first.
    pub(crate) fn open(&self) {
        self.shut.set(false);
        self.holding.set(false);
        if let Some(waker) = self.waiting.borrow_mut().take() {
            waker.wake();
        }
    }

    /// Whether the writer has a frame in hand that the sink does not take.
    pub(crate) fn holds_a_frame(&self) -> bool {
        self.holding.get()
    }
}

/// `sink`, behind a hold.
pub(crate) struct Held<S> {
    sink: S,
    hold: Rc<Hold>,
}

impl<S> Held<S> {
    pub(crate) fn new(sink: S, hold: Rc<Hold>) -> Self {
        Self { sink, hold }
    }
}

impl<S: Sink<Message> + Unpin> Sink<Message> for Held<S> {
    type Error = S::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        if self.hold.shut.get() {
            self.hold.holding.set(true);
            *self.hold.waiting.borrow_mut() = Some(cx.waker().clone());
            return Poll::Pending;
        }
        Pin::new(&mut self.sink).poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, message: Message) -> Result<(), S::Error> {
        Pin::new(&mut self.sink).start_send(message)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        Pin::new(&mut self.sink).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        Pin::new(&mut self.sink).poll_close(cx)
    }
}
