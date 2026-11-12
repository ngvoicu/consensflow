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

/// A stand-in that detaches its job from its shell, run on the project `plan` opens, held to what
/// four harnesses' agents did (`nohup … &` in a shell that then ended): nothing proves whose the job
/// is, so the host leaves it running, and the chief is told nothing. The run fails, honestly, and
/// says that the window's close did not find it either (the job is in a session of its own, with no
/// marker), and it ends it.
#[cfg(unix)]
fn the_job_escapes_the_ending_and_the_run_says_so_and_ends_it(plan: &Plan) -> Result {
    let log = Log::new("live finish test");
    let brief = leaving::stand_in_brief(true);
    let finished = leaving::run_asked(
        Who::StandIn,
        plan,
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
    let job = process_after(&text, said)
        .ok_or_else(|| cf_e2e::Error::Daemon(format!("the line names no job's process: {text}")))?;
    let gone = (0..100).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        !cf_e2e::process::is_alive(job)
    });
    assert!(gone, "the job {job} is left running: {text}");
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_stand_in_that_detaches_its_job_from_its_shell_escapes_the_ending_and_the_run_says_so_and_ends_it(
) -> Result {
    // A look of the run once a second may catch the job while its shell still runs, which is
    // a third of a second: the failure is said the same as of a command the host listed then,
    // or as one it never listed (CI run 42425306008). The case below makes the look fall in it.
    the_job_escapes_the_ending_and_the_run_says_so_and_ends_it(&leaving::plan(Who::StandIn))
}

/// The same escape where the host's look falls on the shell before it has started the job, which
/// it waits 2.5 s to do, so that the host, which looks once a second, lists the shell every time
/// (its own command line holds the job's words) and nothing else of the command: what it listed
/// ended with the shell, and the job the shell started then ran on in a session of its own, which
/// the host never listed (CI run 42425306008).
#[cfg(unix)]
#[test]
fn a_stand_in_that_detaches_its_job_after_the_host_listed_its_shell_alone_escapes_the_ending_too_and_the_run_says_so_and_ends_it(
) -> Result {
    let plan = leaving::plan(Who::StandIn).with_var("CF_TEST_DETACH_DELAY_MS", "2500");
    the_job_escapes_the_ending_and_the_run_says_so_and_ends_it(&plan)
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
    // The harness of the plants keeps the end of a long failure: what went wrong comes last.
    assert!(
        finished.case.held(),
        "{text}\n\nthe first thing wrong: {}",
        finished.summary
    );
    assert!(
        text.contains("the known limit on a Mac"),
        "the run says what it let pass: {text}"
    );
    assert!(
        text.contains(
            "ok   the command ran on in a session of its own, as the known limit on a Mac says it may, and the chief was told nothing: T-1 done "
        ),
        "{text}"
    );
    assert!(
        text.contains("what the chief was told of work the turn left: nothing"),
        "{text}"
    );
    assert!(text.contains(", terminal ??)"), "{text}");
    assert!(!text.contains("FAIL"), "{text}");
    // The run found the job in its own folder: an orphan that began after the window.
    assert!(
        text.contains("the run found what the agent detached in its own folder"),
        "{text}"
    );
    // It ended the job: nothing of the run's is left on the machine for what its seconds are.
    let job = process_after(&text, "still running: ").expect("the line names the job's process");
    let gone = (0..100).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        !cf_e2e::process::is_alive(job)
    });
    assert!(gone, "the job {job} is left running: {text}");
    Ok(())
}

