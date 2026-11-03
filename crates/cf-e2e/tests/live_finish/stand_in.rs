//! The checks against the rig's stand-in agent, which the real daemon and pane host take
//! for a harness: a worker that runs a command as a process of its window and replies,
//! and a worker that leaves a job running past its turn and answers. No harness and no
//! quota, and the daemon and the host are the real ones, so what this proves is that the
//! checks read what they say (the task, the host's listing of the tree, the chief's
//! inbox, what became of the process) and say what they saw as they do of a real harness,
//! whose own behaviour is what the live run is for.

use std::path::Path;

use cf_e2e::live::members::Plan;
use cf_e2e::live::{Log, Who};
use cf_e2e::Result;

use crate::check::{self, Finished};
use crate::leaving;
use crate::task::Task;
use crate::FAKE_AGENT;

/// The stand-in worker run on `task` in a project of the plan `plan`.
fn run(plan: &Plan, task: &Task) -> Result<Finished> {
    let log = Log::new("live finish test");
    check::run(Who::StandIn, plan, task, Path::new(FAKE_AGENT), &log)
}

#[test]
fn a_stand_in_that_runs_the_command_and_replies_finishes_its_task_and_nothing_is_ended() -> Result {
    let finished = run(&check::plan(Who::StandIn), &Task::stand_in())?;
    let text = finished.case.text();
    assert!(finished.case.held(), "{text}");
    assert!(
        text.contains("ok   the result reached the chief, nothing of its own ended: T-1 done "),
        "{text}"
    );
    assert!(
        finished.summary.starts_with("T-1 done ") && finished.summary.contains("nothing ended"),
        "{}",
        finished.summary
    );
    // The turn's end is said whichever way the run came out, with what the host said the
    // window left when it closed and what the chief was told: the command it ran ended
    // with its turn, and left nothing.
    assert!(
        text.contains("what the pane host listed as the turn's work at "),
        "{text}"
    );
    assert!(
        text.contains(
            "what the pane host said the window left running when it closed (its pane.exit): none"
        ),
        "{text}"
    );
    assert!(
        text.contains("what the chief was told of work the turn left: nothing"),
        "{text}"
    );
    assert!(!text.contains("FAIL"), "{text}");
    Ok(())
}

/// The first number after `said` in `text`: the process a line of the run names.
#[cfg(unix)]
fn process_after(text: &str, said: &str) -> Option<u32> {
    text.split_once(said)?
        .1
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[cfg(unix)]
#[test]
fn a_stand_in_that_detaches_its_job_from_its_shell_escapes_the_ending_and_the_run_says_so_and_ends_it(
) -> Result {
    // What four harnesses' agents did (`nohup … &` in a shell that then ended): nothing
    // proves whose the job is, so the host leaves it running, and the chief is told
    // nothing. The run fails, honestly, and says that the window's close did not find it
    // either (the job is in a session of its own, with no marker), and it ends it. A look
    // of the run once a second may have caught the job while its shell still ran, which is
    // a third of a second: the failure is said as of a command the host listed then, or as
    // one it never listed.
    let log = Log::new("live finish test");
    let plan = leaving::plan(Who::StandIn);
    let brief = leaving::stand_in_brief(true);
    let finished = leaving::run_asked(
        Who::StandIn,
        &plan,
        Path::new(FAKE_AGENT),
        &brief,
        leaving::Expect::Ended,
        &log,
    )?;
    let text = finished.case.text();
    assert!(!finished.case.held(), "{text}");
    assert!(
        text.contains("the command escaped the ending")
            || text.contains("the command is still running after the task was done"),
        "{text}"
    );
    assert!(
        text.contains("what the chief was told of work the turn left: nothing"),
        "{text}"
    );
    let said = "it still runs after the window closed, which found nothing of it: ";
    assert!(text.contains(said), "{text}");
    // The run says where the job stands: in a session of its own, which has no terminal
    // (`??`) and which no pane has, so that the window's close did not find it.
    assert!(text.contains(", terminal ??)"), "{text}");
    // The run ended it: it is not left on the machine for what its seconds are.
    let job = process_after(&text, said).expect("the line names the job's process");
    let gone = (0..100).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        !cf_e2e::process::is_alive(job)
    });
    assert!(gone, "the job {job} is left running: {text}");
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_stand_in_that_plays_pi_on_a_mac_and_detaches_its_job_into_a_session_of_its_own_holds_the_case_as_the_known_limit_and_the_run_ends_the_job(
) -> Result {
    // Pi's and Devin's tool shells lead sessions of their own (T-1068: `sleep 4711`, parent 1,
    // terminal `??`), so a job they detach with `&` once the shell ends has nothing that ties it
    // to the window: the host never lists it, the close does not find it and the chief is told
    // nothing. The step expects that of those two on a Mac. It holds when the task is done and
    // its result delivered, the chief was told nothing false and the job is the run's own, which
    // the run ends by which process it is.
    let log = Log::new("live finish test");
    let plan = leaving::plan(Who::StandIn);
    let brief = leaving::stand_in_brief(true);
    let finished = leaving::run_asked(
        Who::StandIn,
        &plan,
        Path::new(FAKE_AGENT),
        &brief,
        leaving::Expect::OwnSession,
        &log,
    )?;
    let text = finished.case.text();
    assert!(finished.case.held(), "{text}");
    assert!(
        text.contains("the known limit on a Mac"),
        "the run says what it let pass: {text}"
    );
    assert!(
        text.contains("what the chief was told of work the turn left: nothing"),
        "{text}"
    );
    assert!(text.contains(", terminal ??)"), "{text}");
    assert!(!text.contains("FAIL"), "{text}");
    // It ended the job: nothing of the run's is left on the machine for what its seconds are.
    let job = process_after(&text, "still running: ").expect("the line names the job's process");
    let gone = (0..100).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        !cf_e2e::process::is_alive(job)
    });
    assert!(gone, "the job {job} is left running: {text}");
    Ok(())
}

