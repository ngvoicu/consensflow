//! Codex's question tool, answered from the board. The app-server asks its
//! client (`item/tool/requestUserInput`); the broker is the client, so it
//! asks the board instead and answers the app-server with what the board
//! said. When nobody answers in time, or there is no board, the request goes
//! on to the TUI and its own dialog takes over. A question the board refuses
//! is answered with the reason: nobody watches a member's window, so its own
//! dialog would hold the task for good.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use cf_base::env::Env;
use cf_base::js;
use cf_base::json::js_order;
use cf_board::door::{ask_the_board_until, refusal_reason};
use cf_board::Board;
use cf_proto::questions::{Answer, Question};
use serde_json::{json, Map, Value};

/// The method of Codex's request.
pub(crate) const REQUEST_USER_INPUT: &str = "item/tool/requestUserInput";

/// The board this window's questions go to: the daemon at `CONSENSFLOW_URL`,
/// as the participant `CONSENSFLOW_TOKEN` names. None outside a window, which
/// has no URL or no token. A token that is set but empty is a token still: the
/// board refuses it, and the question is answered with the reason, as the
/// door has always done, not left to a dialog nobody watches.
pub(crate) fn board_of(env: &Env) -> Option<Arc<Board>> {
    let url = env.text("CONSENSFLOW_URL")?;
    let token = env.os("CONSENSFLOW_TOKEN")?.to_str()?;
    Some(Arc::new(Board::new(Some(url), token)))
}

/// The questions of one request, in the board's shape, with the id each is
/// answered under.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Asked {
    questions: Vec<Question>,
    keys: Vec<String>,
}

/// What the board made of the questions.
#[derive(Debug, PartialEq)]
pub(crate) enum Outcome {
    Answered(Answer),
    /// The board refused them, for this reason.
    Refused(String),
    /// Nobody answered in time, or the board could not be reached, or the wait was cancelled.
    Unanswered,
}

impl Asked {
    /// The questions in a request's `params`; none when they are not what the
    /// door can read, and the request goes on to the TUI as it came.
    pub(crate) fn read(params: Option<&Value>) -> Option<Self> {
        let list = match params.and_then(|params| params.get("questions")) {
            None | Some(Value::Null) => &[][..],
            Some(Value::Array(list)) => list.as_slice(),
            Some(_) => return None,
        };
        let questions = list
            .iter()
            .map(|question| Question::read(question, false))
            .collect::<Option<Vec<_>>>()?;
        let keys = list
            .iter()
            .map(|question| js::text(question.get("id")).into_owned())
            .collect();
        Some(Self { questions, keys })
    }

    /// The response to Codex's request `id` that says what the chief picked.
    pub(crate) fn answered(&self, id: &Value, answer: &Answer) -> Value {
        response(id, self.answers(|at| json!(answer.picks(at))))
    }

    /// The response to Codex's request `id` that gives the board's `reason`
    /// as the answer to every question.
    pub(crate) fn refused(&self, id: &Value, reason: &str) -> Value {
        response(id, self.answers(|_| json!([reason])))
    }

    /// An answer per question id, as `Object.fromEntries` made them: a
    /// question whose id an earlier one had takes that one's place.
    fn answers(&self, labels: impl Fn(usize) -> Value) -> Map<String, Value> {
        self.keys
            .iter()
            .enumerate()
            .map(|(at, key)| (key.clone(), json!({ "answers": labels(at) })))
            .collect()
    }

    /// Puts the questions on the board and waits up to `wait` for the answer,
    /// or until `stop` is raised. Blocks: the caller runs it where blocking is
    /// fine, and everything else goes on meanwhile.
    pub(crate) fn ask(&self, board: &Board, wait: Duration, stop: &AtomicBool) -> Outcome {
        match ask_the_board_until(board, &self.questions, wait, stop) {
            Ok(Some(answer)) => Outcome::Answered(answer),
            Err(cause) if cause.is_refusal() => Outcome::Refused(refusal_reason(&cause)),
            Ok(None) | Err(_) => Outcome::Unanswered,
        }
    }
}

