//! What becomes of the board's answer to Codex's question between the board
//! and Codex's socket: the board hears that it was received only once the
//! frame is written to the socket, and never of one the turn outlived. The
//! writer first, on a socket that takes what the test lets it and fails when
//! it is told to; then a pair, with the question held at a board that answers
//! when the test says and a writer held up behind a frame too big to go.

use std::cell::{Cell, RefCell};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use futures_util::Sink;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use super::fixture::{run, wait, A};
use super::questions::{
    an_ended_turn_ends_the_question_it_asked, asked_of, request_user_input, showed_the_dialog,
    turn_completed, went_idle, window, HeldBoard,
};
use crate::broker::transport::{queue, write_all, Sent};

/// A socket's sink as the test runs it: it takes no frame until it is opened,
/// and fails the one it is given when it is told to.
struct Socket {
    control: Rc<Control>,
}

/// What the test does to the sink, and sees of it.
#[derive(Default)]
struct Control {
    open: Cell<bool>,
    fails: Cell<bool>,
    waiting: RefCell<Option<Waker>>,
    sent: RefCell<Vec<String>>,
}

impl Control {
    /// A socket that takes nothing until it is opened.
    fn shut() -> (Socket, Rc<Self>) {
        let control = Rc::new(Self::default());
        (
            Socket {
                control: Rc::clone(&control),
            },
            control,
        )
    }

    fn open(&self) {
        self.open.set(true);
        if let Some(waker) = self.waiting.borrow_mut().take() {
            waker.wake();
        }
    }

    fn sent(&self) -> Vec<String> {
        self.sent.borrow().clone()
    }
}

impl Sink<Message> for Socket {
    type Error = ();

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), ()>> {
        if self.control.open.get() {
            return Poll::Ready(Ok(()));
        }
        *self.control.waiting.borrow_mut() = Some(cx.waker().clone());
        Poll::Pending
    }

    fn start_send(self: Pin<&mut Self>, message: Message) -> Result<(), ()> {
        if self.control.fails.get() {
            return Err(());
        }
        self.control.sent.borrow_mut().push(message.to_string());
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), ()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), ()>> {
        Poll::Ready(Ok(()))
    }
}

fn not_ended() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[test]
fn a_frame_is_reported_written_once_the_socket_has_it_and_not_before() {
    run(async {
        let (outbox, inbox) = queue();
        let (socket, control) = Control::shut();
        let writer = tokio::task::spawn_local(write_all(socket, inbox));
        let mut report = outbox
            .send_unless(Message::text("answer"), not_ended())
            .unwrap();
        tokio::task::yield_now().await;
        assert!(control.sent().is_empty(), "the socket has taken nothing");
        let pending = tokio::time::timeout(Duration::from_millis(50), &mut report).await;
        assert!(pending.is_err(), "queued is not written");
        control.open();
        assert_eq!(report.await, Ok(true));
        assert_eq!(control.sent(), ["answer"]);
        drop(outbox);
        assert!(writer.await.unwrap());
    });
}

#[test]
fn a_frame_not_wanted_any_more_when_its_turn_comes_is_not_sent_and_is_reported_so() {
    run(async {
        let (outbox, inbox) = queue();
        let (socket, control) = Control::shut();
        let writer = tokio::task::spawn_local(write_all(socket, inbox));
        // A frame the socket does not take yet, and the answer queued behind it.
        assert_eq!(outbox.send(Message::text("first")), Sent::Queued);
        let ended = not_ended();
        let report = outbox
            .send_unless(Message::text("answer"), Arc::clone(&ended))
            .unwrap();
        tokio::task::yield_now().await;
        // The turn is over while the writer is still on the first, and the socket takes it.
        ended.store(true, Ordering::SeqCst);
        control.open();
        assert_eq!(report.await, Ok(false), "taken back, never written");
        assert_eq!(control.sent(), ["first"], "the answer was not sent");
        drop(outbox);
        assert!(
            writer.await.unwrap(),
            "the writer goes on: the socket did not fail"
        );
    });
}

#[test]
fn a_frame_the_writer_has_begun_is_written_whatever_is_raised_meanwhile() {
    run(async {
        let (outbox, inbox) = queue();
        let (socket, control) = Control::shut();
        let writer = tokio::task::spawn_local(write_all(socket, inbox));
        let ended = not_ended();
        let report = outbox
            .send_unless(Message::text("answer"), Arc::clone(&ended))
            .unwrap();
        // The writer has begun it: the socket does not take it yet.
        tokio::task::yield_now().await;
        ended.store(true, Ordering::SeqCst);
        control.open();
        assert_eq!(
            report.await,
            Ok(true),
            "it went: whoever asked counts the end against it"
        );
        assert_eq!(control.sent(), ["answer"]);
        drop(outbox);
        assert!(writer.await.unwrap());
    });
}

#[test]
fn a_socket_that_fails_reports_the_frame_it_failed_on_and_those_behind_it_not_written() {
    run(async {
        let (outbox, inbox) = queue();
        let (socket, control) = Control::shut();
        control.fails.set(true);
        control.open();
        let first = outbox
            .send_unless(Message::text("one"), not_ended())
            .unwrap();
        let behind = outbox
            .send_unless(Message::text("two"), not_ended())
            .unwrap();
        let writer = tokio::task::spawn_local(write_all(socket, inbox));
        assert_eq!(first.await, Ok(false));
        assert!(!writer.await.unwrap(), "the writer says the socket failed");
        assert!(
            behind.await.is_err(),
            "left in the queue, it says nothing, which is not written"
        );
        assert!(control.sent().is_empty());
        assert_eq!(
            outbox.send(Message::text("three")),
            Sent::Closed,
            "nothing more is taken for it"
        );
    });
}

