//! `cf hook <harness>`: what a harness's hooks run. A hook prints only what
//! its harness must read, nothing at all when it has nothing to say, and
//! exits 0 whatever happened: a failing hook would stop its harness.
//!
//! A question hook that was given its answer writes it, flushes what it
//! wrote, and only then tells the board it handed the answer over: the
//! other way round, a hook that died between the two would lose the answer,
//! and this order can at worst hand it over twice.

use std::cell::RefCell;
use std::io::{self, Read, Write};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use cf_base::env::Env;
use cf_base::js;
use cf_board::door::{self, DOOR_WAIT};
use cf_board::Board;
use cf_harness::{claude, devin};
use cf_proto::questions::{Answer, Question, Reply};

pub fn run(
    harness: Option<&str>,
    env: &Env,
    input: &mut dyn Read,
    out: &mut dyn Write,
) -> io::Result<u8> {
    let mut handed: Option<(Board, Answer)> = None;
    let said = match harness {
        Some(tool @ ("claude" | "devin")) => {
            // Outside a window there is no board to ask: nothing is read, nothing said.
            let Some(board) = door::board_of(env) else {
                return Ok(0);
            };
            let wait = question_wait(env);
            let given = RefCell::new(None);
            let ask = |questions: &[Question]| {
                let reply = door::ask(&board, questions, wait, &AtomicBool::new(false));
                if let Reply::Answered(answer) = &reply {
                    *given.borrow_mut() = Some(answer.clone());
                }
                reply
            };
            let said = if tool == "claude" {
                claude::question_hook(input, ask)
            } else {
                devin::question_hook(input, ask)
            };
            handed = given.take().map(|answer| (board, answer));
            said
        }
        Some("devin-session") => devin::session_hook(
            input,
            env.path("CF_DEVIN_ROLE_FILE"),
            env.path("CHISEL_PURE_ACP_WIRE_LOG"),
        ),
        _ => None,
    };
    if let Some(said) = said {
        write!(out, "{said}")?;
        out.flush()?;
    }
    // The answer is the harness's now: the board is told it was handed over.
    if let Some((board, answer)) = handed {
        door::acknowledge(&board, &answer, true);
    }
    Ok(0)
}

/// How long a question hook waits for the board's answer: the door's own
/// wait, or `CONSENSFLOW_QUESTION_WAIT_MS` when set, where what is no number
/// waits not at all.
fn question_wait(env: &Env) -> Duration {
    let Some(text) = env.text("CONSENSFLOW_QUESTION_WAIT_MS") else {
        return DOOR_WAIT;
    };
    let millis = js::number(text);
    if millis.is_nan() || millis <= 0.0 {
        return Duration::ZERO;
    }
    Duration::try_from_secs_f64(millis / 1000.0).unwrap_or(Duration::MAX)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use cf_board::scripted::{reply, scripted, ScriptedApi};
    use serde_json::{json, Value};

    use super::*;

    const EVENT: &str = r#"{"tool_name":"AskUserQuestion","tool_input":{"questions":[{"question":"Which colour?","header":"Colour","options":[{"label":"red"},{"label":"blue"}],"multiSelect":false}]}}"#;

    /// Where a hook writes, which notes how many requests the board had
    /// received when what was written got flushed.
    struct Watching<'a> {
        board: &'a ScriptedApi,
        written: Vec<u8>,
        flushed_at: Rc<Cell<Option<usize>>>,
    }

    impl<'a> Watching<'a> {
        fn new(board: &'a ScriptedApi) -> Self {
            Self {
                board,
                written: Vec::new(),
                flushed_at: Rc::default(),
            }
        }
    }

    impl Write for Watching<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.written.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushed_at.set(Some(self.board.received().len()));
            Ok(())
        }
    }

    /// `cf hook claude` in a window of this board, given the event.
    fn run_claude<'a>(board: &'a ScriptedApi, extra: &[(&str, &str)]) -> (Watching<'a>, u8) {
        let vars = [
            ("CONSENSFLOW_URL", board.url.as_str()),
            ("CONSENSFLOW_TOKEN", "tok"),
        ];
        let env = Env::from_vars(vars.into_iter().chain(extra.iter().copied()));
        let mut out = Watching::new(board);
        let status = run(Some("claude"), &env, &mut EVENT.as_bytes(), &mut out).unwrap();
        (out, status)
    }

    #[test]
    fn an_answer_is_written_and_flushed_before_the_board_is_told_it_was_handed_over() {
        let board = scripted(vec![
            reply(201, json!({ "message": { "id": 5 } })),
            reply(
                200,
                json!({ "question": {}, "answer": { "id": 6, "choices": [["red"]] } }),
            ),
            reply(200, json!({ "message": { "id": 6, "state": "read" } })),
        ]);
        let (out, status) = run_claude(&board, &[]);
        assert_eq!(status, 0);
        assert_eq!(
            out.flushed_at.get(),
            Some(2),
            "the question and its poll were the board's only requests when it was flushed"
        );
        let said: Value = serde_json::from_slice(&out.written).unwrap();
        assert_eq!(said["hookSpecificOutput"]["permissionDecision"], "allow");
        assert_eq!(
            said["hookSpecificOutput"]["updatedInput"]["answers"]["Which colour?"],
            "red"
        );
        let received = board.received();
        assert_eq!(received.len(), 3);
        assert_eq!(
            (received[2].method.as_str(), received[2].path.as_str()),
            ("POST", "/api/answers/6/receipt")
        );
        assert_eq!(received[2].json(), Some(json!({ "received": true })));
    }

    #[test]
    fn a_door_the_board_shut_is_handed_to_the_model_as_it_is_and_nothing_is_acknowledged() {
        let closed = "T-1 was stopped, so m-5 is not answered here: its answer comes to you as a message when the task goes on. Do not ask it again; end your turn now.";
        let board = scripted(vec![
            reply(201, json!({ "message": { "id": 5 } })),
            reply(409, json!({ "error": "door-closed", "message": closed })),
        ]);
        let (out, status) = run_claude(&board, &[]);
        assert_eq!(status, 0);
        let said: Value = serde_json::from_slice(&out.written).unwrap();
        assert_eq!(said["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            said["hookSpecificOutput"]["permissionDecisionReason"],
            closed
        );
        assert_eq!(
            board.received().len(),
            2,
            "no receipt for an answer not handed over"
        );
    }

    #[test]
    fn a_question_nobody_answered_in_time_says_nothing_and_acknowledges_nothing() {
        let board = scripted(vec![reply(201, json!({ "message": { "id": 5 } }))]);
        let (out, status) = run_claude(&board, &[("CONSENSFLOW_QUESTION_WAIT_MS", "0")]);
        assert_eq!(status, 0);
        assert!(out.written.is_empty());
        assert_eq!(out.flushed_at.get(), None);
        assert_eq!(board.received().len(), 1, "only the question was put");
    }
}
