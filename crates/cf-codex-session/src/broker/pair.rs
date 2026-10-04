//! A TUI's connection and the connection to Codex's server that goes with it:
//! one pair, proxied frame by frame in both directions. The broker reads what
//! passes to know which thread the window shows, and forwards every frame as
//! it came. A TUI may open more than one (its thread picker does): each has a
//! pair of its own, and only the one that chose the thread owns it.
//!
//! A pair ends as a whole: whichever of its sockets fails, closes, or says
//! something that is not JSON takes the other with it.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cf_board::Board;
use cf_proto::questions::Reply;
use futures_util::StreamExt;
use serde_json::Value;
use tokio::task::AbortHandle;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

use super::selection::{ClientId, Requests};
use super::transport::{
    connect, next_frame, queue, reading, write_all, Frame, Inbox, Outbox, Reading, Sent, Socket,
    MAX_FRAME,
};
use super::Shared;
use crate::questions::{Asked, REQUEST_USER_INPUT};

/// What the TUI said before the connection to Codex's server opened.
struct Waiting {
    frames: Vec<Utf8Bytes>,
    bytes: usize,
}

/// What became of a frame the TUI said.
enum Said {
    /// The connection to Codex's server is open: it goes through.
    Pass(Utf8Bytes),
    /// It waits for that connection.
    Held,
    /// More than 64 MiB wait already.
    TooMuch,
}

pub(super) struct Pair {
    id: ClientId,
    shared: Rc<Shared>,
    /// What this TUI asked and Codex has not answered yet.
    requests: RefCell<Requests>,
    /// Frames from the TUI until its connection to Codex's server opens, in
    /// order; none once it has.
    waiting: RefCell<Option<Waiting>>,
    tui: Outbox,
    native: Outbox,
    /// Raised when the pair ends, so a question it asked stops at its next poll.
    cancel: Arc<AtomicBool>,
    retired: Cell<bool>,
    tasks: RefCell<Vec<AbortHandle>>,
}

impl Pair {
    /// Proxies `socket`, a TUI's connection that just opened, to a connection
    /// of its own to Codex's server.
    pub(super) fn start(shared: &Rc<Shared>, socket: Socket) {
        let (sink, mut stream) = socket.split();
        let (tui, tui_inbox) = queue();
        let (native, native_inbox) = queue();
        let pair = Rc::new(Self {
            id: shared.next_client(),
            shared: Rc::clone(shared),
            requests: RefCell::new(Requests::default()),
            waiting: RefCell::new(Some(Waiting {
                frames: Vec::new(),
                bytes: 0,
            })),
            tui,
            native,
            cancel: Arc::new(AtomicBool::new(false)),
            retired: Cell::new(false),
            tasks: RefCell::new(Vec::new()),
        });
        shared
            .pairs
            .borrow_mut()
            .insert(pair.id, Rc::downgrade(&pair));
        let this = Rc::clone(&pair);
        pair.spawn(async move {
            if !write_all(sink, tui_inbox).await {
                this.retire();
            }
        });
        let this = Rc::clone(&pair);
        pair.spawn(async move {
            while let Frame::Json(text) = next_frame(&mut stream).await {
                this.tui_said(text);
                if this.retired.get() {
                    return;
                }
            }
            this.retire();
        });
        let this = Rc::clone(&pair);
        pair.spawn(async move {
            match connect(&this.shared.upstream).await {
                Ok(native) => this.native_opened(native, native_inbox),
                Err(_) => this.retire(),
            }
        });
    }

    fn spawn(&self, task: impl Future<Output = ()> + 'static) {
        let task = self.shared.spawn(task);
        if self.retired.get() {
            task.abort();
        } else {
            // A question answered long ago has nothing left to end.
            let mut tasks = self.tasks.borrow_mut();
            tasks.retain(|task| !task.is_finished());
            tasks.push(task);
        }
    }

    /// The connection to Codex's server opened: everything the TUI said before
    /// goes to it now, in order, and what it says next goes straight through.
    fn native_opened(self: &Rc<Self>, socket: Socket, inbox: Inbox) {
        let (sink, mut stream) = socket.split();
        let this = Rc::clone(self);
        self.spawn(async move {
            if !write_all(sink, inbox).await {
                this.retire();
            }
        });
        let this = Rc::clone(self);
        self.spawn(async move {
            while let Frame::Json(text) = next_frame(&mut stream).await {
                this.codex_said(text);
                if this.retired.get() {
                    return;
                }
            }
            this.retire();
        });
        let waiting = self.waiting.borrow_mut().take();
        for text in waiting.into_iter().flat_map(|waiting| waiting.frames) {
            if self.retired.get() {
                return;
            }
            self.process_tui(text);
        }
    }

