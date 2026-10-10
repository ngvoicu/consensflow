//! Waiting for the product, and what is said while waiting. The daemon is asked
//! how it is getting on once a second, every wait ends with an error that says
//! what was waited for, and a run that has nothing new to say says now and then
//! that it is still waiting: the designer's window, the chief's inbox and the
//! board are what it looks at.

use std::time::{Duration, Instant};

use cf_e2e::rig::{Project, Rig};
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

use crate::{report, text, Log};

/// How long the daemon has to open the designer's window, from the task's being
/// on the board: Codex's own start, and its role text asked for first.
const OPENS: Duration = Duration::from_secs(120);

/// How long the designer has to draw, and its result to reach the chief.
const DRAWS: Duration = Duration::from_secs(600);

/// How long a window has to close once its result is delivered.
pub const CLOSES: Duration = Duration::from_secs(60);

/// How often the daemon is asked how it is getting on while waiting, and how
/// often a run that has nothing new says it is still waiting.
const LOOK: Duration = Duration::from_secs(1);
const HEARTBEAT: Duration = Duration::from_secs(30);

/// `rig.wait_for`, looking once a second (each look asks the daemon), which says
/// what it waits for, and for how long, before it does.
pub fn wait(
    rig: &Rig,
    log: &Log,
    what: &str,
    within: Duration,
    mut check: impl FnMut() -> Result<bool>,
) -> Result {
    log.say(format!("waiting up to {} s for {what}", within.as_secs()));
    let mut looked: Option<Instant> = None;
    rig.wait_for(what, within, || {
        if looked.is_some_and(|at| at.elapsed() < LOOK) {
            return Ok(false);
        }
        looked = Some(Instant::now());
        check()
    })
}

/// The `pane.open` of the image designer's window, once the daemon has made it:
/// the window of a session of the member `pygmalion`.
pub fn designer_window(rig: &Rig, project: i64) -> Option<Value> {
    let prefix = format!("p{project}-pygmalion-");
    rig.open_frames().into_iter().find(|frame| {
        frame["id"]
            .as_str()
            .is_some_and(|id| id.starts_with(&prefix))
    })
}

