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

use cf_board::{door, Board};
use cf_proto::questions::{Answer, Reply};
use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::Notify;
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

/// What ends one question: raised when the turn that asked it is over, when
/// Codex resolves the request itself (an interrupt does), or when the pair
/// retires. The poll at the board reads it between its requests, and the
/// answer on its way to Codex is not sent once it is raised, whatever stage
/// it is at.
struct Ending {
    raised: Arc<AtomicBool>,
    woken: Notify,
}

impl Ending {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            raised: Arc::new(AtomicBool::new(false)),
            woken: Notify::new(),
        })
    }

    fn raise(&self) {
        self.raised.store(true, Ordering::SeqCst);
        self.woken.notify_one();
    }

    fn is_raised(&self) -> bool {
        self.raised.load(Ordering::SeqCst)
    }

    /// Ends once it is raised: at once if it is.
    async fn wait(&self) {
        while !self.is_raised() {
            self.woken.notified().await;
        }
    }
}

/// A question Codex asked that the board is answering for it: the thread it
/// is on, its request's id, and what ends it. The question is the request's
/// own: it ends with the turn that asked it, with the request's resolution,
/// and not only with the pair, and it is held, answer and all, until it is
/// known whether Codex was handed the answer.
struct Held {
    thread: Option<String>,
    request: Value,
    ending: Rc<Ending>,
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
    /// The questions the board is answering, each with the flag that ends its poll.
    held: RefCell<Vec<Held>>,
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
            held: RefCell::new(Vec::new()),
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

    /// What the writer of the connection to Codex's server writes to: the
    /// connection's own sink.
    #[cfg(not(test))]
    fn native_sink<S>(&self, sink: S) -> S {
        sink
    }

    /// What the writer of the connection to Codex's server writes to: the
    /// connection's own sink, behind the hold a test shuts to keep the writer
    /// on a frame that the socket does not take, as a socket that fills would,
    /// but however much the system's sockets hold.
    #[cfg(test)]
    fn native_sink<S: futures_util::Sink<Message> + Unpin>(
        &self,
        sink: S,
    ) -> super::tests::Held<S> {
        super::tests::Held::new(sink, Rc::clone(&self.shared.native_hold))
    }

