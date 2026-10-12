//! What the run holds the worker's task to, once it has waited for it: judged
//! from what it saw of the board and of the server while it waited, and from what
//! the chief was given. Every way the task fell short is said together, so that a
//! run that fails says all that was wrong with it.

use cf_e2e::live::held::Reasons;

/// The sum the task asks for, which the worker's answer says.
pub const ANSWER: &str = "5555";

/// What the run saw of the worker's task while it waited for it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// The state the task was last seen in.
    pub state: String,
    /// What the board said the worker's window was doing besides working, each word
    /// once with when: a window held for processes says which, which a worker whose
    /// only other process is its harness's server must never be.
    pub reasons: Reasons,
    /// Whether the server wrote its process id while the task was at work: the
    /// worker's Claude started it. (It is gone with the window once the task is
    /// done, so a look may well never find it running.)
    pub server_started: bool,
    /// The result the chief was given, if it was.
    pub result: Option<String>,
}

/// The task is done, its result reached the chief and says the answer, the board
/// never said the window was held for background work, and the server was started
/// while the worker worked (a task that was never held for a server that was never
/// there proves nothing): or every way it was not.
pub fn judge(seen: &Seen) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    if seen.state != "done" {
        problems.push(format!("T-1 is {}, not done", seen.state));
    }
    if let Some(held) = seen.reasons.held() {
        problems.push(format!(
            "the board said the worker's window was held for work its turn left: {held}"
        ));
    }
    if !seen.server_started {
        problems.push(
            "the MCP server never wrote its process id, so the run cannot tell a task that was not held from a window that had no server"
                .to_owned(),
        );
    }
    match &seen.result {
        None => problems.push("the chief was given no result".to_owned()),
        Some(body) if !body.contains(ANSWER) => {
            problems.push(format!("the result does not say {ANSWER}: {body}"));
        }
        Some(_) => {}
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn good() -> Seen {
        Seen {
            state: "done".to_owned(),
            reasons: Reasons::default(),
            server_started: true,
            result: Some("5555".to_owned()),
        }
    }

    #[test]
    fn a_task_done_with_its_result_at_the_chief_and_no_hold_beside_a_server_that_started_passes() {
        assert_eq!(judge(&good()), Ok(()));
        // The board may say other things of a window; only a hold is wrong.
        let mut seen = good();
        seen.reasons
            .note(Duration::from_secs(2), "waiting for the human");
        assert_eq!(judge(&seen), Ok(()));
    }

    #[test]
    fn a_window_the_board_said_was_held_for_processes_fails_and_names_them() {
        let mut seen = good();
        let held =
            "background work running (2 processes): npm exec @playwright/mcp; node server.js";
        seen.reasons.note(Duration::from_secs(3), held);
        // What was said is kept once, whatever the number of looks.
        seen.reasons.note(Duration::from_secs(4), held);
        seen.reasons.note(Duration::from_secs(5), "");
        let problems = judge(&seen).unwrap_err();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("npm exec @playwright/mcp"),
            "{problems:?}"
        );
        assert!(
            problems[0].contains("said from 3.0 s to 4.0 s"),
            "{problems:?}"
        );
        assert_eq!(seen.reasons.said().len(), 1);
    }

    #[test]
    fn a_window_held_for_agents_its_harness_says_are_pending_is_held_too() {
        let mut seen = good();
        seen.reasons
            .note(Duration::from_secs(3), "background agents running");
        let problems = judge(&seen).unwrap_err();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("background agents running"),
            "{problems:?}"
        );
    }

    #[test]
    fn every_way_the_task_fell_short_is_said_and_a_server_that_never_started_proves_nothing() {
        let problems = judge(&Seen {
            state: "working".to_owned(),
            ..Seen::default()
        })
        .unwrap_err();
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems[0].contains("working, not done"));
        assert!(problems[1].contains("never wrote its process id"));
        assert!(problems[2].contains("no result"));

        let mut wrong = good();
        wrong.result = Some("six thousand".to_owned());
        let problems = judge(&wrong).unwrap_err();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("does not say 5555"), "{problems:?}");
    }
}