/// The same limit where the host's look falls on the shell before it has started the job, which
/// it waits 2.5 s to do: the host lists the shell alone, every time, and the shell has ended when the
/// run looks again. Nothing it listed is there then, and the job it started ran on in a session of
/// its own that the close did not find: the limit, as where the look fell later (CI run 42425306008).
#[cfg(unix)]
#[test]
fn a_stand_in_that_plays_pi_on_a_mac_and_detaches_its_job_after_the_host_listed_its_shell_alone_holds_the_case_as_the_known_limit_too(
) -> Result {
    let log = Log::new("live finish test");
    let plan = leaving::plan(Who::StandIn).with_var("CF_TEST_DETACH_DELAY_MS", "2500");
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
    assert!(
        finished.case.held(),
        "{text}\n\nthe first thing wrong: {}",
        finished.summary
    );
    assert!(
        text.contains("the host listed it before its shell ended"),
        "the host listed the shell, and the limit took the job as one it listed: {text}"
    );
    assert!(
        text.contains(
            "ok   the command ran on in a session of its own, as the known limit on a Mac says it may, and the chief was told nothing: T-1 done "
        ),
        "{text}"
    );
    assert!(text.contains(", terminal ??)"), "{text}");
    assert!(!text.contains("FAIL"), "{text}");
    let job = process_after(&text, "still running: ").expect("the line names the job's process");
    let gone = (0..100).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        !cf_e2e::process::is_alive(job)
    });
    assert!(gone, "the job {job} is left running: {text}");
    Ok(())
}

/// The same limit where the host's look falls while the job's shell still ties it to the window:
/// the host lists the job, and then the shell ends and leaves it in a session of its own, which
/// the close does not find. On GitHub's macOS runner a look fell inside the stand-in's 0.3 s
/// (CI runs 41772376690 and 41781145403); here the shell lingers 2.5 s, so that the host, which
/// looks once a second, lists it every time. Where the job is after the close decides, not
/// whether a look saw it on its way out.
#[cfg(unix)]
#[test]
fn a_detached_job_the_host_listed_before_its_shell_ended_holds_the_case_as_the_known_limit_too(
) -> Result {
    let log = Log::new("live finish test");
    let plan = leaving::plan(Who::StandIn).with_var("CF_TEST_DETACH_LINGER_MS", "2500");
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
    // The widened gap did what it is for: the host listed the job, and the run held it to the
    // limit as one it listed. (The line of what the host listed is cut, and a runner's longer
    // paths cut it before the job's words: CI run 41787972093.)
    assert!(
        text.contains("the host listed it before its shell ended"),
        "the host listed the job, and the limit took it as one it listed: {text}"
    );
    assert!(
        finished.case.held(),
        "{text}\n\nthe first thing wrong: {}",
        finished.summary
    );
    assert!(
        text.contains(
            "ok   the command ran on in a session of its own, as the known limit on a Mac says it may, and the chief was told nothing: T-1 done "
        ),
        "{text}"
    );
    assert!(text.contains(", terminal ??)"), "{text}");
    assert!(!text.contains("FAIL"), "{text}");
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
    // The chief's note names the work by its number: the stand-in's job is no program a note
    // names, and what it was started with is never said (the command lines the host listed are
    // the report's, cut to a line).
    assert!(text.contains("which its close ended: process "), "{text}");
    assert!(!text.contains("which its close ended: python"), "{text}");
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

/// What a case's close finds of what its windows left, with the stand-in that detaches its job
/// from the shell that ran it, which leaves it with no parent in a session of its own that the
/// window's close does not find (the shape of a Monitor's pipeline left by a Claude that ended:
/// `CLAUDE-MONITOR-ENDS` leaves its job so too, but the window's own close after the result ends
/// that one on a Mac, whether the case looks first or not, so it cannot be waited for): the case's
/// close, which the live kit makes every case's, finds it by its folder and its start and says so,
/// and ends nothing (the test ends the job, by its own number); and what runs elsewhere with the
/// same words is not found.
#[cfg(unix)]
#[test]
fn the_close_of_a_case_finds_what_its_window_left_says_so_and_ends_nothing_and_leaves_another_runs_job(
) -> Result {
    use std::time::Duration;

    use cf_e2e::live::members::{find, Machine, Members, ThisMachine};

    let log = Log::new("live finish test");
    let plan = leaving::plan(Who::StandIn);
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), &log)?;
    members.add("worker", &[])?;
    // Another run's job: parent init's, the same words, a folder of its own. Told by the number
    // its shell printed, and ended by which process it is however the test ends: another tree
    // running this test has a `sleep 4713` of its own, which is none of this one's.
    let elsewhere = tempfile::tempdir().unwrap();
    let ran = cf_e2e::process::Run::new("/bin/sh")
        .args(["-c", "sleep 4713 > /dev/null 2>&1 & echo $!"])
        .cwd(elsewhere.path())
        .inheriting_env()
        .run()?;
    assert_eq!(ran.code, Some(0));
    let mut decoy = Decoy {
        pid: ran.stdout.trim().parse().unwrap(),
        command: "sleep 4713".to_owned(),
        read: None,
    };
    decoy.read = ThisMachine
        .processes()?
        .into_iter()
        .find(|one| one.identity.pid == decoy.pid && one.command == decoy.command);
    assert!(decoy.read.is_some(), "the other run's job was never listed");
    members.give("worker", &leaving::stand_in_brief(true))?;
    // The job the agent detached, an orphan the host lists nowhere, as the process it is: the
    // test's to end, however it ends.
    let job = find(
        &log,
        "the detached job to run with no parent",
        Duration::from_secs(120),
        Duration::from_millis(250),
        || {
            let Some(job) = members.rig().jobs("job").into_iter().next() else {
                return Ok(None);
            };
            Ok(ThisMachine
                .processes()?
                .into_iter()
                .find(|one| one.identity.pid == job.pid() && one.ppid == 1))
        },
    )?
    .expect("the stand-in detached its job");
    let job = Decoy {
        pid: job.identity.pid,
        command: job.command.clone(),
        read: Some(job),
    };
    let (closed, swept) = members.close_and_sweep(Duration::from_secs(20));
    let found: Vec<u32> = swept.found.iter().map(|one| one.identity.pid).collect();
    let said = swept.said();
    std::thread::sleep(Duration::from_secs(1));
    assert!(
        found.contains(&job.pid),
        "the job {} was not found: {said}",
        job.pid
    );
    assert!(said.contains(&format!("{} ", job.pid)), "{said}");
    assert!(said.contains("the kit ended none of them"), "{said}");
    assert!(job.runs(), "the sweep ended the job {}: {said}", job.pid);
    assert!(
        !found.contains(&decoy.pid),
        "another run's job is taken for the window's: {found:?} {}",
        decoy.pid
    );
    assert!(swept.problems().is_empty(), "{said}");
    assert!(closed.is_ok(), "{closed:?}");
    Ok(())
}