    /// The connection to Codex's server opened: everything the TUI said before
    /// goes to it now, in order, and what it says next goes straight through.
    fn native_opened(self: &Rc<Self>, socket: Socket, inbox: Inbox) {
        let (sink, mut stream) = socket.split();
        let this = Rc::clone(self);
        self.spawn(async move {
            if !write_all(this.native_sink(sink), inbox).await {
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
        self.end_questions_of(&message);
        if message.get("method").and_then(Value::as_str) == Some(REQUEST_USER_INPUT) {
            if let (Some(id), Some(board)) = (message.get("id"), &self.shared.board) {
                self.hold_question(id.clone(), message.get("params"), text, Arc::clone(board));
                return;
            }
        }
        self.forward(&self.tui, text);
    }

    /// A question Codex asked is over: the turn its thread was on is over, or
    /// the thread is idle (an interrupt ends a turn in the middle of its
    /// question, and says so with both, and with the request's resolution
    /// too), or Codex says the request itself is resolved. What asked it is
    /// not waiting for an answer any more, so its poll stops, and an answer
    /// on its way to Codex is not sent. Another thread's, and another
    /// request's, go on.
    fn end_questions_of(&self, message: &Value) {
        let params = message.get("params");
        let method = message.get("method").and_then(Value::as_str);
        let turn_over = match method {
            Some("turn/completed") => true,
            Some("thread/status/changed") => {
                params
                    .and_then(|params| params.get("status"))
                    .and_then(|status| status.get("type"))
                    .and_then(Value::as_str)
                    == Some("idle")
            }
            _ => false,
        };
        let thread = params
            .and_then(|params| params.get("threadId"))
            .and_then(Value::as_str);
        let resolved = if method == Some("serverRequest/resolved") {
            params.and_then(|params| params.get("requestId"))
        } else {
            None
        };
        for held in self.held.borrow().iter() {
            let turn_ended = turn_over && thread.is_some() && held.thread.as_deref() == thread;
            if turn_ended || resolved.is_some_and(|request| *request == held.request) {
                held.ending.raise();
            }
        }
    }

    /// Codex asked its client a question: the board answers it, while the
    /// frames of this pair and every other go on. When nobody answers in time
    /// the request goes on to the TUI, as it came, unless the question ended
    /// meanwhile ([`Ending`]): then the request is obsolete, nothing goes to
    /// the TUI, and an answer the board gave is given back, not handed over.
    /// The ending is read and the answer queued in one step, so none comes
    /// between them. An answer is handed over by [`Pair::hand_over`], and is
    /// the board's to hear of only once that is known.
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
        let ending = Ending::new();
        self.held.borrow_mut().push(Held {
            thread: params
                .and_then(|params| params.get("threadId"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            request: id.clone(),
            ending: Rc::clone(&ending),
        });
        // A task of the broker's, not the pair's: a pair that ends while the
        // board is still polled for the question has its poll end at the next
        // request, and an answer the board claimed meanwhile is still given back.
        self.shared.spawn(async move {
            let (asking, polling, raised) = (
                asked.clone(),
                Arc::clone(&board),
                Arc::clone(&ending.raised),
            );
            let outcome =
                tokio::task::spawn_blocking(move || asking.ask(&polling, wait, &raised)).await;
            let ended = ending.is_raised();
            match (outcome, ended) {
                (Ok(Reply::Answered(answer)), false) => {
                    this.hand_over(&asked.answered(&id, &answer), answer, board, ending);
                    return;
                }
                (Ok(Reply::Answered(answer)), true) => acknowledge(board, answer, false).await,
                (Ok(Reply::Refused(reason)), false) => {
                    this.answer_codex(&asked.refused(&id, &reason));
                }
                (Ok(Reply::Unanswered) | Err(_), false) => this.forward(&this.tui, text),
                (_, true) => {}
            }
            this.release(&ending);
        });
    }

    /// The board's answer to Codex's question goes to Codex, and the board is
    /// told whether it was handed over once that is known: the writer says so
    /// when the frame is written to Codex's socket, and the answer is
    /// received only then. If the question ends first (the turn is over,
    /// Codex resolved the request, the pair is gone) the frame is not sent,
    /// or if it is being written already, the answer is not counted as
    /// received all the same: it may have come too late to be taken. A socket
    /// that fails, or is not there, is not a receipt either. The question is
    /// held until then, so its ending is heard, and the board is told by a
    /// task of the broker's own: a pair that ends does not take it with it.
    fn hand_over(
        self: &Rc<Self>,
        response: &Value,
        answer: Answer,
        board: Arc<Board>,
        ending: Rc<Ending>,
    ) {
        let frame = Message::Text(Utf8Bytes::from(response.to_string()));
        let written = match self.native.send_unless(frame, Arc::clone(&ending.raised)) {
            Ok(written) => written,
            Err(_) => {
                self.release(&ending);
                self.retire();
                self.shared.spawn(acknowledge(board, answer, false));
                return;
            }
        };
        let this = Rc::clone(self);
        self.shared.spawn(async move {
            let handed = tokio::select! {
                written = written => written.unwrap_or(false),
                () = ending.wait() => false,
            };
            this.release(&ending);
            acknowledge(board, answer, handed && !ending.is_raised()).await;
        });
    }

    /// The question is no longer held: it was answered, or it ended.
    fn release(&self, ending: &Rc<Ending>) {
        self.held
            .borrow_mut()
            .retain(|held| !Rc::ptr_eq(&held.ending, ending));
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
        for held in self.held.borrow().iter() {
            held.ending.raise();
        }
        self.tui.close();
        self.native.close();
        for task in self.tasks.borrow_mut().drain(..) {
            task.abort();
        }
    }
}

/// Tells the board, off the broker's thread, whether the answer it claimed
/// for Codex's request was handed over: the board's answer to that is of no
/// use to a request that is settled either way.
async fn acknowledge(board: Arc<Board>, answer: Answer, received: bool) {
    let _ = tokio::task::spawn_blocking(move || door::acknowledge(&board, &answer, received)).await;
}
