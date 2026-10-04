//! The door a member's harness question tool opens onto the board: a hook
//! posts the questions as the window's participant, waits for the chief to
//! answer on the board, and hands the answer back into the tool call, so
//! nothing is typed into the window. When the answer does not come in time,
//! the door gives up and the harness's own dialog takes over.
//!
//! The Pi and OpenCode extensions keep their own copy,
//! `hosts/lib/question-door.js`: each harness loads it into its own runtime.

use std::time::{Duration, Instant};

use cf_proto::questions::{Answer, Question};
use serde_json::{json, Value};

use crate::{Board, BoardError, Method};

/// How long a door waits for the board before the harness's own dialog takes over.
pub const DOOR_WAIT: Duration = Duration::from_millis(3_500_000);
/// One request's share of that wait; the API holds a request 25 seconds at most.
const POLL_WAIT: Duration = Duration::from_secs(20);

/// What a member's window tells its model when the board refuses its
/// question: nobody watches a member's window, so its own dialog would hold
/// the task for good.
pub fn refusal_reason(cause: &BoardError) -> String {
    format!(
        "ConsensFlow could not put this question to the chief ({cause}). Ask with cf ask \"…\" instead."
    )
}

/// Puts `questions` on the board and waits up to `wait` for their answer:
/// `None` when the wait ran out first. A wait too long to reach never runs out.
pub fn ask_the_board(
    board: &Board,
    questions: &[Question],
    wait: Duration,
) -> Result<Option<Answer>, BoardError> {
    let path = "/api/questions";
    let posted = board.call(Method::Post, path, Some(&json!({ "questions": questions })))?;
    let id = posted
        .pointer("/message/id")
        .and_then(Value::as_u64)
        .ok_or(BoardError::Malformed {
            path: path.to_string(),
            what: "message id",
        })?;
    let until = Instant::now().checked_add(wait);
    loop {
        let left = until.map_or(POLL_WAIT, |until| {
            until.saturating_duration_since(Instant::now())
        });
        if left.is_zero() {
            return Ok(None);
        }
        let path = format!(
            "/api/questions/{id}?wait={}",
            left.min(POLL_WAIT).as_millis()
        );
        let polled = board.call(Method::Get, &path, None)?;
        match polled.get("answer") {
            Some(Value::Null) => {}
            Some(answer) => {
                return serde_json::from_value(answer.clone())
                    .map(Some)
                    .map_err(|_| BoardError::Malformed {
                        path,
                        what: "answer",
                    });
            }
            None => {
                return Err(BoardError::Malformed {
                    path,
                    what: "answer",
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripted::{reply, scripted};
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
        let answer = ask_the_board(&board, &one_question(), DOOR_WAIT)
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
        let gone = ask_the_board(&board, &one_question(), Duration::from_millis(300)).unwrap();
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
    fn a_refused_question_is_a_refusal_whose_reason_points_the_model_to_cf_ask() {
        let api = scripted(vec![reply(409, json!({ "message": "T-3 was cancelled" }))]);
        let board = Board::new(Some(&api.url), "tok");
        let refused = ask_the_board(&board, &one_question(), DOOR_WAIT).unwrap_err();
        assert!(refused.is_refusal());
        assert_eq!(
            refusal_reason(&refused),
            "ConsensFlow could not put this question to the chief (T-3 was cancelled). Ask with cf ask \"…\" instead."
        );
    }

    #[test]
    fn an_answer_it_cannot_read_is_no_refusal() {
        let api = scripted(vec![
            reply(201, json!({ "message": { "id": 4 } })),
            reply(200, json!({ "question": {} })),
        ]);
        let board = Board::new(Some(&api.url), "tok");
        let unread = ask_the_board(&board, &one_question(), DOOR_WAIT).unwrap_err();
        assert!(!unread.is_refusal());
    }
}
