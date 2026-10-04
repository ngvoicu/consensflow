//! `cf hook <harness>`: what a harness's hooks run. A hook prints only what
//! its harness must read, nothing at all when it has nothing to say, and
//! exits 0 whatever happened: a failing hook would stop its harness.

use std::io::{self, Read, Write};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use cf_base::env::Env;
use cf_base::js;
use cf_board::door::{self, DOOR_WAIT};
use cf_harness::{claude, devin};
use cf_proto::questions::Question;

pub fn run(
    harness: Option<&str>,
    env: &Env,
    input: &mut dyn Read,
    out: &mut dyn Write,
) -> io::Result<u8> {
    let said = match harness {
        Some(tool @ ("claude" | "devin")) => {
            // Outside a window there is no board to ask: nothing is read, nothing said.
            let Some(board) = door::board_of(env) else {
                return Ok(0);
            };
            let wait = question_wait(env);
            let ask = |questions: &[Question]| {
                door::ask(&board, questions, wait, &AtomicBool::new(false))
            };
            if tool == "claude" {
                claude::question_hook(input, ask)
            } else {
                devin::question_hook(input, ask)
            }
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
