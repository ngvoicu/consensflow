use super::*;
use crate::scripted::{hang_up, reply, scripted};
use cf_proto::questions::Choice;

fn one_question() -> Vec<Question> {
    vec![Question {
        question: Some("Which database?".into()),
        header: Some("Database".into()),
        options: vec![
            Choice {
                label: Some("SQLite".into()),
                description: Some("one file".into()),
            },
            Choice {
                label: Some("Postgres".into()),
                description: None,
            },
        ],
        multiple: false,
    }]
}

#[test]
fn puts_the_questions_on_the_board_and_polls_until_the_answer_comes() {
    let api = scripted(vec![
        reply(201, json!({ "message": { "id": 12 } })),
        reply(200, json!({ "question": {}, "answer": null })),
        reply(
            200,
            json!({ "question": {}, "answer": { "id": 13, "choices": [["SQLite"]] } }),
        ),
    ]);
    let board = Board::new(Some(&api.url), "tok");
    let answer = ask_the_board_until(&board, &one_question(), DOOR_WAIT, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(answer.picks(0), ["SQLite".to_string()]);
    assert!(answer.picks(1).is_empty());

    let received = api.received();
    assert_eq!(received[0].path, "/api/questions");
    assert_eq!(
        received[0].json(),
        Some(json!({ "questions": [{
            "question": "Which database?",
            "header": "Database",
            "options": [
                { "label": "SQLite", "description": "one file" },
                { "label": "Postgres" },
            ],
            "multiple": false,
        }] }))
    );
    assert_eq!(received[1].path, "/api/questions/12?wait=20000");
    assert_eq!(received[2].path, "/api/questions/12?wait=20000");
}

#[test]
fn asks_for_no_more_than_the_wait_that_is_left_and_gives_up_after_it() {
    let api = scripted(vec![
        reply(201, json!({ "message": { "id": 4 } })),
        reply(200, json!({ "answer": null })).held(Duration::from_millis(400)),
    ]);
    let board = Board::new(Some(&api.url), "tok");
    let gone = ask_the_board_until(
        &board,
        &one_question(),
        Duration::from_millis(300),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(gone, None);
    let received = api.received();
    let wait: u64 = received[1]
        .path
        .rsplit('=')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(wait <= 300, "asked to wait {wait} ms of 300");
}

#[test]
fn a_raised_stop_ends_the_wait_once_the_poll_in_hand_returns() {
    let api = scripted(vec![
        reply(201, json!({ "message": { "id": 4 } })),
        reply(200, json!({ "answer": null })).held(Duration::from_millis(300)),
        // Never asked for: the stop was up by then.
        reply(
            200,
            json!({ "answer": { "id": 5, "choices": [["SQLite"]] } }),
        ),
    ]);
    let board = Board::new(Some(&api.url), "tok");
    let stop = AtomicBool::new(false);
    let asked = std::thread::scope(|scope| {
        let asking = scope.spawn(|| ask_the_board_until(&board, &one_question(), DOOR_WAIT, &stop));
        // Up while the first poll is held at the board.
        while api.received().len() < 2 {
            std::thread::sleep(Duration::from_millis(5));
        }
        stop.store(true, Ordering::Relaxed);
        asking.join().unwrap()
    });
    assert_eq!(asked.unwrap(), None);
    assert_eq!(api.received().len(), 2, "no poll after the stop");
}

#[test]
fn a_stop_up_before_the_question_is_asked_posts_nothing() {
    let api = scripted(vec![reply(201, json!({ "message": { "id": 4 } }))]);
    let board = Board::new(Some(&api.url), "tok");
    let asked = ask_the_board_until(&board, &one_question(), DOOR_WAIT, &AtomicBool::new(true));
    assert_eq!(asked.unwrap(), None);
    assert!(api.received().is_empty());
}

const QUICK: [Duration; 4] = [Duration::from_millis(1); 4];

#[test]
fn a_poll_whose_reply_was_lost_is_asked_again_and_gets_the_answer_the_board_claimed_for_it() {
    // The board took the poll, claimed the answer for it, and the reply
    // never got back: asked again, it gives the same answer.
    let api = scripted(vec![
        reply(201, json!({ "message": { "id": 12 } })),
        hang_up(),
        reply(
            200,
            json!({ "question": {}, "answer": { "id": 13, "choices": [["SQLite"]] } }),
        ),
    ]);
    let board = Board::new(Some(&api.url), "tok");
    let answer = ask_retrying(
        &board,
        &one_question(),
        DOOR_WAIT,
        &AtomicBool::new(false),
        &QUICK,
    )
    .unwrap()
    .unwrap();
    assert_eq!(answer.id(), Some(13));
    let paths: Vec<_> = api
        .received()
        .into_iter()
        .map(|r| (r.method, r.path))
        .collect();
    assert_eq!(
        paths,
        [
            ("POST".to_owned(), "/api/questions".to_owned()),
            ("GET".to_owned(), "/api/questions/12?wait=20000".to_owned()),
            ("GET".to_owned(), "/api/questions/12?wait=20000".to_owned()),
        ],
        "the question was put once and the poll twice"
    );
}

#[test]
fn a_board_that_stays_out_of_reach_is_asked_four_times_more_and_then_the_dialog_takes_over() {
    let mut replies = vec![reply(201, json!({ "message": { "id": 12 } }))];
    replies.extend((0..5).map(|_| hang_up()));
    // A sixth would be an answer: it must not be asked for.
    replies.push(reply(
        200,
        json!({ "question": {}, "answer": { "id": 13, "choices": [["SQLite"]] } }),
    ));
    let api = scripted(replies);
    let board = Board::new(Some(&api.url), "tok");
    let stop = AtomicBool::new(false);
    let asked = ask_retrying(&board, &one_question(), DOOR_WAIT, &stop, &QUICK);
    // What `ask` makes of it is the harness's own dialog.
    assert!(asked.unwrap_err().is_unreachable());
    assert_eq!(api.received().len(), 6, "the question, and five polls");
}

#[test]
fn polls_that_fail_once_each_between_answers_never_add_up_to_the_end_of_the_door() {
    // Five failures in all, none more than one in a row: each is forgiven.
    let mut replies = vec![reply(201, json!({ "message": { "id": 12 } }))];
    for _ in 0..5 {
        replies.push(hang_up());
        replies.push(reply(200, json!({ "question": {}, "answer": null })));
    }
    replies.push(reply(
        200,
        json!({ "question": {}, "answer": { "id": 13, "choices": [["SQLite"]] } }),
    ));
    let api = scripted(replies);
    let board = Board::new(Some(&api.url), "tok");
    let answer = ask_retrying(
        &board,
        &one_question(),
        DOOR_WAIT,
        &AtomicBool::new(false),
        &QUICK,
    )
    .unwrap();
    assert_eq!(answer.and_then(|answer| answer.id()), Some(13));
}

#[test]
fn the_question_is_put_once_whatever_comes_of_it_and_a_refusal_or_an_unreadable_answer_is_not_asked_again(
) {
    let api = scripted(vec![
        hang_up(),
        reply(201, json!({ "message": { "id": 12 } })),
    ]);
    let board = Board::new(Some(&api.url), "tok");
    let lost = ask_retrying(
        &board,
        &one_question(),
        DOOR_WAIT,
        &AtomicBool::new(false),
        &QUICK,
    );
    assert!(lost.unwrap_err().is_unreachable());
    assert_eq!(api.received().len(), 1, "no second question");

    let api = scripted(vec![
        reply(201, json!({ "message": { "id": 12 } })),
        reply(409, json!({ "error": "door-closed", "message": "shut" })),
        reply(200, json!({ "question": {}, "answer": null })),
    ]);
    let board = Board::new(Some(&api.url), "tok");
    let refused = ask_retrying(
        &board,
        &one_question(),
        DOOR_WAIT,
        &AtomicBool::new(false),
        &QUICK,
    );
    assert!(refused.unwrap_err().is_refusal());
    assert_eq!(api.received().len(), 2, "the refusal was final");
}

#[test]
fn a_refused_question_is_a_refusal_whose_reason_points_the_model_to_cf_ask() {
    let api = scripted(vec![reply(409, json!({ "message": "T-3 was cancelled" }))]);
    let board = Board::new(Some(&api.url), "tok");
    let refused = ask_the_board_until(&board, &one_question(), DOOR_WAIT, &AtomicBool::new(false))
        .unwrap_err();
    assert!(refused.is_refusal());
    assert_eq!(
        refusal_reason(&refused),
        "ConsensFlow could not put this question to the chief (T-3 was cancelled). Ask with cf ask \"…\" instead."
    );
}

#[test]
fn a_door_the_board_shut_is_refused_in_the_boards_own_words_and_any_other_refusal_is_wrapped() {
    let closed = "T-1 was stopped, so m-5 is not answered here: its answer comes to you as a message when the task goes on. Do not ask it again; end your turn now.";
    let api = scripted(vec![
        reply(201, json!({ "message": { "id": 5 } })),
        reply(409, json!({ "error": "door-closed", "message": closed })),
        reply(
            403,
            json!({ "error": "ask-in-your-terminal", "message": "ask the human in your terminal" }),
        ),
        reply(201, json!({ "message": { "id": 6 } })),
        reply(
            403,
            json!({ "error": "not-your-question", "message": "m-6 was asked by someone else" }),
        ),
    ]);
    let board = Board::new(Some(&api.url), "tok");
    let stop = AtomicBool::new(false);
    let asked = |board: &Board| ask(board, &one_question(), DOOR_WAIT, &stop);
    assert_eq!(asked(&board), Reply::Refused(closed.to_owned()));
    assert_eq!(
        asked(&board),
        Reply::Refused(
            "ConsensFlow could not put this question to the chief (ask the human in your terminal). Ask with cf ask \"…\" instead."
                .to_owned()
        ),
        "refused when it was put"
    );
    assert_eq!(
        asked(&board),
        Reply::Refused(
            "ConsensFlow could not put this question to the chief (m-6 was asked by someone else). Ask with cf ask \"…\" instead."
                .to_owned()
        ),
        "refused at the poll, for another reason than a shut door"
    );
}

fn answer_numbered(id: Option<i64>) -> Answer {
    serde_json::from_value(json!({ "id": id, "choices": [["SQLite"]] })).unwrap()
}

#[test]
fn an_answer_handed_over_or_not_is_said_so_to_the_board_and_nothing_it_says_back_is_raised() {
    let api = scripted(vec![
        reply(200, json!({ "message": { "id": 13, "state": "read" } })),
        reply(200, json!({ "message": { "id": 13, "state": "queued" } })),
        reply(
            409,
            json!({ "error": "door-closed", "message": "m-13 is not answered here" }),
        ),
        // A daemon of Node's has no such route.
        reply(
            404,
            json!({ "error": "unknown-route", "message": "no such command: POST /api/answers/13/receipt" }),
        ),
    ]);
    let board = Board::new(Some(&api.url), "tok");
    let answer = answer_numbered(Some(13));
    for received in [true, false, true, true] {
        acknowledge(&board, &answer, received);
    }
    let said: Vec<_> = api
        .received()
        .into_iter()
        .map(|request| {
            let body = request.json();
            (request.method, request.path, body)
        })
        .collect();
    let receipt = |received: bool| {
        (
            "POST".to_owned(),
            "/api/answers/13/receipt".to_owned(),
            Some(json!({ "received": received })),
        )
    };
    assert_eq!(
        said,
        [receipt(true), receipt(false), receipt(true), receipt(true)]
    );
}

/// The receipt of answer 13 as the board is asked it.
fn receipt(received: bool) -> (String, String, Option<Value>) {
    (
        "POST".to_owned(),
        "/api/answers/13/receipt".to_owned(),
        Some(json!({ "received": received })),
    )
}

#[test]
fn a_receipt_whose_reply_was_lost_is_said_again_until_the_board_takes_it_whichever_it_says() {
    for received in [true, false] {
        let api = scripted(vec![
            hang_up(),
            hang_up(),
            reply(200, json!({ "message": { "id": 13, "state": "read" } })),
        ]);
        let board = Board::new(Some(&api.url), "tok");
        acknowledge_retrying(&board, &answer_numbered(Some(13)), received, &QUICK);
        let said: Vec<_> = api
            .received()
            .into_iter()
            .map(|request| (request.method.clone(), request.path.clone(), request.json()))
            .collect();
        assert_eq!(
            said,
            [receipt(received), receipt(received), receipt(received)],
            "received: {received}"
        );
    }
}

#[test]
fn a_board_that_stays_out_of_reach_is_told_five_times_and_nothing_is_raised() {
    let mut replies: Vec<_> = (0..5).map(|_| hang_up()).collect();
    // A sixth would take it: it must not be asked.
    replies.push(reply(
        200,
        json!({ "message": { "id": 13, "state": "read" } }),
    ));
    let api = scripted(replies);
    let board = Board::new(Some(&api.url), "tok");
    acknowledge_retrying(&board, &answer_numbered(Some(13)), true, &QUICK);
    assert_eq!(api.received().len(), 5, "the receipt, and four more");
}

#[test]
fn a_receipt_the_board_refused_is_not_said_again() {
    for refusal in [
        reply(
            409,
            json!({ "error": "door-closed", "message": "m-13 is not answered here" }),
        ),
        // A daemon of Node's has no such route.
        reply(
            404,
            json!({ "error": "unknown-route", "message": "no such command" }),
        ),
    ] {
        let api = scripted(vec![refusal, reply(200, json!({ "message": {} }))]);
        let board = Board::new(Some(&api.url), "tok");
        acknowledge_retrying(&board, &answer_numbered(Some(13)), true, &QUICK);
        assert_eq!(api.received().len(), 1, "the refusal was final");
    }
}

#[test]
fn an_answer_with_no_number_is_not_acknowledged_and_a_board_that_is_not_there_is_not_raised() {
    let api = scripted(vec![]);
    let board = Board::new(Some(&api.url), "tok");
    acknowledge(&board, &answer_numbered(None), true);
    assert!(api.received().is_empty(), "it has no number to say");
    acknowledge(&Board::new(None, "tok"), &answer_numbered(Some(13)), true);
}

#[test]
fn an_answer_it_cannot_read_is_no_refusal() {
    let api = scripted(vec![
        reply(201, json!({ "message": { "id": 4 } })),
        reply(200, json!({ "question": {} })),
    ]);
    let board = Board::new(Some(&api.url), "tok");
    let unread = ask_the_board_until(&board, &one_question(), DOOR_WAIT, &AtomicBool::new(false))
        .unwrap_err();
    assert!(!unread.is_refusal());
}

#[test]
fn a_window_has_a_board_when_it_has_a_url_and_a_token_even_an_empty_one() {
    let env = |vars: &[(&str, &str)]| Env::from_vars(vars.iter().copied());
    let url = ("CONSENSFLOW_URL", "http://127.0.0.1:9");
    assert!(board_of(&env(&[url, ("CONSENSFLOW_TOKEN", "window-token")])).is_some());
    assert!(board_of(&env(&[url, ("CONSENSFLOW_TOKEN", "")])).is_some());
    assert!(board_of(&env(&[url])).is_none(), "no token");
    assert!(board_of(&env(&[("CONSENSFLOW_TOKEN", "window-token")])).is_none());
    assert!(board_of(&env(&[("CONSENSFLOW_URL", ""), ("CONSENSFLOW_TOKEN", "t")])).is_none());
    assert!(board_of(&env(&[])).is_none());
}
