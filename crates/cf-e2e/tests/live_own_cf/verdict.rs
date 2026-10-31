//! What a worker's run of `live own-cf` came to, from what it left: the calls each decoy wrote
//! down beside itself (the proof a shell found it first, whatever the model then said), and the
//! worker's result, in which the window's own `cf` answering `cf whoami` names the worker by
//! its handle and role, which only the daemon the window belongs to knows: the handle the board
//! gives the worker's participant (`live-devin-silver-island`), not its agent's id.

/// What was seen of one worker.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    /// The task's state when the wait for it ended.
    pub state: String,
    /// The worker's result, once it is the chief's.
    pub result: Option<String>,
    /// The calls each decoy was given, by its label: a decoy given none is not here.
    pub decoys: Vec<(&'static str, Vec<String>)>,
    /// The worker's handle, as the board names its participant: empty where it named none.
    pub handle: String,
}

/// What the window's own `cf` says to `cf whoami` in the window of the worker `handle`: its
/// handle and role, as `@live-devin (worker)`.
pub fn whoami_of(handle: &str) -> String {
    format!("@{handle} (worker)")
}

/// The verdict: `Ok` with what holds it, where the window's own `cf` answered and no decoy was
/// run; else every reason it does not hold.
pub fn judge(seen: &Seen) -> Result<String, Vec<String>> {
    let handle = seen.handle.as_str();
    let mut problems = Vec::new();
    for (label, calls) in &seen.decoys {
        problems.push(format!(
            "the worker's shell found the decoy cf in {label} first: it ran `cf {}` there",
            calls.join("`, `cf ")
        ));
    }
    if seen.state != "done" {
        problems.push(format!("the task is {}, not done", seen.state));
    }
    let answered = !handle.is_empty()
        && seen
            .result
            .as_deref()
            .is_some_and(|result| result.contains(&whoami_of(handle)));
    match &seen.result {
        None if seen.state == "done" => {
            problems.push("the task is done, and no result reached the chief".to_owned());
        }
        Some(_) if !answered && seen.decoys.is_empty() => problems.push(format!(
            "the result does not hold what the window's own cf answers to `cf whoami` ({}): \
             no cf was proven run",
            whoami_of(handle)
        )),
        _ => {}
    }
    if problems.is_empty() {
        Ok(format!(
            "the window's own cf answered `cf whoami` ({}); no decoy was run",
            whoami_of(handle)
        ))
    } else {
        Err(problems)
    }
}

/// The line of the result that names the `cf` the worker's shell finds, as it reported it
/// (`command -v cf`, PowerShell's `Get-Command`, cmd's `where`): the first line of it that ends
/// in `cf` or `cf.exe` and names a folder, or none.
pub fn found_line(result: &str) -> Option<&str> {
    result.lines().map(str::trim).find(|line| {
        let line = line.trim_matches('`');
        (line.ends_with("/cf") || line.ends_with("\\cf") || line.ends_with("cf.exe"))
            && (line.contains('/') || line.contains('\\'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The handle the board gives a Devin worker: its agent's id and two words.
    const HANDLE: &str = "live-devin-silver-island";

    fn seen(state: &str, result: Option<&str>, decoys: Vec<(&'static str, Vec<String>)>) -> Seen {
        Seen {
            state: state.to_owned(),
            result: result.map(str::to_owned),
            decoys,
            handle: HANDLE.to_owned(),
        }
    }

    const ANSWER: &str =
        "1. @live-devin-silver-island (worker) in project own-cf-devin, on T-1: Which cf\n\
                          2. /x/consensflow/bin/cf\n3. /x/consensflow/bin:/usr/bin";

    #[test]
    fn the_windows_own_cf_answering_whoami_and_no_decoy_run_holds() {
        let held = judge(&seen("done", Some(ANSWER), Vec::new()));
        assert_eq!(
            held,
            Ok(
                "the window's own cf answered `cf whoami` (@live-devin-silver-island (worker)); \
                 no decoy was run"
                    .to_owned()
            )
        );
    }

    #[test]
    fn a_decoy_that_was_run_fails_it_by_its_folder_whatever_the_result_says() {
        let decoys = vec![(
            "~/.local/bin",
            vec!["whoami".to_owned(), "--version".to_owned()],
        )];
        let failed = judge(&seen("done", Some(ANSWER), decoys)).unwrap_err();
        assert_eq!(
            failed,
            [
                "the worker's shell found the decoy cf in ~/.local/bin first: it ran `cf whoami`, `cf --version` there"
            ]
        );
    }

    #[test]
    fn a_result_with_no_answer_of_the_daemons_proves_nothing_and_fails() {
        let failed = judge(&seen("done", Some("stand-in cf: whoami"), Vec::new())).unwrap_err();
        assert_eq!(
            failed,
            [
                "the result does not hold what the window's own cf answers to `cf whoami` \
              (@live-devin-silver-island (worker)): no cf was proven run"
            ]
        );
        // Another worker's handle is not this one's, and the agent's id alone is no handle.
        let mut other = seen("done", Some(ANSWER), Vec::new());
        other.handle = "live-pi-noble-birch".to_owned();
        assert!(judge(&other).is_err());
        let mut bare = seen(
            "done",
            Some("@live-devin (worker) in project p"),
            Vec::new(),
        );
        bare.handle = "live-devin-silver-island".to_owned();
        assert!(judge(&bare).is_err());
    }

    #[test]
    fn a_worker_the_board_named_no_handle_of_proves_nothing() {
        // A `cf whoami` of a participant with no handle says `@ (worker)`: an empty handle
        // found in it proves nothing of this worker.
        let mut nameless = seen(
            "done",
            Some("@ (worker) in project own-cf-devin"),
            Vec::new(),
        );
        nameless.handle = String::new();
        assert!(judge(&nameless).is_err());
    }

    #[test]
    fn a_task_not_done_or_done_with_no_result_fails_saying_so() {
        assert_eq!(
            judge(&seen("working", None, Vec::new())).unwrap_err(),
            ["the task is working, not done"]
        );
        assert_eq!(
            judge(&seen("done", None, Vec::new())).unwrap_err(),
            ["the task is done, and no result reached the chief"]
        );
    }

    #[test]
    fn the_line_that_names_the_cf_found_is_one_that_ends_in_cf_and_names_a_folder() {
        assert_eq!(found_line(ANSWER), Some("2. /x/consensflow/bin/cf"));
        assert_eq!(
            found_line("C:\\Users\\me\\scoop\\shims\\cf.exe\nmore"),
            Some("C:\\Users\\me\\scoop\\shims\\cf.exe")
        );
        assert_eq!(
            found_line("`/c/Users/me/.local/bin/cf`"),
            Some("`/c/Users/me/.local/bin/cf`")
        );
        assert_eq!(found_line("cf\nnothing here"), None);
    }
}
