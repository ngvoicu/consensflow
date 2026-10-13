//! Live: a chief that is in a turn when two notes come for it is handed each of them
//! once. It is opt-in (`cargo xtask live notes`, `npm run live:notes`): it spends the
//! machine's quota on a chief of each harness named, and needs each installed and
//! logged in, so it is ignored unless asked for and is no part of `check` or of CI.
//!
//! The field bug it answers (2026-10-13, ConsensFlow 3.0.3, a Devin chief on
//! Windows): the human cancelled T-17, ConsensFlow sent the chief two notes one right
//! after the other (m-186, "@human cancelled T-17 (…)", and m-187, "T-17's window
//! closed with background work still running: …"), and the chief reported that each
//! went into its window twice in a row. A paste is confirmed when the harness's record
//! shows its header, and one the record had not shown in a minute was taken for lost
//! and pasted again; a harness that holds what it is given in a turn, and writes it to
//! its record when the turn takes it, shows it later than that. What the rig's
//! stand-in Devin shows of the daemon's side (`regressions/devin_notes_twice.rs`) this
//! shows of a real one: whether its record is late, and whether the daemon waits.
//!
//! A real harness is the chief of a project on the native daemon, and its one member
//! is the stand-in (no model). The human asks the chief to run a command that takes
//! 100 s, and while it is in it the chief gives the member a task whose window
//! leaves a job running and the human cancels it: ConsensFlow has two notes for the
//! chief, and cannot give them yet. The command ends by itself, the chief is at rest,
//! and the notes go in one after the other. The folder tells the chief to run a command
//! on each note and to wait for it, and that command takes 60 s, twice the daemon's
//! arrival window of the run (30 s, from `CONSENSFLOW_ARRIVAL_TIMEOUT_MS`): a harness
//! that keeps the second note until the first turn ends shows it later than the
//! window, and a daemon that took that for a paste lost would paste it again.
//!
//! The commands are `sleep`, and a ping of the machine's own address on Windows
//! (`quiet_command`): they end by themselves, so the check finds none and ends none
//! (which a process list would, and Windows has no `ps`), and it reads the chief's
//! turns from what the daemon wrote of its window. When both notes are given and the
//! chief has been at rest for 20 s, the check reads the chief's own record, as the
//! daemon reads it (Claude Code's conversation, Devin's store), and holds it to this:
//! the start of each note's header is in the messages the chief was given exactly once
//! ([`verdict`]). Said beside it, and judged by nothing: how often the daemon pasted
//! each note, and how long after the first paste the record showed it (a harness that
//! holds a note in its turn shows it as late as the turn is long), and whether the
//! daemon pasted a note again and why.
//!
//! ```text
//! cargo xtask live notes                      Devin and Claude, each as the chief
//! cargo xtask live notes devin                one
//! cargo xtask live notes claude devin         each, one after the other
//! cargo xtask live notes devin --keep         leaves the run's home and project where they are
//! cargo xtask live notes -- --list            lists this test
//! cargo xtask windows --host zeewin -- cargo xtask live notes devin
//! ```
//!
//! Harnesses: the two the app pastes into, claude and devin (Codex, Pi and OpenCode
//! take their messages through their own queues, and the daemon waits for the record
//! of what they queued). The run fails when a note went into a chief's record twice, or
//! never, or when the check could not hold a turn open to find out.

// The parts are in `live_notes/`: cargo looks for the modules of a test's root file
// beside it, where each would be a suite of its own.
#[path = "live_notes/check.rs"]
mod check;
#[path = "live_notes/turns.rs"]
mod turns;
#[path = "live_notes/verdict.rs"]
mod verdict;

use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, ask, Harness, Log};
use cf_e2e::{Error, Result};

/// What the line of a chief that held says it showed, and one that did not.
const HELD: &str = "a chief in a turn is handed each of two notes once";
const FAILED: &str = "a chief in a turn was not handed each of two notes once";

/// The chiefs a run is asked for: Devin and Claude when none is named, else those
/// named, each once, in the order given. Only the harnesses the daemon pastes into
/// can be asked: the rest queue what they are given, and the daemon waits for them.
fn chiefs(named: &[String]) -> Result<Vec<Harness>> {
    if named.is_empty() {
        return Ok(vec![Harness::Devin, Harness::Claude]);
    }
    let mut asked = Vec::new();
    for word in named {
        let harness = match Harness::from_name(word) {
            Some(harness @ (Harness::Claude | Harness::Devin)) => harness,
            Some(other) => {
                return Err(Error::Daemon(format!(
                    "{} takes its messages through its own queue and is never pasted into: only claude and devin can be the chief of this check",
                    other.name()
                )))
            }
            None => return Err(Error::Daemon(format!("no such harness: {word}"))),
        };
        if !asked.contains(&harness) {
            asked.push(harness);
        }
    }
    Ok(asked)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a chief of each harness named; cargo xtask live notes (npm run live:notes)"]
fn a_chief_in_a_turn_is_handed_each_of_two_notes_once() -> Result {
    let log = Log::new("live notes");
    ask::prepare()?;
    let asked = chiefs(&[live::list("chief"), live::list("harness")].concat())?;
    let mut checks = Vec::new();
    for chief in asked {
        let scoped = log.scoped(format!("{} chief", chief.name()));
        let case = check::run(chief, &scoped);
        let version = live::version_of(chief);
        log.block(
            &format!("== {} chief: {version}", chief.name()),
            &case.text(),
        );
        checks.push(Check::new(
            &format!("{} chief", chief.name()),
            if case.held() { HELD } else { FAILED },
            case.held(),
            version,
        ));
    }
    report::conclude(&log, "what each chief came to", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn devin_and_claude_are_the_chiefs_unless_told_and_those_told_run_once_each_in_the_order_given()
    {
        assert_eq!(
            chiefs(&[]).unwrap(),
            [Harness::Devin, Harness::Claude],
            "both, Devin first"
        );
        assert_eq!(
            chiefs(&words(&["claude", "devin", "claude"])).unwrap(),
            [Harness::Claude, Harness::Devin]
        );
        assert_eq!(chiefs(&words(&["devin"])).unwrap(), [Harness::Devin]);
    }

    #[test]
    fn a_harness_the_daemon_never_pastes_into_is_refused_and_says_why() {
        for queued in ["codex", "pi", "opencode"] {
            let failed = chiefs(&words(&[queued])).unwrap_err().to_string();
            assert!(failed.contains("its own queue"), "{queued}: {failed}");
            assert!(failed.starts_with(queued), "{failed}");
        }
        assert_eq!(
            chiefs(&words(&["nobody"])).unwrap_err().to_string(),
            "no such harness: nobody"
        );
    }
}
