//! Codex's question tool, answered from the board. The app-server asks its
//! client (`item/tool/requestUserInput`); the broker is the client, so it
//! asks the board instead and answers the app-server with what the board
//! said. When nobody answers in time, or there is no board, the request goes
//! on to the TUI and its own dialog takes over. A question the board refuses
//! is answered with the reason: nobody watches a member's window, so its own
//! dialog would hold the task for good.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use cf_base::js;
use cf_base::json::js_order;
use cf_board::door::{ask, Asking};
use cf_board::Board;
use cf_proto::questions::{Answer, Question, Reply};
use serde_json::{json, Map, Value};

/// The method of Codex's request.
pub(crate) const REQUEST_USER_INPUT: &str = "item/tool/requestUserInput";

/// The questions of one request, in the board's shape, with the id each is
/// answered under.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Asked {
    questions: Vec<Question>,
    keys: Vec<String>,
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
    pub(crate) fn ask(
        &self,
        board: &Board,
        asking: &Asking,
        wait: Duration,
        stop: &AtomicBool,
    ) -> Reply {
        ask(board, &self.questions, asking, wait, stop)
    }

    /// What names Codex's request across a reconnect of its pair, where its RPC
    /// id changes: the thread, turn and item it names. A request that names no
    /// item is known by the id it came under and its questions.
    pub(crate) fn key(&self, id: &Value, params: Option<&Value>) -> RequestKey {
        let named = |field: &str| {
            params
                .and_then(|params| params.get(field))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        let item = named("itemId");
        let anonymous = item.is_none().then(|| {
            format!(
                "{id}\u{0}{}",
                serde_json::to_string(&self.questions).unwrap_or_default()
            )
        });
        RequestKey {
            thread: named("threadId"),
            turn: named("turnId"),
            item,
            anonymous,
        }
    }
}

/// Which native request a question is: the same one, replayed after its pair's
/// reconnect, has the same key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestKey {
    thread: Option<String>,
    turn: Option<String>,
    item: Option<String>,
    anonymous: Option<String>,
}

/// The requests the broker has put to the board and not settled, for its whole
/// lifetime: each keeps when it was first received and the question it made
/// there, so a replay of it after a reconnect is the same question, asked at the
/// same moment. A broker that restarts loses them.
///
/// One pair at a time holds a request: the one that received it last. The board's
/// claim on its answer is that pair's, and a retired pair that was first to hear
/// the answer leaves it with the entry for the pair that holds the request now.
#[derive(Default)]
pub(crate) struct Stamps {
    held: Vec<Entry>,
    holds: u64,
    /// Requests a pair settled, with the hold that settled them: a retired hold
    /// that hears of its answer afterwards has nothing to give back.
    settled: Vec<(RequestKey, u64)>,
}

struct Entry {
    key: RequestKey,
    /// The RPC id the request has now.
    id: Value,
    asking: Asking,
    /// Which hold of the request is the live one.
    owner: u64,
    /// An answer a retired pair's poll was given and did not settle.
    claimed: Option<Answer>,
}

/// A pair's hold of a request.
pub(crate) struct Hold {
    pub(crate) asking: Asking,
    pub(crate) owner: u64,
}

impl Hold {
    /// When the request was first received.
    #[cfg(test)]
    pub(crate) fn asked_at(&self) -> i64 {
        self.asking.asked_at()
    }
}

/// The most requests it holds: the oldest goes first.
const MOST_HELD: usize = 64;

impl Stamps {
    /// The hold of the request `key` that a pair takes now: its asking began at the
    /// request's first receipt, and it replaces any earlier hold of it; `id` is the RPC
    /// id it came under this time.
    pub(crate) fn of(&mut self, key: RequestKey, id: &Value) -> Hold {
        self.holds += 1;
        let owner = self.holds;
        if let Some(entry) = self.held.iter_mut().find(|entry| entry.key == key) {
            entry.id = id.clone();
            entry.owner = owner;
            return Hold {
                asking: entry.asking.clone(),
                owner,
            };
        }
        if self.held.len() >= MOST_HELD {
            self.held.remove(0);
        }
        let asking = Asking::now();
        self.held.push(Entry {
            key,
            id: id.clone(),
            asking: asking.clone(),
            owner,
            claimed: None,
        });
        Hold { asking, owner }
    }

    /// A later hold of the request `key` has replaced `owner`'s.
    pub(crate) fn superseded(&self, key: &RequestKey, owner: u64) -> bool {
        match self.held.iter().find(|entry| entry.key == *key) {
            Some(entry) => entry.owner != owner,
            None => self
                .settled
                .iter()
                .any(|(settled, by)| settled == key && *by != owner),
        }
    }

    /// A retired pair's poll was given `answer` after a replacement took the request:
    /// the replacement settles it.
    pub(crate) fn leave_claim(&mut self, key: &RequestKey, answer: Answer) {
        if let Some(entry) = self.held.iter_mut().find(|entry| entry.key == *key) {
            entry.claimed = Some(answer);
        }
    }