/// A process of the test's own that is none of the run's: `tail -f <file>`, which its shell left
/// with no parent and whose number it printed, so that its command line holds the path of `file`.
/// Ended when the test ends however it does: as the process it was read as, or, where no read
/// came first, the process of that number and that command line begun no earlier than the test.
#[cfg(unix)]
struct Decoy {
    pid: u32,
    command: String,
    read: Option<cf_e2e::live::members::Running>,
}

#[cfg(unix)]
impl Decoy {
    /// Started in `folder` by a shell of the test's own, which ends at once; armed before
    /// anything that can fail is read.
    fn start(file: &Path, folder: &Path) -> Result<Self> {
        use cf_e2e::live::members::{Machine, ThisMachine};

        let ran = cf_e2e::process::Run::new("/bin/sh")
            .args(["-c", "tail -f \"$1\" > /dev/null 2>&1 & echo $!", "sh"])
            .arg(file)
            .cwd(folder)
            .inheriting_env()
            .run()?;
        assert_eq!(ran.code, Some(0), "{ran}");
        let pid = ran.stdout.trim().parse::<u32>();
        assert!(pid.is_ok(), "the decoy's shell printed no pid: {ran}");
        let mut decoy = Self {
            pid: pid.unwrap_or_default(),
            command: format!("tail -f {}", file.display()),
            read: None,
        };
        decoy.read = ThisMachine
            .processes()?
            .into_iter()
            .find(|one| one.identity.pid == decoy.pid && one.command == decoy.command);
        assert!(
            decoy.read.is_some(),
            "the decoy {} was never listed",
            decoy.pid
        );
        Ok(decoy)
    }

    /// Whether it still runs, as the process it was read as.
    fn runs(&self) -> bool {
        use cf_e2e::live::members::{Machine, ThisMachine};

        self.read.as_ref().is_some_and(|read| {
            ThisMachine
                .processes()
                .unwrap_or_default()
                .iter()
                .any(|one| one.identity == read.identity)
        })
    }
}