/// How a window was opened, a word at a time, the long ones cut: the program
/// that supervises Codex, Codex, and the flags it was given.
fn launched(window: &Value) -> String {
    window["argv"]
        .as_array()
        .map(|argv| {
            argv.iter()
                .map(|word| report::cut(&text(word), 90))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

/// The last `lines` lines of a window's screen, as the pane host keeps it, each
/// on a line of its own after a mark; nothing where the window is gone.
fn on_screen(rig: &Rig, window: &Value, lines: usize) -> String {
    let asked = rig.host(
        "pane.snapshot",
        json!({ "id": window["id"], "generation": window["generation"], "tail": lines }),
    );
    let tail = asked.ok().map(|answer| answer["tail"].clone());
    tail.as_ref()
        .and_then(Value::as_array)
        .map(|shown| {
            shown
                .iter()
                .map(|line| format!("\n    | {}", report::cut(&text(line), 110)))
                .collect()
        })
        .unwrap_or_default()
}

/// Why waiting for the task's result is no use, if it is none: the task failed
/// or was called off, or the designer asked the chief a question, which the
/// test, playing a chief that does not answer, leaves where it is. Said from
/// what the chief was told (`inbox`).
fn stuck(task: i64, state: &str, inbox: &[Value]) -> Option<String> {
    let body = |message: &Value| report::cut(&text(&message["body"]), 400);
    if let Some(question) = inbox.iter().find(|message| message["kind"] == "question") {
        return Some(format!(
            "the designer asked the chief a question and waits for the answer: {}",
            body(question)
        ));
    }
    if state != "failed" && state != "cancelled" {
        return None;
    }
    let told = inbox
        .iter()
        .map(body)
        .find(|said| said.starts_with(&format!("T-{task} ")))
        .unwrap_or_else(|| "the chief was told nothing of it".to_owned());
    Some(format!("T-{task} is {state}: {told}"))
}

/// An error that says why waiting for the task's result is no use, when it is
/// none.
fn ended_with_no_result(project: &Project, task: i64) -> Result {
    let state = text(&project.task(task)?["state"]);
    match stuck(task, &state, &project.inbox("chief")?) {
        Some(why) => Err(Error::Daemon(why)),
        None => Ok(()),
    }
}

/// The result of the designer's the chief was given, if it was.
fn delivered(project: &Project) -> Result<Option<Value>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .find(|message| message["kind"] == "result" && message["state"] == "delivered"))
}

/// Waits for the daemon to open the designer's window.
pub fn await_window(rig: &Rig, project: &Project, task: i64, log: &Log) -> Result<Value> {
    wait(
        rig,
        log,
        "the daemon to open the image designer's window",
        OPENS,
        || {
            ended_with_no_result(project, task)?;
            Ok(designer_window(rig, project.id()).is_some())
        },
    )?;
    let window = designer_window(rig, project.id())
        .ok_or_else(|| Error::Daemon("the designer's window was opened, and is gone".to_owned()))?;
    log.say(format!(
        "the window {} is open: {}",
        window["id"],
        launched(&window)
    ));
    Ok(window)
}

/// Waits for the designer's result to be delivered to the chief, saying what
/// the task and the window are doing when that changes, and now and then when
/// it does not.
pub fn await_result(
    rig: &Rig,
    project: &Project,
    task: i64,
    window: &Value,
    log: &Log,
) -> Result<Value> {
    let mut said = (String::new(), Instant::now());
    wait(
        rig,
        log,
        "the image designer's result to reach the chief",
        DRAWS,
        || {
            ended_with_no_result(project, task)?;
            let state = text(&project.task(task)?["state"]);
            let activity = text(&project.lane("pygmalion")?["activity"]["state"]);
            let line = format!("T-{task} is {state}; the designer's window is {activity}");
            if line != said.0 || said.1.elapsed() >= HEARTBEAT {
                log.say(format!("{line}{}", on_screen(rig, window, 4)));
                said = (line, Instant::now());
            }
            Ok(delivered(project)?.is_some())
        },
    )?;
    delivered(project)?
        .ok_or_else(|| Error::Daemon("the result was delivered, and is gone".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_is_no_use_for_a_task_that_failed_or_was_called_off_or_a_designer_that_asked() {
        let note = json!({ "kind": "note", "body": "T-1 failed: the window never showed its first message" });
        let working: Vec<Value> = Vec::new();
        assert_eq!(stuck(1, "working", &working), None);
        assert_eq!(stuck(1, "done", std::slice::from_ref(&note)), None);
        assert_eq!(
            stuck(1, "failed", &[note]).as_deref(),
            Some("T-1 is failed: T-1 failed: the window never showed its first message")
        );
        assert_eq!(
            stuck(2, "cancelled", &working).as_deref(),
            Some("T-2 is cancelled: the chief was told nothing of it")
        );
        let asked = json!({ "kind": "question", "body": "Which colour should the honey be?" });
        assert_eq!(
            stuck(1, "working", &[asked]).as_deref(),
            Some(
                "the designer asked the chief a question and waits for the answer: \
                 Which colour should the honey be?"
            )
        );
    }

    #[test]
    fn a_window_is_shown_as_it_was_opened_a_word_at_a_time_and_the_long_ones_cut() {
        let long = "x".repeat(200);
        let window = json!({ "argv": ["/bin/cf", "codex-session", long, "-c", "a\nb"] });
        let shown = launched(&window);
        assert!(shown.starts_with("/bin/cf codex-session xxxx"), "{shown}");
        assert!(shown.ends_with("… -c a b"), "{shown}");
        assert!(shown.len() < 140, "{shown}");
        assert_eq!(launched(&json!({})), "");
    }
}