    /// A frame from the TUI: held until the connection to Codex's server
    /// opens (64 MiB at most, else the pair ends), then passed through.
    fn tui_said(self: &Rc<Self>, text: Utf8Bytes) {
        let said = match self.waiting.borrow_mut().as_mut() {
            None => Said::Pass(text),
            Some(waiting) => {
                waiting.bytes += text.len();
                if waiting.bytes > MAX_FRAME {
                    Said::TooMuch
                } else {
                    waiting.frames.push(text);
                    Said::Held
                }
            }
        };
        match said {
            Said::Pass(text) => self.process_tui(text),
            Said::Held => {}
            Said::TooMuch => self.retire(),
        }
    }

    /// A frame the TUI said to Codex: what it may change is learned, and it
    /// goes on as it came, except a main start that must open in
    /// full-permission mode.
    fn process_tui(self: &Rc<Self>, text: Utf8Bytes) {
        let message = match reading(&text) {
            Reading::Message(message) => message,
            Reading::TooDeep => return self.forward(&self.native, text),
            Reading::NotJson => return self.retire(),
        };
        let rewritten = self.shared.state.borrow_mut().tui_said(
            self.id,
            &mut self.requests.borrow_mut(),
            &message,
        );
        let out = rewritten.map_or(text, |message| Utf8Bytes::from(message.to_string()));
        self.forward(&self.native, out);
    }

    /// A frame Codex said to the TUI: an answer that names a thread is
    /// learned, and a question goes to the board when there is one.
    fn codex_said(self: &Rc<Self>, text: Utf8Bytes) {
        let message = match reading(&text) {
            Reading::Message(message) => message,
            Reading::TooDeep => return self.forward(&self.tui, text),
            Reading::NotJson => return self.retire(),
        };
        self.shared.state.borrow_mut().codex_said(
            self.id,
            &mut self.requests.borrow_mut(),
            &message,
        );
        if message.get("method").and_then(Value::as_str) == Some(REQUEST_USER_INPUT) {
            if let (Some(id), Some(board)) = (message.get("id"), &self.shared.board) {
                self.hold_question(id.clone(), message.get("params"), text, Arc::clone(board));
                return;
            }
        }
        self.forward(&self.tui, text);
    }

    /// Codex asked its client a question: the board answers it, while the
    /// frames of this pair and every other go on. When nobody answers in time
    /// the request goes on to the TUI, as it came.
    fn hold_question(
        self: &Rc<Self>,
        id: Value,
        params: Option<&Value>,
        text: Utf8Bytes,
        board: Arc<Board>,
    ) {
        let Some(asked) = Asked::read(params) else {
            self.forward(&self.tui, text);
            return;
        };
        let this = Rc::clone(self);
        let wait = self.shared.question_wait;
        let stop = Arc::clone(&self.cancel);
        self.spawn(async move {
            let asking = asked.clone();
            let outcome =
                tokio::task::spawn_blocking(move || asking.ask(&board, wait, &stop)).await;
            match outcome {
                Ok(Reply::Answered(answer)) => this.answer_codex(&asked.answered(&id, &answer)),
                Ok(Reply::Refused(reason)) => this.answer_codex(&asked.refused(&id, &reason)),
                Ok(Reply::Unanswered) | Err(_) => this.forward(&this.tui, text),
            }
        });
    }

    /// How many of its tasks the pair still holds a handle to.
    #[cfg(test)]
    pub(super) fn tasks_held(&self) -> usize {
        self.tasks.borrow().len()
    }

    fn answer_codex(&self, response: &Value) {
        self.forward(&self.native, Utf8Bytes::from(response.to_string()));
    }

    /// Queues `text` for `destination`; a destination that is closed, or that
    /// would hold more than 64 MiB unsent, ends the pair.
    fn forward(&self, destination: &Outbox, text: Utf8Bytes) {
        if destination.send(Message::Text(text)) != Sent::Queued {
            self.retire();
        }
    }

    /// Ends the pair: when its TUI chose the window's thread, the window shows
    /// none now; both sockets close, and a question it asked stops asking.
    pub(super) fn retire(&self) {
        if self.retired.replace(true) {
            return;
        }
        self.shared.state.borrow_mut().retire(self.id);
        self.shared.pairs.borrow_mut().remove(&self.id);
        self.cancel.store(true, Ordering::Relaxed);
        self.tui.close();
        self.native.close();
        for task in self.tasks.borrow_mut().drain(..) {
            task.abort();
        }
    }
}