#[test]
fn a_run_that_expects_the_limit_of_a_job_the_close_ends_holds_it_to_the_ending_and_lets_nothing_pass(
) -> Result {
    // The expectation lets one shape pass. A stand-in that leaves its job in a shell that waits
    // for it, as Claude Code's shell tool does, is ended by the window's close and the chief is
    // told, and the run says so: the same checks as for a harness that is expected to end it.
    let log = Log::new("live finish test");
    let plan = leaving::plan(Who::StandIn);
    let brief = leaving::stand_in_brief(false);
    let finished = leaving::run_asked(
        Who::StandIn,
        &plan,
        Path::new(FAKE_AGENT),
        &brief,
        leaving::Expect::OwnSession,
        &log,
    )?;
    let text = finished.case.text();
    assert!(finished.case.held(), "{text}");
    assert!(
        text.contains(
            "ok   the command was ended by the window's close after the result, and the chief told: T-1 done "
        ),
        "{text}"
    );
    assert!(!text.contains("the known limit on a Mac"), "{text}");
    assert!(!text.contains("FAIL"), "{text}");
    Ok(())
}

#[test]
fn a_stand_in_that_leaves_a_job_running_and_answers_has_its_task_done_and_its_windows_close_ends_the_job_and_tells_the_chief(
) -> Result {
    let log = Log::new("live finish test");
    let plan = leaving::plan(Who::StandIn);
    let finished = leaving::run(Who::StandIn, &plan, Path::new(FAKE_AGENT), &log)?;
    let text = finished.case.text();
    assert!(finished.case.held(), "{text}");
    assert!(
        text.contains(
            "ok   the command was ended by the window's close after the result, and the chief told: T-1 done "
        ),
        "{text}"
    );
    // The host listed the job as running while the turn was watched, with the command line
    // it was started with, which holds the number of seconds the check looks for.
    assert!(
        text.contains(&format!("--background-job {} ", leaving::SECONDS)),
        "{text}"
    );
    // The chief was told what the window closed with, in the note of its close: the turn's
    // end ended nothing, and the close found the job.
    assert!(
        text.contains(
            "what the chief was told of work the turn left:\n    T-1's window closed with background work running, which its close ended: "
        ),
        "{text}"
    );
    assert!(
        !text.contains("(its pane.exit): none"),
        "the close found the job the turn left: {text}"
    );
    assert!(!text.contains("FAIL"), "{text}");
    assert!(
        finished.summary.starts_with("T-1 done after "),
        "{}",
        finished.summary
    );
    Ok(())
}
