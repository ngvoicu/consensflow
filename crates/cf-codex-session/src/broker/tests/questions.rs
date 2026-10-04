//! Codex's question tool, answered from the board by the broker.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::rc::Weak;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use cf_board::scripted::{reply, scripted};
use cf_board::Board;
use serde_json::{json, Value};

use super::fixture::{run, wait, Fixture, Options, Tui, A};

/// Codex's request for the question tool, as the app-server sends it.
fn request_user_input() -> Value {
    json!({
        "id": "ask-1",
        "method": "item/tool/requestUserInput",
        "params": {
            "threadId": A,
            "turnId": "turn-1",
            "itemId": "item-1",
            "isBlocking": true,
            "questions": [{
                "id": "colour",
                "header": "Colour",
                "question": "Which colour?",
                "options": [
                    { "label": "red", "description": "Warm" },
                    { "label": "blue", "description": "Cool" },
                ],
            }],
        },
    })
}

/// A board that takes a question and holds the answer to its polls until the
/// test lets it go, so what happens while a question is held does not depend
/// on how fast the test runs.
struct HeldBoard {
    url: String,
    released: Arc<AtomicBool>,
    polls: Arc<AtomicUsize>,
}

impl HeldBoard {
    /// A board whose polls, once released, are answered with `answer`.
    fn start(answer: Value) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("its address"));
        let released = Arc::new(AtomicBool::new(false));
        let polls = Arc::new(AtomicUsize::new(0));
        let (held, counted) = (Arc::clone(&released), Arc::clone(&polls));
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (held, counted, answer) =
                    (Arc::clone(&held), Arc::clone(&counted), answer.clone());
                thread::spawn(move || serve(stream, &held, &counted, &answer));
            }
        });
        Self {
            url,
            released,
            polls,
        }
    }

    /// Lets the held polls answer.
    fn release(&self) {
        self.released.store(true, Ordering::SeqCst);
    }

    /// How many polls it has been asked.
    fn polls(&self) -> usize {
        self.polls.load(Ordering::SeqCst)
    }
}

/// One request of the board: a question is taken, a poll waits to be released.
fn serve(stream: TcpStream, released: &AtomicBool, polls: &AtomicUsize, answer: &Value) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone the stream"));
    let mut request = String::new();
    reader.read_line(&mut request).expect("a request line");
    let mut length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).expect("a header line");
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).expect("a body");
    let (status, reply) = if request.starts_with("POST") {
        (201, json!({ "message": { "id": 61 } }))
    } else {
        polls.fetch_add(1, Ordering::SeqCst);
        while !released.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(5));
        }
        (200, json!({ "question": {}, "answer": answer }))
    };
    let text = reply.to_string();
    let mut stream = stream;
    // The window may be gone by the time its poll is answered.
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
        text.len()
    );
}

/// A window whose board is at `url`, and a TUI that connected to it. Codex's end of the TUI's connection is peer 1.
async fn window(url: &str, question_wait: Duration) -> (Fixture, Tui) {
    let board = Arc::new(Board::new(Some(url), "window-token"));
    let f = Fixture::with(Options {
        board: Some(board),
        question_wait,
        ..Options::default()
    })
    .await;
    let tui = f.connect().await;
    (f, tui)
}

fn asked_of(f: &Fixture, id: &str) -> Option<Value> {
    f.codex
        .requests()
        .into_iter()
        .find(|message| message["id"] == id)
}

fn showed_the_dialog(tui: &Tui) -> bool {
    tui.has_seen(|message| message["method"] == "item/tool/requestUserInput")
}

#[test]
fn codexs_question_tool_is_answered_from_the_board_by_the_broker_and_the_tui_never_sees_it() {
    run(async {
        let api = scripted(vec![
            reply(201, json!({ "message": { "id": 61 } })),
            reply(
                200,
                json!({ "question": {}, "answer": {
                    "id": 70, "from": "chief", "body": "Colour: blue", "choices": [["blue"]],
                } }),
            ),
        ]);
        let (f, tui) = window(&api.url, Duration::from_secs(5)).await;
        f.codex.send_json(1, &request_user_input());
        wait(|| !api.received().is_empty()).await;
        let posted = api.received()[0].clone();
        assert_eq!(
            (posted.method.as_str(), posted.path.as_str()),
            ("POST", "/api/questions")
        );
        assert_eq!(posted.authorization.as_deref(), Some("Bearer window-token"));
        assert_eq!(
            posted.json().unwrap()["questions"],
            json!([{
                "question": "Which colour?",
                "header": "Colour",
                "options": [
                    { "label": "red", "description": "Warm" },
                    { "label": "blue", "description": "Cool" },
                ],
                "multiple": false,
            }])
        );
        wait(|| asked_of(&f, "ask-1").is_some()).await;
        assert_eq!(
            asked_of(&f, "ask-1").unwrap(),
            json!({ "id": "ask-1", "result": { "answers": { "colour": { "answers": ["blue"] } } } })
        );
        assert!(!showed_the_dialog(&tui), "the window showed no dialog");
    });
}