/// A frame of `size` bytes of padding, which the TUI says to Codex.
fn padded(size: usize) -> String {
    format!(
        r#"{{"id":"pad","method":"pad","params":{{"padding":"{}"}}}}"#,
        "x".repeat(size)
    )
}

/// Codex says its request `id` on `thread` is resolved.
fn resolved(thread: &str, id: &str) -> Value {
    json!({
        "method": "serverRequest/resolved",
        "params": { "threadId": thread, "requestId": id },
    })
}

#[test]
fn a_request_codex_resolved_ends_the_poll_of_the_question_it_was_and_the_answer_is_given_back() {
    run(an_ended_turn_ends_the_question_it_asked(resolved(
        A, "ask-1",
    )));
}

#[test]
fn what_codex_says_of_an_interrupted_question_all_at_once_gives_the_answer_back_once() {
    run(async {
        // The three that came 7 ms after an interrupt on Codex 0.160.1.
        let board = HeldBoard::start(json!({ "id": 70, "choices": [["red"]] }));
        let (f, tui) = window(&board.url, Duration::from_secs(60)).await;
        f.codex.send_json(1, &request_user_input());
        wait(|| board.polls() == 1).await;
        for said in [turn_completed(A), went_idle(A), resolved(A, "ask-1")] {
            f.codex.send_json(1, &said);
        }
        wait(|| tui.has_seen(|message| message["method"] == "serverRequest/resolved")).await;
        board.release();
        wait(|| !board.receipts().is_empty()).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(board.receipts(), [json!(false)], "given back once");
        assert!(asked_of(&f, "ask-1").is_none());
        assert!(!showed_the_dialog(&tui));
    });
}

#[test]
fn the_resolution_of_another_request_leaves_the_question_to_be_answered() {
    run(async {
        let board = HeldBoard::start(json!({ "id": 70, "choices": [["red"]] }));
        let (f, tui) = window(&board.url, Duration::from_secs(60)).await;
        f.codex.send_json(1, &request_user_input());
        wait(|| board.polls() == 1).await;
        f.codex.send_json(1, &resolved(A, "another-request"));
        wait(|| tui.has_seen(|message| message["method"] == "serverRequest/resolved")).await;
        board.release();
        wait(|| !board.receipts().is_empty()).await;
        assert_eq!(
            asked_of(&f, "ask-1").unwrap()["result"]["answers"]["colour"]["answers"],
            json!(["red"])
        );
        assert_eq!(board.receipts(), [json!(true)]);
    });
}

#[test]
fn an_answer_waiting_to_be_written_is_not_a_receipt_and_the_turn_ending_gives_it_back_at_once() {
    run(async {
        let board = HeldBoard::start(json!({ "id": 70, "choices": [["red"]] }));
        let (f, tui) = window(&board.url, Duration::from_secs(60)).await;
        f.codex.send_json(1, &request_user_input());
        wait(|| board.polls() == 1).await;
        // Codex's end of the TUI's connection stops reading, and the TUI says
        // something too big for the socket to take: the writer is held up on it.
        f.codex.stall_peer(1);
        tui.send_text(&padded(20 * 1024 * 1024));
        tokio::time::sleep(Duration::from_millis(300)).await;
        // The board answers: the frame is queued behind it, and nothing is said of it.
        board.release();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(board.polls(), 1);
        assert!(
            board.receipts().is_empty(),
            "queued is not handed over: the board is told nothing yet, {:?}",
            board.receipts()
        );
        // The turn that asked is over before the writer got to the answer.
        f.codex.send_json(1, &turn_completed(A));
        wait(|| !board.receipts().is_empty()).await;
        assert_eq!(board.receipts(), [json!(false)]);
        assert!(asked_of(&f, "ask-1").is_none());
    });
}

#[test]
fn an_answer_waiting_to_be_written_when_the_socket_to_codex_goes_is_given_back() {
    run(async {
        let board = HeldBoard::start(json!({ "id": 70, "choices": [["red"]] }));
        let (f, tui) = window(&board.url, Duration::from_secs(60)).await;
        f.codex.send_json(1, &request_user_input());
        wait(|| board.polls() == 1).await;
        f.codex.stall_peer(1);
        tui.send_text(&padded(20 * 1024 * 1024));
        tokio::time::sleep(Duration::from_millis(300)).await;
        board.release();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(board.receipts().is_empty(), "nothing was handed over");
        // Codex's end of the connection goes with the answer still waiting for its turn to be written.
        f.codex.terminate(1);
        wait(|| !board.receipts().is_empty()).await;
        assert_eq!(board.receipts(), [json!(false)]);
        assert!(
            !showed_the_dialog(&tui),
            "the request is the turn's that is gone: no dialog for it"
        );
    });
}

#[test]
fn a_pair_that_ends_while_the_board_is_still_asked_gives_back_the_answer_the_poll_in_hand_gets() {
    run(async {
        let board = HeldBoard::start(json!({ "id": 70, "choices": [["red"]] }));
        let (f, tui) = window(&board.url, Duration::from_secs(60)).await;
        f.codex.send_json(1, &request_user_input());
        wait(|| board.polls() == 1).await;
        tui.terminate();
        wait(|| !f.codex.is_open(1)).await;
        // The poll in hand is answered after the pair has gone: the answer it claimed is not Codex's.
        board.release();
        wait(|| !board.receipts().is_empty()).await;
        assert_eq!(board.receipts(), [json!(false)]);
        assert!(asked_of(&f, "ask-1").is_none());
    });
}
