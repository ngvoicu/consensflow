//! `cf hook <harness>`: what a harness's hooks run. A hook prints only what
//! its harness must read, nothing at all when it has nothing to say, and
//! exits 0 whatever happened: a failing hook would stop its harness.

use std::io::{self, Read, Write};
use std::time::Duration;

use cf_base::env::Env;
use cf_base::js;
use cf_base::json::js_order;
use cf_board::door::DOOR_WAIT;
use cf_board::Board;
use cf_harness::{claude, devin};

pub fn run(
    harness: Option<&str>,
    env: &Env,
    input: &mut dyn Read,
    out: &mut dyn Write,
) -> io::Result<u8> {
    let board = || {
        env.text("CONSENSFLOW_TOKEN")
            .map(|token| Board::new(env.text("CONSENSFLOW_URL"), token))
    };
    let said = match harness {
        Some("claude") => claude::question_hook(input, board().as_ref(), question_wait(env)),
        Some("devin") => devin::question_hook(input, board().as_ref(), question_wait(env)),
        // Devin reads a SessionStart hook's output whole: no line break after it.
        Some("devin-session") => {
            let said = devin::session_hook(
                input,
                env.path("CF_DEVIN_ROLE_FILE"),
                env.path("CHISEL_PURE_ACP_WIRE_LOG"),
            );
            if let Some(said) = said {
                write!(out, "{}", serde_json::to_string(&js_order(said))?)?;
            }
            return Ok(0);
        }
        _ => None,
    };
    if let Some(said) = said {
        writeln!(out, "{}", serde_json::to_string(&js_order(said))?)?;
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