#[test]
fn codexs_question_the_board_refuses_is_answered_with_the_reason_never_left_to_a_dialog_nobody_sees(
) {
    run(async {
        let api = scripted(vec![reply(
            400,
            json!({ "error": "bad-questions", "message": "questions: one to 4 questions" }),
        )]);
        let (f, tui) = window(&api.url, Duration::from_secs(5)).await;
        f.codex.send_json(1, &request_user_input());
        wait(|| asked_of(&f, "ask-1").is_some()).await;
        assert_eq!(
            asked_of(&f, "ask-1").unwrap(),
            json!({
                "id": "ask-1",
                "result": { "answers": { "colour": { "answers": [
                    "ConsensFlow could not put this question to the chief (questions: one to 4 questions). Ask with cf ask \"…\" instead."
                ] } } },
            })
        );
        assert!(!showed_the_dialog(&tui));
    });
}

#[test]
fn codexs_question_goes_on_to_the_tui_when_the_board_does_not_answer_in_time_or_there_is_no_board()
{
    run(async {
        let api = scripted(vec![
            reply(201, json!({ "message": { "id": 61 } })),
            reply(200, json!({ "question": {}, "answer": null })).held(Duration::from_millis(200)),
        ]);
        let (f, tui) = window(&api.url, Duration::from_millis(50)).await;
        f.codex.send_json(1, &request_user_input());
        wait(|| showed_the_dialog(&tui)).await;
        let shown = tui
            .seen()
            .into_iter()
            .find(|message| message["id"] == "ask-1")
            .unwrap();
        assert_eq!(shown, request_user_input(), "it goes on as it came");
        let posts = api
            .received()
            .iter()
            .filter(|received| received.method == "POST")
            .count();
        assert_eq!(posts, 1, "it was asked on the board first");
        assert!(
            asked_of(&f, "ask-1").is_none(),
            "nothing answered it for Codex"
        );

        let plain = Fixture::start().await;
        let plain_tui = plain.connect().await;
        plain.codex.send_json(1, &request_user_input());
        wait(|| showed_the_dialog(&plain_tui)).await;
    });
}

#[test]
fn codexs_question_goes_on_to_the_tui_when_the_board_cannot_be_reached() {
    run(async {
        let board = Arc::new(Board::new(Some("http://127.0.0.1:9"), "window-token"));
        let f = Fixture::with(Options {
            board: Some(board),
            ..Options::default()
        })
        .await;
        let tui = f.connect().await;
        f.codex.send_json(1, &request_user_input());
        wait(|| showed_the_dialog(&tui)).await;
        assert!(asked_of(&f, "ask-1").is_none());
    });
}

#[test]
fn a_question_the_door_cannot_read_or_that_has_no_id_goes_on_to_the_tui_without_asking_the_board() {
    run(async {
        let api = scripted(vec![reply(201, json!({ "message": { "id": 61 } }))]);
        let (f, tui) = window(&api.url, Duration::from_secs(5)).await;
        let mut unreadable = request_user_input();
        unreadable["params"]["questions"] = json!({ "not": "a list" });
        unreadable["id"] = json!("ask-2");
        let mut anonymous = request_user_input();
        anonymous.as_object_mut().unwrap().remove("id");
        f.codex.send_json(1, &unreadable);
        f.codex.send_json(1, &anonymous);
        wait(|| {
            tui.has_seen(|message| message["id"] == "ask-2")
                && tui.has_seen(|message| {
                    message["method"] == "item/tool/requestUserInput" && message.get("id").is_none()
                })
        })
        .await;
        assert!(api.received().is_empty(), "the board was not asked");
    });
}