fn response(id: &Value, answers: Map<String, Value>) -> Value {
    js_order(json!({ "id": id, "result": { "answers": answers } }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    use cf_board::scripted::{reply, scripted};

    fn request() -> Value {
        json!({
            "id": "ask-1",
            "method": REQUEST_USER_INPUT,
            "params": {
                "threadId": "t",
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

    fn asked(request: &Value) -> Asked {
        Asked::read(request.get("params")).unwrap()
    }

    #[test]
    fn puts_the_question_on_the_board_in_the_boards_shape() {
        let asked = asked(&request());
        assert_eq!(
            serde_json::to_value(&asked.questions).unwrap(),
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
    }

    #[test]
    fn answers_each_question_under_its_id_with_the_labels_the_chief_picked() {
        let params = json!({ "questions": [
            { "id": "colour", "question": "Which?" },
            { "id": "size", "question": "How big?" },
            { "id": "none", "question": "Why?" },
        ] });
        let asked = Asked::read(Some(&params)).unwrap();
        let answer: Answer =
            serde_json::from_value(json!({ "id": 70, "choices": [["blue"], ["s", "m"]] })).unwrap();
        assert_eq!(
            asked.answered(&json!("ask-1"), &answer).to_string(),
            r#"{"id":"ask-1","result":{"answers":{"colour":{"answers":["blue"]},"size":{"answers":["s","m"]},"none":{"answers":[]}}}}"#
        );
    }

    #[test]
    fn answers_a_refused_question_with_the_reason_under_every_id() {
        let params = json!({ "questions": [{ "id": "a" }, { "id": "b" }] });
        let asked = Asked::read(Some(&params)).unwrap();
        assert_eq!(
            asked.refused(&json!(7), "no way").to_string(),
            r#"{"id":7,"result":{"answers":{"a":{"answers":["no way"]},"b":{"answers":["no way"]}}}}"#
        );
    }

    #[test]
    fn a_question_id_is_written_as_javascript_wrote_it_as_a_key() {
        let params = json!({ "questions": [
            { "question": "no id" },
            { "id": 2 },
            { "id": "dup" },
            { "id": 1 },
            { "id": "dup" },
            "plain text",
        ] });
        let asked = Asked::read(Some(&params)).unwrap();
        let answer: Answer = serde_json::from_value(
            json!({ "choices": [["a"], ["b"], ["c"], ["d"], ["e"], ["f"]] }),
        )
        .unwrap();
        // Index-like keys first and in order; a repeated id keeps its first
        // place and takes the later answer; what has no id is "undefined".
        assert_eq!(
            asked.answered(&json!(1), &answer)["result"]["answers"].to_string(),
            r#"{"1":{"answers":["d"]},"2":{"answers":["b"]},"undefined":{"answers":["f"]},"dup":{"answers":["e"]}}"#
        );
    }

    #[test]
    fn no_questions_at_all_is_an_empty_list_for_the_board_to_refuse() {
        for params in [json!({}), json!({ "questions": null })] {
            let asked = Asked::read(Some(&params)).unwrap();
            assert!(asked.questions.is_empty() && asked.keys.is_empty());
        }
        assert!(Asked::read(None).is_some());
    }

    #[test]
    fn a_request_the_door_cannot_read_goes_on_to_the_tui() {
        for params in [
            json!({ "questions": {} }),
            json!({ "questions": "which?" }),
            json!({ "questions": [null] }),
            json!({ "questions": [{ "options": "red" }] }),
            json!({ "questions": [{ "options": [null] }] }),
        ] {
            assert_eq!(Asked::read(Some(&params)), None, "{params}");
        }
    }

    #[test]
    fn asks_the_board_and_gives_what_it_answered() {
        let api = scripted(vec![
            reply(201, json!({ "message": { "id": 61 } })),
            reply(
                200,
                json!({ "question": {}, "answer": { "id": 70, "choices": [["blue"]] } }),
            ),
        ]);
        let board = Board::new(Some(&api.url), "window-token");
        let outcome =
            asked(&request()).ask(&board, Duration::from_secs(5), &AtomicBool::new(false));
        let Outcome::Answered(answer) = outcome else {
            panic!("{outcome:?}");
        };
        assert_eq!(answer.picks(0), ["blue".to_string()]);
        assert_eq!(
            api.received()[0].authorization.as_deref(),
            Some("Bearer window-token")
        );
    }

    #[test]
    fn a_board_that_refuses_gives_its_reason() {
        let api = scripted(vec![reply(
            400,
            json!({ "error": "bad-questions", "message": "questions: one to 4 questions" }),
        )]);
        let board = Board::new(Some(&api.url), "window-token");
        let outcome =
            asked(&request()).ask(&board, Duration::from_secs(5), &AtomicBool::new(false));
        assert_eq!(
            outcome,
            Outcome::Refused(
                "ConsensFlow could not put this question to the chief (questions: one to 4 questions). Ask with cf ask \"…\" instead."
                    .into()
            )
        );
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

    #[test]
    fn a_window_whose_token_is_empty_has_its_questions_refused_by_the_board_with_the_reason() {
        let api = scripted(vec![reply(
            401,
            json!({ "error": "unauthorized", "message": "unknown participant" }),
        )]);
        let env = Env::from_vars([
            ("CONSENSFLOW_URL", api.url.as_str()),
            ("CONSENSFLOW_TOKEN", ""),
        ]);
        let board = board_of(&env).unwrap();
        let outcome =
            asked(&request()).ask(&board, Duration::from_secs(5), &AtomicBool::new(false));
        assert!(
            matches!(&outcome, Outcome::Refused(reason) if reason.contains("unknown participant")),
            "{outcome:?}"
        );
        assert_eq!(api.received().len(), 1);
    }

    #[test]
    fn a_board_that_is_not_there_or_does_not_answer_in_time_gives_no_answer() {
        let gone = Board::new(Some("http://127.0.0.1:9"), "t");
        let quick = AtomicBool::new(false);
        assert_eq!(
            asked(&request()).ask(&gone, Duration::from_secs(5), &quick),
            Outcome::Unanswered
        );

        let api = scripted(vec![
            reply(201, json!({ "message": { "id": 61 } })),
            reply(200, json!({ "answer": null })),
        ]);
        let board = Board::new(Some(&api.url), "t");
        assert_eq!(
            asked(&request()).ask(&board, Duration::from_millis(50), &quick),
            Outcome::Unanswered
        );
        quick.store(true, Ordering::Relaxed);
    }
}
