//! The daemon's readiness of a Devin window a live test opened, as the product does
//! it: `harness-ask` drives the adapter's own window (`devin_readiness`) until it is
//! ready, unfit or the time is up, and asks the rig's pane host what it asks of one
//! (its screen, the `/bypass` it pastes and the Enter it presses), each request a line
//! of its output that this answers on its input. What the window was given is what
//! the product gave it: a check that pasted the command itself passed whatever the
//! product did (Astra and Sol on the B1 fix).

use std::io::BufRead;
use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

use super::answered;
use crate::live::window::Window;
use crate::process::Run;
use crate::rig::Rig;
use crate::{cf, Error, Result};

/// How often the window is asked, as the engine's looks come.
const EVERY: Duration = Duration::from_millis(500);

/// What came of the readiness of a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevinReadiness {
    /// What its looks answered, in order: `ready`, `held: …` or `unfit: …`, one that
    /// came again in a row said once.
    pub answers: Vec<String>,
    /// Whether the last look found it ready.
    pub ready: bool,
    /// What the window was given meanwhile: the text of each paste, and `⏎` for each
    /// Enter pressed alone.
    pub given: Vec<String>,
}

/// The pane host's request `op` with `body`, as what the window was given says it,
/// where it gives the window something.
fn given_by(op: &str, body: &Value) -> Option<String> {
    match op {
        "pane.write_paste" => Some(body["body"].as_str().unwrap_or_default().to_owned()),
        "pane.input" if body["bytes"] == json!([13]) => Some("⏎".to_owned()),
        "pane.input" => Some(format!("keys {}", body["bytes"])),
        _ => None,
    }
}

/// The readiness of `window`, a Devin window open in `rig` whose launch's wire log is
/// `wire`, driven as the daemon drives it for up to `within`.
pub fn devin_readiness(
    rig: &Rig,
    window: &Window,
    wire: &Path,
    within: Duration,
) -> Result<DevinReadiness> {
    let pane = window.pane();
    let session = window
        .session()
        .ok_or_else(|| Error::Daemon("the window is on no conversation".to_owned()))?;
    let millis = |span: Duration| u64::try_from(span.as_millis()).unwrap_or(u64::MAX);
    let question = json!({
        "op": "devin_readiness",
        "wire": wire,
        "session": session,
        "pane": pane,
        "withinMs": millis(within),
        "everyMs": millis(EVERY),
    });
    let mut given = Vec::new();
    let answer = relaying(
        cf::harness_ask()?,
        &question,
        within + SPARE,
        &mut |op, body| {
            given.extend(given_by(op, body));
            rig.host(op, body.clone())
        },
    )?;
    let answers = answer["answers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|said| said.as_str().map(str::to_owned))
        .collect();
    Ok(DevinReadiness {
        answers,
        ready: answer["ready"] == true,
        given,
    })
}

/// How long past the readiness's own time the helper is given: its start, and a pane
/// host's answer to its last request.
const SPARE: Duration = Duration::from_secs(30);

/// One question to the test binary `program` (`harness-ask`) whose answer comes after
/// the pane host's requests it makes, which `host` answers: what it answered. The helper
/// has until `deadline` from its start, whatever it prints or does not: one stalled
/// inside a readiness held the check for ever, its output never ending (Astra on the B1
/// fix, the second look). Its output is read on a thread of its own, and a helper past
/// its time is ended, and the question fails.
fn relaying(
    program: &Path,
    body: &Value,
    deadline: Duration,
    host: &mut dyn FnMut(&str, &Value) -> Result<Value>,
) -> Result<Value> {
    let started = std::time::Instant::now();
    let mut asked = Run::new(program)
        .input(format!("{body}\n"))
        .inheriting_env()
        .spawn()?;
    let output = asked
        .take_output()
        .ok_or_else(|| Error::Daemon("harness-ask has no output to read".to_owned()))?;
    let (lines, read) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in output.lines() {
            let Ok(line) = line else {
                break;
            };
            if lines.send(line).is_err() {
                break;
            }
        }
    });
    let mut said = String::new();
    loop {
        let left = deadline.saturating_sub(started.elapsed());
        let line = match read.recv_timeout(left) {
            Ok(line) => line,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                asked.kill();
                return Err(Error::Timeout(format!(
                    "harness-ask did not answer in {} s, and was ended; it said {said:?}",
                    deadline.as_secs()
                )));
            }
        };
        let request = serde_json::from_str::<Value>(&line)
            .ok()
            .and_then(|frame| frame.get("host").cloned());
        let Some(request) = request else {
            said.push_str(&line);
            said.push('\n');
            continue;
        };
        let op = request["op"].as_str().unwrap_or_default();
        let reply = match host(op, &request["body"]) {
            Ok(answer) => json!({ "answer": answer }),
            Err(failed) => json!({ "error": failed.to_string() }),
        };
        asked.send(format!("{reply}\n"));
    }
    let code = asked
        .wait(deadline.saturating_sub(started.elapsed()))?
        .and_then(|status| status.code());
    answered("harness-ask", code, &said)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program that reads its input and says nothing until its input ends, which the
    /// relay never ends: a helper stalled inside a readiness.
    fn stalled() -> std::path::PathBuf {
        if cfg!(windows) {
            let root =
                crate::process::own_var("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
            Path::new(&root).join("System32").join("sort.exe")
        } else {
            "/bin/cat".into()
        }
    }

    #[test]
    fn a_helper_that_stalls_is_ended_at_its_deadline_and_the_question_fails() {
        // A helper stalled inside a readiness held the check for ever (Astra on the B1 fix,
        // the second look): the relay waits on its output only until the deadline.
        let asked = std::sync::mpsc::channel();
        let (answer, answered) = asked;
        std::thread::spawn(move || {
            let program = stalled();
            let came = relaying(
                &program,
                &json!({ "op": "devin_readiness" }),
                Duration::from_secs(1),
                &mut |_, _| Ok(json!({ "ok": true })),
            );
            let _ = answer.send(came.map_err(|failed| failed.to_string()));
        });
        let came = answered
            .recv_timeout(Duration::from_secs(20))
            .expect("the relay came back within its deadline and a little more");
        let failed = came.expect_err("a helper that never answered is no answer");
        assert!(failed.contains("did not answer in 1 s"), "{failed}");
    }

    #[test]
    fn what_a_window_is_given_is_each_paste_and_each_enter_alone() {
        assert_eq!(
            given_by("pane.write_paste", &json!({ "body": "/bypass" })).as_deref(),
            Some("/bypass")
        );
        assert_eq!(
            given_by("pane.input", &json!({ "bytes": [13] })).as_deref(),
            Some("⏎")
        );
        assert_eq!(
            given_by("pane.input", &json!({ "bytes": [27] })).as_deref(),
            Some("keys [27]")
        );
        assert_eq!(given_by("pane.snapshot", &json!({ "id": "p" })), None);
    }
}
