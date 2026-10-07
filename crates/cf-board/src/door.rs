//! The door a member's harness question tool opens onto the board: a hook
//! posts the questions as the window's participant, waits for the chief to
//! answer on the board, and hands the answer back into the tool call, so
//! nothing is typed into the window. When the answer does not come in time,
//! the door gives up and the harness's own dialog takes over.
//!
//! The question hooks of `cf hook` and the Codex window's broker (`cf
//! codex-session`) ask through [`ask`], where a closed window ends the wait.
//! An answer is claimed for the door when it polls for it, and is the door's
//! to hand to its harness: once it has, it says so ([`acknowledge`]), which
//! is what makes the answer received. A poll that gets no answer (its reply
//! was lost on its way back) is asked again a few times, and the board gives
//! the same claimed answer to the same question: the harness's own dialog,
//! which nobody watches in a member's window, takes over only from a board
//! that stays out of reach. The receipt is said again as a poll is: an answer
//! handed over and never received would be pasted a second time. A door that
//! was shut by a pause is refused, and what the board says it is passed on
//! as it is. OpenCode's extension keeps
//! its own copy, `hosts/lib/question-door.js`: the harness loads it into its
//! own runtime.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use cf_base::env::Env;
use cf_proto::questions::{Answer, Question, Reply};
use serde_json::{json, Value};

use crate::{Board, BoardError};

/// The board a window's questions go to: the daemon at `CONSENSFLOW_URL`,
/// as the participant `CONSENSFLOW_TOKEN` names. None outside a window, which
/// has no URL or no token. A token that is set but empty is a token still, as
/// the door has always taken one: the board refuses it, and the question is
/// answered with the reason, not left to a dialog nobody watches. (`cf`'s
/// commands take a token only when it says something: `Board::from_env`.)
pub fn board_of(env: &Env) -> Option<Board> {
    let url = env.text("CONSENSFLOW_URL")?;
    let token = env.os("CONSENSFLOW_TOKEN")?;
    Some(Board::new(Some(url), &token.to_string_lossy()))
}

/// Puts `questions` on the board and waits up to `wait` for the chief's
/// answer, or until `stop` is raised: what came of it. A door the board
/// shut (the task was stopped) is told so in the board's own words, which
/// the window hands to its model as they are; any other refusal is wrapped in
/// what points the model to `cf ask`.
pub fn ask(board: &Board, questions: &[Question], wait: Duration, stop: &AtomicBool) -> Reply {
    match ask_the_board_until(board, questions, wait, stop) {
        Ok(Some(answer)) => Reply::Answered(answer),
        Err(cause) if cause.code() == Some(DOOR_CLOSED) => Reply::Refused(cause.to_string()),
        Err(cause) if cause.is_refusal() => Reply::Refused(refusal_reason(&cause)),
        Ok(None) | Err(_) => Reply::Unanswered,
    }
}

/// The door has handed `answer` to its harness (or could not): the board is
/// told, as the door that claimed it, and an answer handed over is received.
/// What the board says back is of no use to a door that has already done
/// what it was for: a refusal (the door was shut meanwhile, so the answer
/// comes as a message too), a daemon that cannot be reached, a daemon of
/// Node's that knows no such route: none of them is raised. A board that
/// cannot be reached is asked again, as a poll is ([`POLL_RETRIES`]): the
/// answer is in the harness's hands already, and a receipt that was lost
/// leaves it queued, to be pasted a second time and taken for the result.
/// Saying it twice does no harm: the board takes an answer already read as
/// read, and a claim already given back as given back. A hook whose board is
/// down for good ends that long later.
pub fn acknowledge(board: &Board, answer: &Answer, received: bool) {
    acknowledge_retrying(board, answer, received, &POLL_RETRIES);
}

/// [`acknowledge`], a receipt that gets no answer said again after each pause
/// of `retries`.
fn acknowledge_retrying(board: &Board, answer: &Answer, received: bool, retries: &[Duration]) {
    let Some(id) = answer.id() else {
        return;
    };
    let path = format!("/api/answers/{id}/receipt");
    let body = json!({ "received": received });
    let mut lost = 0;
    while let Err(cause) = board.post(&path, &body) {
        if !cause.is_unreachable() || lost >= retries.len() {
            return;
        }
        thread::sleep(retries[lost]);
        lost += 1;
    }
}

/// The code the board gives a door it has shut.
const DOOR_CLOSED: &str = "door-closed";

/// How long a door waits for the board before the harness's own dialog takes over.
pub const DOOR_WAIT: Duration = Duration::from_millis(3_500_000);
/// One request's share of that wait; the API holds a request 25 seconds at most.
const POLL_WAIT: Duration = Duration::from_secs(20);
/// How long a door waits before it asks again after a poll that got no
/// answer, once for each poll that failed in a row: the poll's reply may have
/// been lost on its way back, and the board gives the same answer to the same
/// question again. A board that stays out of reach through all of them is
/// gone, and the harness's own dialog takes over.
const POLL_RETRIES: [Duration; 4] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_millis(1_000),
    Duration::from_millis(2_000),
];

/// What a member's window tells its model when the board refuses its
/// question: nobody watches a member's window, so its own dialog would hold
/// the task for good.
fn refusal_reason(cause: &BoardError) -> String {
    format!(
        "ConsensFlow could not put this question to the chief ({cause}). Ask with cf ask \"…\" instead."
    )
}

/// Puts `questions` on the board and waits up to `wait` for their answer
/// (`None` when the wait ran out first; a wait too long to reach never runs
/// out), or until `stop` is raised: checked before each request, so a poll
/// already held at the board ends first (up to 20 seconds). A window whose
/// question nobody waits for any more (its broker closed, its connection
/// gone) raises it. A poll that gets no answer is asked again
/// ([`POLL_RETRIES`]); the question itself is put once, whatever comes of it.
fn ask_the_board_until(
    board: &Board,
    questions: &[Question],
    wait: Duration,
    stop: &AtomicBool,
) -> Result<Option<Answer>, BoardError> {
    ask_retrying(board, questions, wait, stop, &POLL_RETRIES)
}

/// [`ask_the_board_until`], a poll that gets no answer asked again after each
/// pause of `retries`.
fn ask_retrying(
    board: &Board,
    questions: &[Question],
    wait: Duration,
    stop: &AtomicBool,
    retries: &[Duration],
) -> Result<Option<Answer>, BoardError> {
    if stop.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let posted = board.post("/api/questions", &json!({ "questions": questions }))?;
    let id = posted
        .value()
        .pointer("/message/id")
        .and_then(Value::as_u64)
        .ok_or_else(|| posted.lacks("message id"))?;
    let until = Instant::now().checked_add(wait);
    let mut lost = 0;
    loop {
        let left = until.map_or(POLL_WAIT, |until| {
            until.saturating_duration_since(Instant::now())
        });
        if left.is_zero() || stop.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let path = format!(
            "/api/questions/{id}?wait={}",
            left.min(POLL_WAIT).as_millis()
        );
        let polled = match board.get(&path) {
            Ok(polled) => polled,
            Err(cause) if cause.is_unreachable() && lost < retries.len() => {
                thread::sleep(retries[lost]);
                lost += 1;
                continue;
            }
            Err(cause) => return Err(cause),
        };
        lost = 0;
        match polled.part("answer")? {
            Value::Null => {}
            answer => {
                return serde_json::from_value(answer.clone())
                    .map(Some)
                    .map_err(|_| polled.lacks("answer"));
            }
        }
    }
}

#[cfg(test)]
mod tests;
