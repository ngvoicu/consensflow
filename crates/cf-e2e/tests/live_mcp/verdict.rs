//! What the run holds the worker's task to, once it has waited for it: judged
//! from what it saw of the board and of the server while it waited, and from what
//! the chief was given and told. Every way the task fell short is said together, so
//! that a run that fails says all that was wrong with it.

/// The sum the task asks for, which the worker's answer says.
pub const ANSWER: &str = "5555";

/// What the run saw of the worker's task while it waited for it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// The state the task was last seen in.
    pub state: String,
    /// The server as the run first saw it, while the task was at work: the process
    /// it wrote and when that began, as the system says it. None while it had not
    /// written its process id: the worker's Claude had not started it.
    pub server: Option<String>,
    /// The server once the daemon had taken the result and had time to end what the
    /// turn left, which the worker's window being kept open by the human leaves it to
    /// live through: the same process, or none where it is gone or another took its
    /// number.
    pub server_after: Option<String>,
    /// The result the chief was given, if it was.
    pub result: Option<String>,
    /// What the chief was told of work the worker's turn left: the notes that say so,
    /// read once the daemon had done with it, which a worker whose only other process
    /// is its harness's server must never cause: the host ends what it proves the
    /// turn's, and leaves alone and says nothing of what it cannot tell.
    pub notes: Vec<String>,
}

/// The task is done, its result reached the chief and says the answer, the chief was
/// told nothing of work the turn left, and the server, which was started while the
/// worker worked (a task that was never at risk of ending a server that was never there
/// proves nothing), is the same process it was after the daemon took the result and
/// ended what the turn left: the server is the harness's own, and ending it would
/// break the worker's tools. Or every way it was not.
pub fn judge(seen: &Seen) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    if seen.state != "done" {
        problems.push(format!("T-1 is {}, not done", seen.state));
    }
    for note in &seen.notes {
        problems.push(format!(
            "the chief was told the worker's turn left work, which a turn that ran no tool did not (what the daemon ended may be the harness's own MCP server): {note}"
        ));
    }
    match (&seen.server, &seen.server_after) {
        (None, _) => problems.push(
            "the MCP server never wrote its process id, so the run cannot tell a worker whose server was left alone from a window that had no server"
                .to_owned(),
        ),
        (Some(before), after) if after.as_ref() != Some(before) => {
            problems.push(format!(
                "the worker's MCP server was ended, or replaced, when the result was taken: it was {before}, and after the daemon had ended what the turn left it is {}",
                after.as_deref().unwrap_or("gone")
            ));
        }
        (Some(_), _) => {}
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
    use super::*;

    const SERVER: &str = "4242 Tue Oct 13 22:24:16 2026";

    fn good() -> Seen {
        Seen {
            state: "done".to_owned(),
            server: Some(SERVER.to_owned()),
            server_after: Some(SERVER.to_owned()),
            result: Some("5555".to_owned()),
            notes: Vec::new(),
        }
    }

    #[test]
    fn a_task_done_with_its_result_at_the_chief_and_nothing_said_of_work_beside_a_server_that_lived_passes(
    ) {
        assert_eq!(judge(&good()), Ok(()));
    }

    #[test]
    fn a_chief_told_the_turn_left_work_fails_and_names_what_was_said() {
        let mut seen = good();
        let note = "T-1's turn left background work running. Ended: npm exec @playwright/mcp; node server.js.";
        seen.notes = vec![note.to_owned()];
        let problems = judge(&seen).unwrap_err();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].ends_with(note), "{problems:?}");
        assert!(problems[0].contains("MCP server"), "{problems:?}");
    }

    #[test]
    fn a_server_that_was_ended_or_replaced_by_the_time_the_daemon_was_done_fails_whether_or_not_it_was_said(
    ) {
        // Ended, and nothing told: the harness's own server is gone all the same.
        let mut ended = good();
        ended.server_after = None;
        let problems = judge(&ended).unwrap_err();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains(SERVER) && problems[0].ends_with("it is gone"),
            "{problems:?}"
        );
        // Another process with its number, or the server started again: not the one.
        let mut replaced = good();
        replaced.server_after = Some("4242 Tue Oct 13 22:25:01 2026".to_owned());
        let problems = judge(&replaced).unwrap_err();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("Tue Oct 13 22:25:01 2026"),
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