    /// The answer a retired poll left for the live hold of `key`, if it is taken.
    pub(crate) fn take_claim(&mut self, key: &RequestKey) -> Option<Answer> {
        self.held
            .iter_mut()
            .find(|entry| entry.key == *key)
            .and_then(|entry| entry.claimed.take())
    }

    /// The request `key` is settled: Codex has its answer. The answer a retired
    /// poll left, if one, is given back.
    pub(crate) fn settle(&mut self, key: &RequestKey, owner: u64) -> Option<Answer> {
        let left = self.take_claim(key);
        self.held.retain(|entry| entry.key != *key);
        self.settled.retain(|(settled, _)| settled != key);
        if self.settled.len() >= MOST_HELD {
            self.settled.remove(0);
        }
        self.settled.push((key.clone(), owner));
        left
    }

    /// The turn on `thread` is over: its requests are forgotten, and the answers
    /// retired polls left for them are given back.
    pub(crate) fn end_turn_of(&mut self, thread: &str) -> Vec<Answer> {
        self.forget(|entry| entry.key.thread.as_deref() == Some(thread))
    }

    /// Codex says the request it knows as `id` (on `thread`, where it says) is resolved.
    pub(crate) fn resolve(&mut self, id: &Value, thread: Option<&str>) -> Vec<Answer> {
        self.forget(|entry| {
            entry.id == *id
                && thread.is_none_or(|thread| entry.key.thread.as_deref() == Some(thread))
        })
    }

    fn forget(&mut self, gone: impl Fn(&Entry) -> bool) -> Vec<Answer> {
        let mut left = Vec::new();
        self.held.retain_mut(|entry| {
            if gone(entry) {
                left.extend(entry.claimed.take());
                false
            } else {
                true
            }
        });
        left
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
    fn a_request_replayed_under_another_rpc_id_has_the_stamp_of_its_first_receipt() {
        let mut first = request();
        first["params"]["turnId"] = json!("turn-1");
        first["params"]["itemId"] = json!("item-1");
        let mut replay = first.clone();
        replay["id"] = json!("ask-9");
        let mut stamps = Stamps::default();
        let key = |request: &Value| asked(request).key(&request["id"], request.get("params"));
        let begun = stamps.of(key(&first), &first["id"]);
        std::thread::sleep(Duration::from_millis(5));
        let again = stamps.of(key(&replay), &replay["id"]);
        assert_eq!(begun.asked_at(), again.asked_at());
        // Another item of the same id is another request, asked afresh.
        let mut other = first.clone();
        other["params"]["itemId"] = json!("item-2");
        let apart = stamps.of(key(&other), &other["id"]);
        assert!(apart.asked_at() > begun.asked_at());
        // A request settled is forgotten; its replay would be new.
        stamps.settle(&key(&first), 0);
        std::thread::sleep(Duration::from_millis(5));
        assert!(stamps.of(key(&first), &first["id"]).asked_at() > apart.asked_at());
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
        let outcome = asked(&request()).ask(
            &board,
            &Asking::now(),
            Duration::from_secs(5),
            &AtomicBool::new(false),
        );
        let Reply::Answered(answer) = outcome else {
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
        let outcome = asked(&request()).ask(
            &board,
            &Asking::now(),
            Duration::from_secs(5),
            &AtomicBool::new(false),
        );
        assert_eq!(
            outcome,
            Reply::Refused(
                "ConsensFlow could not put this question to the chief (questions: one to 4 questions). Ask with cf ask \"…\" instead."
                    .into()
            )
        );
    }

    #[test]
    fn a_window_whose_token_is_empty_has_its_questions_refused_by_the_board_with_the_reason() {
        let api = scripted(vec![reply(
            401,
            json!({ "error": "unauthorized", "message": "unknown participant" }),
        )]);
        let env = cf_base::env::Env::from_vars([
            ("CONSENSFLOW_URL", api.url.as_str()),
            ("CONSENSFLOW_TOKEN", ""),
        ]);
        let board = cf_board::door::board_of(&env).unwrap();
        let outcome = asked(&request()).ask(
            &board,
            &Asking::now(),
            Duration::from_secs(5),
            &AtomicBool::new(false),
        );
        assert!(
            matches!(&outcome, Reply::Refused(reason) if reason.contains("unknown participant")),
            "{outcome:?}"
        );
        assert_eq!(api.received().len(), 1);
    }

    #[test]
    fn a_board_that_is_not_there_or_does_not_answer_in_time_gives_no_answer() {
        let gone = Board::new(Some("http://127.0.0.1:9"), "t");
        let quick = AtomicBool::new(false);
        assert_eq!(
            asked(&request()).ask(&gone, &Asking::now(), Duration::from_secs(5), &quick),
            Reply::Unanswered
        );

        let api = scripted(vec![
            reply(201, json!({ "message": { "id": 61 } })),
            reply(200, json!({ "answer": null })),
        ]);
        let board = Board::new(Some(&api.url), "t");
        assert_eq!(
            asked(&request()).ask(&board, &Asking::now(), Duration::from_millis(50), &quick),
            Reply::Unanswered
        );
        quick.store(true, Ordering::Relaxed);
    }
}