#[cfg(unix)]
impl Drop for Decoy {
    fn drop(&mut self) {
        use cf_e2e::door::{Door, Grounded};
        use cf_e2e::live::members::{Machine, ThisMachine};

        let unread = || {
            let all = ThisMachine.processes().ok()?;
            Grounded::started_here(&all, self.pid, &self.command)
        };
        let decoy = match self.read.take() {
            Some(read) => Grounded::read_here(&read),
            None => unread(),
        };
        if let Some(decoy) = decoy {
            let _ = Door::of_run().end(&decoy);
        }
    }
}

/// A live rig's home is the person's own (the windows of a real harness run on it), outside the
/// rig's own temp root: at 18:58 on 2026-11-12 the close's sweep took `/Users/…` for a word of the
/// run and ended 22 processes of the owner's and of other tasks. Here the home is a folder of the
/// test's own, and a process of the test's, begun before the run's windows, with no parent, holds
/// its path: it is none of what the windows left, and it runs on.
#[cfg(unix)]
#[test]
fn a_process_begun_before_the_run_that_holds_the_path_of_a_home_outside_the_runs_temp_root_is_none_of_what_the_windows_left_and_runs_on(
) -> Result {
    use std::time::Duration;

    use cf_e2e::live::members::Members;

    let log = Log::new("live finish test");
    let home = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let file = home.path().join("decoy");
    std::fs::write(&file, "").unwrap();
    let decoy = Decoy::start(&file, elsewhere.path())?;
    let mut plan = leaving::plan(Who::StandIn);
    plan.vars
        .push(("HOME".to_owned(), home.path().display().to_string()));
    let members = Members::open(&plan, Path::new(FAKE_AGENT), &log)?;
    assert_eq!(
        members.rig().var("HOME"),
        Some(home.path().display().to_string().as_str()),
        "the rig's home is the test's own folder"
    );
    let (closed, swept) = members.close_and_sweep(Duration::from_secs(20));
    std::thread::sleep(Duration::from_secs(1));
    let said = swept.said();
    assert!(
        swept.found.iter().all(|one| one.identity.pid != decoy.pid),
        "a process that holds the path of a home outside the run's temp root is taken for what the windows left: {said}"
    );
    assert!(decoy.runs(), "the decoy {} was ended: {said}", decoy.pid);
    assert!(closed.is_ok(), "{closed:?}");
    Ok(())
}

/// A process of the test's own, begun after the run's window, with no parent, that holds a path
/// under the rig's own temp root (its home's): what the windows left, as the close's sweep says
/// it, and not ended by it: the sweep reports, and the test ends it by its own number.
#[cfg(unix)]
#[test]
fn a_process_begun_after_the_window_that_holds_a_path_under_the_runs_temp_root_is_said_to_be_left_and_the_sweep_ends_nothing(
) -> Result {
    use std::time::Duration;

    use cf_e2e::live::members::Members;

    let log = Log::new("live finish test");
    let members = Members::open(&leaving::plan(Who::StandIn), Path::new(FAKE_AGENT), &log)?;
    let elsewhere = tempfile::tempdir().unwrap();
    let file = Path::new(members.rig().var("HOME").expect("the rig has a home")).join("decoy");
    std::fs::write(&file, "").unwrap();
    let decoy = Decoy::start(&file, elsewhere.path())?;
    let (closed, swept) = members.close_and_sweep(Duration::from_secs(20));
    std::thread::sleep(Duration::from_secs(1));
    let said = swept.said();
    assert!(
        swept.found.iter().any(|one| one.identity.pid == decoy.pid),
        "the decoy {} is not said to be left: {said}",
        decoy.pid
    );
    assert!(said.contains(&format!("{} ", decoy.pid)), "{said}");
    assert!(
        decoy.runs(),
        "the sweep ended the decoy {}: {said}",
        decoy.pid
    );
    assert!(closed.is_ok(), "{closed:?}");
    Ok(())
}