#[test]
fn a_question_held_at_the_board_holds_up_neither_deliveries_nor_the_frames_of_the_tui() {
    run(async {
        let board = HeldBoard::start(json!({ "id": 70, "choices": [["red"]] }));
        let (f, tui) = window(&board.url, Duration::from_secs(60)).await;
        f.start_thread(&tui, 1, json!({ "id": A, "status": { "type": "idle" } }))
            .await;
        f.codex.send_json(1, &request_user_input());
        // The board has the question and holds its answer.
        wait(|| board.polls() == 1).await;
        // Meanwhile a delivery is taken, and what the TUI says goes through, and what Codex says to it.
        let (delivered, started) = tokio::join!(
            f.deliver(A, json!({})),
            f.respond("turn/start", json!({ "turn": { "id": "turn-1" } })),
        );
        assert_eq!(delivered, json!({ "ok": true, "admitted": true }));
        assert_eq!(started["params"]["threadId"], A);
        tui.send(json!({ "id": 9, "method": "later", "params": {} }));
        wait(|| f.codex.is_held_by_id(&json!(9))).await;
        f.codex.send_json(
            1,
            &json!({ "method": "turn/started", "params": { "threadId": A } }),
        );
        wait(|| tui.has_seen(|message| message["method"] == "turn/started")).await;
        assert!(
            asked_of(&f, "ask-1").is_none(),
            "the board has not answered yet"
        );
        // Then the answer comes, and Codex gets it.
        board.release();
        wait(|| asked_of(&f, "ask-1").is_some()).await;
        assert_eq!(
            asked_of(&f, "ask-1").unwrap()["result"]["answers"]["colour"]["answers"],
            json!(["red"])
        );
        let order: Vec<_> = f
            .codex
            .requests()
            .iter()
            .filter_map(|message| {
                (message["method"] == "turn/start")
                    .then_some("turn/start")
                    .or_else(|| (message["id"] == "ask-1").then_some("answer"))
            })
            .collect();
        assert_eq!(order, ["turn/start", "answer"]);
    });
}

#[test]
fn a_question_still_held_at_the_board_does_not_hold_up_the_end_of_the_window() {
    let api = scripted(vec![
        reply(201, json!({ "message": { "id": 61 } })),
        reply(200, json!({ "question": {}, "answer": null })).held(Duration::from_secs(4)),
    ]);
    let started = std::time::Instant::now();
    run(async {
        let (f, _tui) = window(&api.url, Duration::from_secs(3600)).await;
        f.codex.send_json(1, &request_user_input());
        wait(|| api.received().len() == 2).await;
        // The window ends here, the board's poll held for seconds more.
    });
    let took = started.elapsed();
    assert!(took < Duration::from_secs(3), "{took:?}");
}

#[test]
fn a_question_still_held_when_the_broker_closes_stops_asking_at_its_next_poll() {
    run(async {
        // Released with no answer, the poll in hand returns and the door would
        // ask again: a second poll is a question nobody stopped.
        let board = HeldBoard::start(Value::Null);
        let (f, tui) = window(&board.url, Duration::from_secs(60)).await;
        f.codex.send_json(1, &request_user_input());
        wait(|| board.polls() == 1).await;
        f.broker.close().await;
        board.release();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(board.polls(), 1, "no poll after the broker closed");
        assert!(
            !showed_the_dialog(&tui),
            "nothing was left for a window that is gone"
        );
        assert!(asked_of(&f, "ask-1").is_none());
    });
}

#[test]
fn a_question_whose_tui_connection_ended_stops_asking_at_its_next_poll_and_the_others_go_on() {
    run(async {
        let board = HeldBoard::start(Value::Null);
        let (f, tui) = window(&board.url, Duration::from_secs(60)).await;
        let other = f.connect().await;
        f.codex.send_json(1, &request_user_input());
        wait(|| board.polls() == 1).await;
        tui.terminate();
        // Codex's end of its connection is closed once the broker has ended the pair.
        wait(|| !f.codex.is_open(1)).await;
        board.release();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(board.polls(), 1, "no poll once its connection ended");
        assert!(asked_of(&f, "ask-1").is_none());
        assert!(!other.is_closed(), "another TUI's connection is its own");
    });
}

#[test]
fn a_pair_keeps_no_handle_of_a_question_answered_long_ago() {
    run(async {
        let replies = (0..4)
            .flat_map(|at| {
                [
                    reply(201, json!({ "message": { "id": 61 + at } })),
                    reply(
                        200,
                        json!({ "question": {}, "answer": {
                            "id": 70 + at, "from": "chief", "body": "Colour: blue", "choices": [["blue"]],
                        } }),
                    ),
                ]
            })
            .collect();
        let api = scripted(replies);
        let (f, _tui) = window(&api.url, Duration::from_secs(5)).await;
        let held = || {
            f.broker
                .shared
                .pairs
                .borrow()
                .values()
                .find_map(Weak::upgrade)
                .map_or(0, |pair| pair.tasks_held())
        };
        let mut after_one = 0;
        for at in 1..=4 {
            let id = format!("ask-{at}");
            let mut request = request_user_input();
            request["id"] = json!(id);
            f.codex.send_json(1, &request);
            wait(|| asked_of(&f, &id).is_some()).await;
            if at == 1 {
                after_one = held();
            }
        }
        assert_eq!(
            held(),
            after_one,
            "four answered questions hold no more than one"
        );
    });
}
