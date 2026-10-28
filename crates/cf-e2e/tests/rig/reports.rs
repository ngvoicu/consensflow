//! A rig tells of a case that failed, and of no case that passed. A rig dropped
//! unclosed is one its case left on the way: by a check that failed (a panic), by an
//! error it returned (a case is a `Result`, and the wait that timed out ends most of
//! the failed ones that way, with no panic in flight when the rig is dropped), or by
//! a close that failed (the rig is closed once a close succeeded, and not before).
//! It keeps what its windows showed in a file beside its home (`<home>-left.txt`,
//! `rig/report.rs`) for the person who reads the failure, and says where in the
//! output of that case, and which case it was. A case that passes ends by closing its
//! rig, and the rig has nothing to tell of it; one that passes and forgets the close
//! has nothing to tell either. A rig whose case returns `cf_e2e::Outcome`
//! (`Config::judged_by_outcome`) writes its report as it is dropped, while it is up, and
//! says it once the case is known to have failed: the case's `?` made a `Failed` (before
//! the drop, or after it, for a rig a helper owned that returned its error), or a check
//! panicked. A drop that no failure follows on its thread was a case that passed, and
//! says nothing. A rig whose case returns nothing of the kind (the live suites', a run's)
//! cannot tell and says its report for every drop unclosed. A case that holds its close to
//! fail says so
//! (`Rig::expect_the_close_to_fail`), and a failure it holds to is no report either.
//! The report of a rig built for the real harnesses (`live::rig`), whose windows
//! show what a real harness drew with the person's login, is kept under the
//! checkout's build folder instead, where a clean takes it away; CI uploads that folder's
//! for a failed job, since ten cases of the rig suite build on `live::rig` with the
//! stand-in, and no real harness runs there.
//!
//! No report is kept of a hang that a step's limit or the job's kills: the process
//! that would drop the rig is ended with it, and a job that is cancelled is not
//! `failure()` either, so nothing is uploaded. Most hangs are ended before, by a
//! case's own wait: its timeout is an error the case returns, which does keep one.
//!
//! CI's run of 2026-10-21 said it "kept in" fourteen files for as many cases that
//! had passed: thirteen of the folder-trust regression's and the follow-up's one
//! ended without closing their rigs. Those cases close them now, and are held to it
//! here, as they are written, for every suite's file (the live ones' too, whose rigs cannot
//! tell a pass from a failure): each function that starts a rig, or calls a helper of its
//! file that hands one on, closes it. What that reads is text, and it does not see: a
//! rig inside another type (`Members`, which the live members' cases close through
//! `members.close()`, a close it counts but whose start it does not), a helper called as
//! a method (a name after a dot is none of the helpers'), and a function whose head its
//! pattern does not match (`async`, `const`, in a trait: the suites have none). A
//! case that goes outside these takes its own care, and the rig does not report it.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::process::{signal, Run, Signal};
use cf_e2e::rig::{Config, Rig};
use cf_e2e::{checkout, live};
use regex::Regex;

use crate::Outcome;

/// Where the rig whose home is `home` keeps its report.
fn report_beside(home: &Path) -> PathBuf {
    let name = home.file_name().unwrap_or_default().to_string_lossy();
    home.with_file_name(format!("{name}-left.txt"))
}

/// The home of `rig`.
fn home_of(rig: &Rig) -> PathBuf {
    let workspace = rig.workspace();
    workspace.parent().unwrap_or(workspace).to_path_buf()
}

/// A case that fails as most do: it waits for what no window does, and the
/// timeout ends it with an error it returns, its rig still up. `home` is where
/// its rig's home was.
fn a_case_that_waits_for_what_never_comes(home: &mut Option<PathBuf>) -> Outcome {
    let rig = crate::rig()?;
    *home = Some(home_of(&rig));
    rig.wait_for("what no window does", Duration::from_millis(200), || {
        Ok(false)
    })?;
    rig.close()?;
    Ok(())
}

/// A case that holds its close to fail, and fails first: the wait for what never
/// comes ends it, with an error, before it closes the rig.
fn a_case_that_waits_for_what_never_comes_expecting_its_close_to_fail(
    home: &mut Option<PathBuf>,
) -> Outcome {
    let rig = crate::rig()?;
    *home = Some(home_of(&rig));
    rig.expect_the_close_to_fail();
    rig.wait_for("what no window does", Duration::from_millis(200), || {
        Ok(false)
    })?;
    rig.close()?;
    Ok(())
}

/// A case that ends as every passing one does, by closing its rig, and whose close
/// fails: the process that is none of any window's is ended before the rig is, which
/// the close finds (the rig's own check of what the pane host ends).
fn a_case_whose_close_fails(home: &mut Option<PathBuf>, expected: bool) -> Outcome {
    let rig = crate::rig()?;
    *home = Some(home_of(&rig));
    if expected {
        rig.expect_the_close_to_fail();
    }
    signal(rig.stranger_pid(), Signal::Kill)?;
    // The daemon and the pane host end first, which takes longer than the signal does.
    std::thread::sleep(Duration::from_secs(1));
    rig.close()?;
    Ok(())
}

/// A case whose close times out: the daemon and the pane host are given no time to end.
/// On Unix the daemon is stopped first (SIGSTOP), so that it is there, its input gone,
/// when the rig is dropped and its report is made: a daemon left running may end of its
/// input's end before that, and the report then has nothing to ask. The drop kills it.
fn a_case_whose_close_times_out(home: &mut Option<PathBuf>) -> Outcome {
    let rig = crate::rig()?;
    *home = Some(home_of(&rig));
    #[cfg(unix)]
    {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;
        kill(
            Pid::from_raw(i32::try_from(rig.daemon_pid())?),
            Signal::SIGSTOP,
        )?;
    }
    rig.close_within(Duration::ZERO)?;
    Ok(())
}

/// A case whose check fails after it shut its rig down to look at what was left
/// (`shut_down_within` keeps the rig, which is the case's still): it panics.
fn a_case_whose_check_fails_after_a_shutdown(home: &mut Option<PathBuf>) {
    let Ok(mut rig) = crate::rig() else {
        panic!("no rig to fail with");
    };
    *home = Some(home_of(&rig));
    assert!(rig.shut_down_within(Duration::from_secs(10)).is_ok());
    panic!("a check of the case that does not hold");
}

/// A case that passes and forgets to close its rig: the rig is dropped as the case
/// returns. `home` is where its rig's home was.
fn a_case_that_passes_without_closing_its_rig(home: &mut Option<PathBuf>) -> Outcome {
    let rig = crate::rig()?;
    *home = Some(home_of(&rig));
    Ok(())
}

/// A step of a case that owns a rig and returns the error that ends it, which is no
/// `Failed` yet: the rig is dropped as the step returns, before the case's `?` sees it.
fn a_step_whose_rig_is_dropped_by_its_own_error(home: &mut Option<PathBuf>) -> cf_e2e::Result<()> {
    let rig = crate::rig()?;
    *home = Some(home_of(&rig));
    rig.wait_for("what no window does", Duration::from_millis(200), || {
        Ok(false)
    })?;
    rig.close()?;
    Ok(())
}

/// A case that returns the error of such a step.
fn a_case_that_returns_the_error_of_its_step(home: &mut Option<PathBuf>) -> Outcome {
    a_step_whose_rig_is_dropped_by_its_own_error(home)?;
    Ok(())
}

/// A case that fails a check on the error of such a step.
fn a_case_that_fails_a_check_on_the_error_of_its_step(home: &mut Option<PathBuf>) {
    assert!(
        a_step_whose_rig_is_dropped_by_its_own_error(home).is_ok(),
        "the step ended well"
    );
}

/// A case that goes on from the error of such a step, and passes.
fn a_case_that_goes_on_from_the_error_of_its_step(home: &mut Option<PathBuf>) -> Outcome {
    let went_on = a_step_whose_rig_is_dropped_by_its_own_error(home);
    assert!(went_on.is_err(), "the step ended well");
    Ok(())
}

/// A case that fails by a check with its rig up, never closed or shut down.
fn a_case_whose_check_fails_with_its_rig_up(home: &mut Option<PathBuf>) {
    let Ok(rig) = crate::rig() else {
        panic!("no rig to fail with");
    };
    *home = Some(home_of(&rig));
    panic!("a check of the case that does not hold");
}

/// A case that passes and forgets to close its rig, which is not judged by what the
/// case returns (it is started as a live suite's is, which returns another kind of
/// result).
fn a_case_that_passes_without_closing_a_rig_that_is_not_judged(
    home: &mut Option<PathBuf>,
) -> Outcome {
    let rig = Rig::start(Config::new(crate::FAKE_AGENT))?;
    *home = Some(home_of(&rig));
    Ok(())
}

/// A case that fails as most do, its rig given the name of a case that is no test's.
fn a_case_given_a_name_that_waits_for_what_never_comes(home: &mut Option<PathBuf>) -> Outcome {
    let rig = Rig::start(crate::config().case("a run that is no test"))?;
    *home = Some(home_of(&rig));
    rig.wait_for("what no window does", Duration::from_millis(200), || {
        Ok(false)
    })?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_rig_dropped_unclosed_by_a_case_that_passed_keeps_no_report() -> Outcome {
    let mut home = None;

    let passed = a_case_that_passes_without_closing_its_rig(&mut home);

    assert!(passed.is_ok(), "the case did not pass");
    let home = home.ok_or("the case never started its rig")?;
    assert!(
        !report_beside(&home).exists(),
        "a report was kept of a case that passed and did not close its rig"
    );
    assert!(!home.exists(), "the home of a rig that was dropped is gone");
    Ok(())
}

#[test]
fn a_rig_a_step_dropped_on_an_error_keeps_its_report_once_the_case_returns_the_error() -> Outcome {
    let mut home = None;

    let failed = a_case_that_returns_the_error_of_its_step(&mut home);

    assert!(failed.is_err(), "the wait for what never comes ended well");
    let report = report_beside(&home.ok_or("the case never started its rig")?);
    let kept = fs::read_to_string(&report)?;
    assert!(kept.contains(HEADING), "{kept}");
    fs::remove_file(report)?;
    Ok(())
}

#[test]
fn a_rig_a_step_dropped_on_an_error_keeps_its_report_once_the_case_fails_a_check_on_it() -> Outcome
{
    let mut home = None;

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        a_case_that_fails_a_check_on_the_error_of_its_step(&mut home);
    }));

    assert!(caught.is_err(), "the check held");
    let report = report_beside(&home.ok_or("the case never started its rig")?);
    let kept = fs::read_to_string(&report)?;
    assert!(kept.contains(HEADING), "{kept}");
    fs::remove_file(report)?;
    Ok(())
}

#[test]
fn a_rig_a_step_dropped_on_an_error_the_case_went_on_from_keeps_no_report() -> Outcome {
    let mut home = None;

    let passed = a_case_that_goes_on_from_the_error_of_its_step(&mut home);

    assert!(passed.is_ok(), "the case did not pass");
    let report = report_beside(&home.ok_or("the case never started its rig")?);
    assert!(
        !report.exists(),
        "a report was kept of a case that went on from the error of its step"
    );
    Ok(())
}

#[test]
fn a_rig_started_after_a_case_failed_on_its_thread_is_judged_by_its_own_case() -> Outcome {
    // An error the thread returned before this rig was started is none of its failure.
    let mut failed_home = None;
    assert!(a_case_that_waits_for_what_never_comes(&mut failed_home).is_err());
    fs::remove_file(report_beside(
        &failed_home.ok_or("no rig for the failed case")?,
    ))?;
    let mut home = None;

    let passed = a_case_that_passes_without_closing_its_rig(&mut home);

    assert!(passed.is_ok(), "the case did not pass");
    let report = report_beside(&home.ok_or("the case never started its rig")?);
    assert!(
        !report.exists(),
        "a report was kept of a case that passed after another had failed on the thread"
    );
    Ok(())
}

#[test]
fn a_rig_dropped_unclosed_while_a_check_fails_keeps_its_report() -> Outcome {
    let mut home = None;

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        a_case_whose_check_fails_with_its_rig_up(&mut home);
    }));

    assert!(caught.is_err(), "the check held");
    let report = report_beside(&home.ok_or("the case never started its rig")?);
    let kept = fs::read_to_string(&report)?;
    assert!(kept.contains(HEADING), "{kept}");
    fs::remove_file(report)?;
    Ok(())
}

#[test]
fn a_rig_not_judged_by_what_its_case_returns_keeps_its_report_for_every_drop_unclosed() -> Outcome {
    let mut home = None;

    let passed = a_case_that_passes_without_closing_a_rig_that_is_not_judged(&mut home);

    assert!(passed.is_ok(), "the case did not pass");
    let report = report_beside(&home.ok_or("the case never started its rig")?);
    let kept = fs::read_to_string(&report)?;
    assert!(kept.contains(HEADING), "{kept}");
    fs::remove_file(report)?;
    Ok(())
}

#[test]
fn a_report_names_the_test_that_left_it_and_the_case_a_rig_is_given() -> Outcome {
    // On a thread named as the test harness names a test's.
    let named = std::thread::Builder::new()
        .name("a_suite::a_case_that_fails".to_owned())
        .spawn(|| {
            let mut home = None;
            let failed = a_case_that_waits_for_what_never_comes(&mut home);
            (failed.is_err(), home)
        })?
        .join()
        .map_err(|_| "the named thread panicked")?;
    let (failed, home) = named;
    assert!(failed, "the wait for what never comes ended well");
    let report = report_beside(&home.ok_or("the case never started its rig")?);
    let kept = fs::read_to_string(&report)?;
    fs::remove_file(report)?;
    assert!(
        kept.contains("case: a_suite::a_case_that_fails\n"),
        "the report does not name its test: {kept}"
    );

    let mut home = None;
    let failed = a_case_given_a_name_that_waits_for_what_never_comes(&mut home);
    assert!(failed.is_err(), "the wait for what never comes ended well");
    let report = report_beside(&home.ok_or("the case never started its rig")?);
    let kept = fs::read_to_string(&report)?;
    fs::remove_file(report)?;
    assert!(
        kept.contains("case: a run that is no test\n"),
        "the report does not name the case its rig was given: {kept}"
    );
    Ok(())
}

#[test]
fn a_rig_its_case_closed_leaves_no_report() -> Outcome {
    let rig = crate::rig()?;
    let home = home_of(&rig);

    rig.close()?;

    assert!(
        !report_beside(&home).exists(),
        "a report was kept of a case that closed its rig"
    );
    assert!(!home.exists(), "the home of a rig that closed is gone");
    Ok(())
}

/// What every report starts with.
const HEADING: &str = "--- the rig, as the failed case left it";

#[test]
fn a_rig_its_case_ends_as_a_drop_on_purpose_leaves_no_report() -> Outcome {
    // A case about what a drop does (live-keychain's: the Claude config of a rig a failed
    // case dropped is asked what the run came to), merged with the rule that a case that
    // passes leaves no report (T-501). Its rig is not judged by what the case returns: an
    // unjudged rig keeps its report for every drop unclosed, a case that passed included,
    // so only a drop on purpose that is honoured leaves none here (a judged rig of a case
    // that passes leaves none whatever, T-503).
    let rig = Rig::start(Config::new(crate::FAKE_AGENT))?;
    let home = home_of(&rig);

    rig.close_as_dropped();

    assert!(
        !report_beside(&home).exists(),
        "a report was kept of a rig its case dropped on purpose"
    );
    assert!(
        !home.exists(),
        "the home of a rig dropped on purpose is gone"
    );
    Ok(())
}

/// A case about what a drop does that ends its rig as a drop on purpose and then fails,
/// as most do, by the error of a step it returns: the rig is judged by what its case
/// returns, and its case has failed by the time the error is made.
fn a_case_that_drops_on_purpose_and_then_returns_an_error(home: &mut Option<PathBuf>) -> Outcome {
    let rig = crate::rig()?;
    *home = Some(home_of(&rig));
    rig.close_as_dropped();
    Err(std::io::Error::other("what the case found after the drop"))?;
    Ok(())
}

#[test]
fn a_rig_its_case_ends_as_a_drop_on_purpose_keeps_no_report_though_the_case_fails_after() -> Outcome
{
    // The rule that a rig dropped unclosed keeps its report once its case has failed
    // (T-428) meets the one that a drop on purpose keeps none (T-501): as with a close the
    // case holds to fail (`expect_the_close_to_fail`), what the case finds after is the
    // case's to say, in its error.
    let mut home = None;

    let failed = a_case_that_drops_on_purpose_and_then_returns_an_error(&mut home);

    assert!(failed.is_err(), "the step ended well");
    let home = home.ok_or("the case never started its rig")?;
    assert!(
        !report_beside(&home).exists(),
        "a report was kept of a rig its case dropped on purpose, once the case failed"
    );
    Ok(())
}

#[test]
fn a_rig_its_case_left_on_an_error_it_returned_keeps_its_report() -> Outcome {
    let mut home = None;

    let failed = a_case_that_waits_for_what_never_comes(&mut home);

    assert!(failed.is_err(), "the wait for what never comes ended well");
    let report = report_beside(&home.ok_or("the case never started its rig")?);
    let kept = fs::read_to_string(&report)?;
    assert!(kept.contains(HEADING), "{kept}");
    fs::remove_file(report)?;
    Ok(())
}

#[test]
fn a_rig_whose_close_fails_keeps_its_report() -> Outcome {
    let mut home = None;

    let failed = a_case_whose_close_fails(&mut home, false);

    assert!(
        failed.is_err(),
        "the close of a rig whose stranger was ended ended well"
    );
    let report = report_beside(&home.ok_or("the case never started its rig")?);
    assert!(report.exists(), "a close that failed kept no report");
    let kept = fs::read_to_string(&report)?;
    assert!(kept.contains(HEADING), "{kept}");
    fs::remove_file(report)?;
    Ok(())
}

#[test]
fn a_report_of_a_rig_whose_close_was_tried_does_not_ask_the_daemon_what_it_says() -> Outcome {
    let mut home = None;

    let failed = a_case_whose_close_times_out(&mut home);

    assert!(failed.is_err(), "a close given no time ended well");
    let report = report_beside(&home.ok_or("the case never started its rig")?);
    let kept = fs::read_to_string(&report)?;
    fs::remove_file(report)?;
    // Its input gone, the daemon is asked nothing (it would be waited on for ten
    // seconds where it is there and answers nothing). On Unix it is there, stopped, and
    // that is all the report may say; on Windows it may have ended of its input's end.
    let said = kept
        .split("what the daemon says:\n")
        .nth(1)
        .and_then(|after| after.lines().next())
        .unwrap_or_default();
    assert!(
        said.starts_with("  not asked: ") || (cfg!(windows) && said == "  the daemon is gone"),
        "{said:?}"
    );
    Ok(())
}

#[test]
fn a_rig_whose_case_holds_its_close_to_fail_keeps_no_report_of_the_failure() -> Outcome {
    let mut home = None;

    let failed = a_case_whose_close_fails(&mut home, true);

    assert!(failed.is_err(), "the close of the rig ended well");
    let report = report_beside(&home.ok_or("the case never started its rig")?);
    assert!(
        !report.exists(),
        "a report was kept of a failure the case held its close to"
    );
    Ok(())
}

#[test]
fn a_rig_whose_case_holds_its_close_to_fail_and_never_tries_it_keeps_its_report() -> Outcome {
    let mut home = None;

    let failed = a_case_that_waits_for_what_never_comes_expecting_its_close_to_fail(&mut home);

    assert!(failed.is_err(), "the wait for what never comes ended well");
    let report = report_beside(&home.ok_or("the case never started its rig")?);
    assert!(
        report.exists(),
        "a case that failed before its close kept no report"
    );
    fs::remove_file(report)?;
    Ok(())
}

/// Where the report of a rig built for the real harnesses is kept: under the
/// checkout's build folder, which no run uploads and a clean takes away.
const LIVE_REPORTS: &str = "app/src-tauri/target/live-reports";

/// A case on a rig built for the real harnesses (with none installed here: its
/// windows are the stand-in's), failing as most do.
fn a_live_rigs_case_that_waits_for_what_never_comes(home: &mut Option<PathBuf>) -> Outcome {
    let rig = Rig::start(live::rig(&[], crate::FAKE_AGENT))?;
    *home = Some(home_of(&rig));
    rig.wait_for("what no window does", Duration::from_millis(200), || {
        Ok(false)
    })?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_live_rigs_report_is_kept_under_the_checkouts_build_folder_and_not_in_the_temp_folder(
) -> Outcome {
    let mut home = None;

    let failed = a_live_rigs_case_that_waits_for_what_never_comes(&mut home);

    assert!(failed.is_err(), "the wait for what never comes ended well");
    let beside = report_beside(&home.ok_or("the case never started its rig")?);
    let kept = checkout::path(LIVE_REPORTS).join(beside.file_name().unwrap_or_default());
    let (under_target, in_temp) = (kept.exists(), beside.exists());
    // Taken away before anything is said of them.
    let _ = fs::remove_file(&kept);
    let _ = fs::remove_file(&beside);
    assert!(
        under_target,
        "a live rig's report is not in {}",
        kept.display()
    );
    assert!(
        !in_temp,
        "a live rig's report, which may hold a real harness's screens, is in the temp folder: {}",
        beside.display()
    );
    Ok(())
}

#[test]
fn a_rig_shut_down_and_dropped_while_a_check_fails_keeps_its_report() -> Outcome {
    let mut home = None;

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        a_case_whose_check_fails_after_a_shutdown(&mut home);
    }));

    assert!(caught.is_err(), "the check held");
    let report = report_beside(&home.ok_or("the case never started its rig")?);
    let kept = fs::read_to_string(&report)?;
    assert!(kept.contains(HEADING), "{kept}");
    fs::remove_file(report)?;
    Ok(())
}

/// The case the test binary is asked to run alone.
const CASE_ALONE: &str = "reports::a_rig_its_case_left_on_an_error_it_returned_keeps_its_report";

/// What the test binary says of one case it is asked to run alone: its output,
/// as `cargo test` shows it (`shown`: as `--nocapture` does).
fn output_of_the_case_alone(shown: bool) -> Result<String, Box<dyn std::error::Error>> {
    let mut asked = vec!["--exact", CASE_ALONE, "--test-threads=1"];
    if shown {
        asked.push("--nocapture");
    }
    let ran = Run::new(&std::env::current_exe()?)
        .args(asked)
        .inheriting_env()
        .run()?;
    assert_eq!(ran.code, Some(0), "{ran}");
    Ok(format!("{}{}", ran.stdout, ran.stderr))
}

#[test]
fn where_a_report_is_kept_is_said_in_the_output_of_the_case_that_failed_and_of_no_other() -> Outcome
{
    // The case above fails inside itself and passes: its report is the rig's own, said
    // as a failed case's would be, and a test run holds it unless it is asked for.
    let held = output_of_the_case_alone(false)?;
    assert!(!held.contains("is kept in"), "{held}");
    let shown = output_of_the_case_alone(true)?;
    assert!(
        shown.contains("the rig, as the failed case left it, is kept in"),
        "{shown}"
    );
    // And the line says which case left it, so that a log of many points at it.
    assert!(
        shown.contains(&format!("(case: {CASE_ALONE})")),
        "the line does not name its case: {shown}"
    );
    Ok(())
}

/// What starts a rig in a function: the rig's own `start`, and the suite's `rig()` (not a
/// `.rig()` of a project's members, which is one already started). A function that
/// expects the start to be refused, `let Err(…) = Rig::start(…)`, has none to close.
const STARTS: &str = r"\bRig::start\(|(?:^|[^.\w])rig\(\)";
const REFUSED: &str = r"let Err\(";

/// What ends one: a close of any of its kinds. (`shut_down_within` keeps the rig.)
const CLOSES: &str = r"\.close(?:_\w+)?\(";

/// The head of a function, and the type a signature names to hand a rig on.
const HEADER: &str = r"^\s*(?:pub(?:\([a-z]+\))?\s+)?fn (\w+)";
const RIG: &str = r"\bRig\b";

/// A function of a source file: its name, whether its return type names a `Rig` (it
/// hands one on, to a case that has it to close), whether its parameters name one too
/// (the rig it gives is the one it was given, ended and started again), and its lines
/// after the signature.
struct Function<'a> {
    name: String,
    hands_a_rig_on: bool,
    takes_a_rig: bool,
    body: Vec<&'a str>,
}

/// The functions of `text`, each from its `fn` to the line that closes it, which
/// rustfmt puts at the indent of the `fn`; the functions inside one are its own.
fn functions_of<'a>(text: &'a str, header: &Regex, rig: &Regex) -> Vec<Function<'a>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let Some(head) = header.captures(lines[at]) else {
            at += 1;
            continue;
        };
        let indent = lines[at].len() - lines[at].trim_start().len();
        let close = format!("{}}}", " ".repeat(indent));
        let end = (at..lines.len())
            .find(|&line| lines[line].starts_with(&close))
            .unwrap_or(lines.len() - 1);
        // The signature ends at the line that opens the body.
        let open = (at..=end)
            .find(|&line| lines[line].trim_end().ends_with('{'))
            .unwrap_or(at);
        let signature = lines[at..=open].join(" ");
        let (taken, returned) = signature.rsplit_once("->").unwrap_or((&signature, ""));
        found.push(Function {
            name: head[1].to_owned(),
            hands_a_rig_on: rig.is_match(returned),
            takes_a_rig: rig.is_match(taken),
            body: lines[open + 1..=end].to_vec(),
        });
        at = end + 1;
    }
    found
}

/// The source files under `folder`.
fn sources_in(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(sources_in(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
    found.sort();
    found
}

/// A function that starts rigs, and how many it starts and closes.
#[derive(Debug, PartialEq, Eq)]
struct Starting {
    name: String,
    started: usize,
    closed: usize,
}

/// Each function of `text` that starts a rig, with what it starts and what it closes.
/// A call of a helper of the text that hands a rig on starts one, as `Rig::start` does; a
/// helper that takes a rig and gives one back (the app quit and started again) starts
/// none, and a `let rig =` is no start.
fn startings_in(text: &str) -> Result<Vec<Starting>, Box<dyn std::error::Error>> {
    let (starts, refused, closes) = (
        Regex::new(STARTS)?,
        Regex::new(REFUSED)?,
        Regex::new(CLOSES)?,
    );
    let (header, rig) = (Regex::new(HEADER)?, Regex::new(RIG)?);
    let functions = functions_of(text, &header, &rig);
    let helpers = functions
        .iter()
        .filter(|function| function.hands_a_rig_on && !function.takes_a_rig)
        .map(|function| Regex::new(&format!(r"(?:^|[^.\w]){}\(", regex::escape(&function.name))))
        .collect::<Result<Vec<_>, _>>()?;
    let mut found = Vec::new();
    for function in functions.iter().filter(|function| !function.hands_a_rig_on) {
        let started = function
            .body
            .iter()
            .filter(|line| !refused.is_match(line))
            .map(|line| {
                starts.find_iter(line).count()
                    + helpers
                        .iter()
                        .map(|helper| helper.find_iter(line).count())
                        .sum::<usize>()
            })
            .sum::<usize>();
        let closed = function
            .body
            .iter()
            .map(|line| closes.find_iter(line).count())
            .sum::<usize>();
        if started > 0 {
            found.push(Starting {
                name: function.name.clone(),
                started,
                closed,
            });
        }
    }
    Ok(found)
}

#[test]
fn every_function_of_the_suites_that_starts_a_rig_closes_it_or_hands_it_on() -> Outcome {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut left = Vec::new();
    let mut starting = 0;
    for file in sources_in(&tests) {
        // This module's own cases leave their rigs unclosed, on purpose.
        if file.ends_with("reports.rs") {
            continue;
        }
        for function in startings_in(&fs::read_to_string(&file)?)? {
            starting += 1;
            if function.started > function.closed {
                left.push(format!(
                    "{}: {} starts {} rigs and closes {}",
                    file.strip_prefix(&tests).unwrap_or(&file).display(),
                    function.name,
                    function.started,
                    function.closed
                ));
            }
        }
    }

    assert!(
        starting > 100,
        "only {starting} functions that start a rig were found: the scan is blind"
    );
    assert!(
        left.is_empty(),
        "a case that passes ends by closing its rig, or the rig of a suite that cannot tell a pass \
         from a failure (a live one) keeps a report of a case that did not fail:\n{}",
        left.join("\n")
    );
    Ok(())
}

/// A `#[test]` function: its name, and what it returns where it returns something.
const CASE: &str = r"#\[test\][^\n]*\n(?:[ \t]*#\[[^\n]*\n)*[ \t]*(?:pub(?:\([a-z]+\))?\s+)?fn\s+(\w+)\s*\(\s*\)\s*(?:->\s*([^{]+?))?\s*\{";

#[test]
fn every_case_of_the_rig_and_the_regressions_returns_nothing_or_an_outcome() -> Outcome {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let case = Regex::new(CASE)?;
    let mut other = Vec::new();
    let mut found = 0;
    for folder in ["regressions", "rig"] {
        for file in sources_in(&tests.join(folder)) {
            let text = fs::read_to_string(&file)?;
            for found_case in case.captures_iter(&text) {
                found += 1;
                let returns = found_case
                    .get(2)
                    .map_or("", |returned| returned.as_str().trim());
                if !returns.is_empty() && returns != "Outcome" {
                    other.push(format!(
                        "{folder}/{}: {} returns {returns}",
                        file.file_name().unwrap_or_default().to_string_lossy(),
                        &found_case[1]
                    ));
                }
            }
        }
    }

    assert!(
        found > 200,
        "only {found} cases were found: the scan is blind"
    );
    assert!(
        other.is_empty(),
        "a case that fails by an error it returns tells its rig so only by returning an Outcome, \
         and the rig of any other keeps its report for a case that passed, or none for one that \
         failed:\n{}",
        other.join("\n")
    );
    Ok(())
}

#[test]
fn every_rig_the_suites_of_the_rig_the_regressions_and_the_load_start_is_judged_by_its_case(
) -> Outcome {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    for file in ["rig.rs", "regressions.rs", "load.rs"] {
        let text = fs::read_to_string(tests.join(file))?;
        let (made, judged) = (
            text.matches("Config::new(").count(),
            text.matches(".judged_by_outcome()").count(),
        );
        assert!(
            made > 0,
            "{file} makes no rig configuration: the scan is blind"
        );
        assert_eq!(
            made, judged,
            "{file} makes {made} rig configurations and judges {judged} by what their cases \
             return: a rig dropped unclosed by a case that passed keeps a report of it"
        );
    }
    Ok(())
}

/// A `#[cfg(...)]` in a suite's list of modules sits on the `mod` line it gates, after the
/// module's `#[path]`: put above the `#[path]`, a module added in its place in the list goes
/// between the two and takes the attribute (`4b5f3b1ca` moved the Unix-only attribute of
/// the Devin case onto the Codex case before it, and the Devin case went to Windows, where
/// its stand-in has no `/bin/sh`: Calliope's re-check of `a0aecd192`, T-854). And each
/// `#[path]` names the file of the module under it.
#[test]
fn a_platform_attribute_in_a_suites_list_sits_on_the_mod_line_it_gates() -> Outcome {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut wrong = Vec::new();
    let mut gated = 0;
    for file in ["rig.rs", "regressions.rs"] {
        let text = fs::read_to_string(tests.join(file))?;
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        for (at, line) in lines.iter().enumerate() {
            let next = lines.get(at + 1).copied().unwrap_or("");
            if line.starts_with("#[cfg(") {
                gated += 1;
                if !next.starts_with("mod ") {
                    wrong.push(format!(
                        "{file}:{}: {line} is followed by {next:?}, not the mod it gates",
                        at + 1
                    ));
                }
            }
            if let Some(path) = line
                .strip_prefix("#[path = \"")
                .and_then(|rest| rest.strip_suffix("\"]"))
            {
                let stem = Path::new(path)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("");
                let module = lines[at + 1..]
                    .iter()
                    .find(|line| !line.starts_with("#["))
                    .copied()
                    .unwrap_or("");
                if module != format!("mod {stem};") {
                    wrong.push(format!(
                        "{file}:{}: {line} is the path of {module:?}",
                        at + 1
                    ));
                }
            }
        }
    }
    assert!(
        gated > 5,
        "only {gated} attributes were found: the scan is blind"
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    Ok(())
}

/// A file of a suite, in the shape main's folder-trust cases have: a helper that starts a
/// rig and hands it on, a case that takes its rig from it, and the app quit and started
/// again, a helper that takes a rig and gives one back.
const SUITE: &str = r#"
fn asking_with(vars: &[(&str, &str)]) -> cf_e2e::Result<Rig> {
    Rig::start(config())
}

fn quit_and_start_again(mut rig: Rig) -> cf_e2e::Result<Rig> {
    let root = rig.close_keeping_the_root()?;
    Rig::start(config().existing_root(root))
}

#[test]
fn a_case_that_takes_its_rig_from_a_helper() -> Outcome {
    let rig = asking_with(&[("A", "1")])?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_case_that_restarts() -> Outcome {
    let rig = Rig::start(config())?;
    let rig = quit_and_start_again(rig)?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_case_that_expects_the_start_refused() -> Outcome {
    let Err(refused) = Rig::start(config().daemon("liar")) else {
        panic!("started");
    };
    Ok(())
}
"#;

#[test]
fn the_scan_counts_a_rig_a_helper_hands_on_and_none_a_helper_gives_back_or_a_refused_start(
) -> Outcome {
    let found = startings_in(SUITE)?;
    let starts = |name: &str| found.iter().find(|function| function.name == name);

    // All three, closed as they should be: the helper's start counts for the case.
    assert_eq!(
        starts("a_case_that_takes_its_rig_from_a_helper"),
        Some(&Starting {
            name: "a_case_that_takes_its_rig_from_a_helper".to_owned(),
            started: 1,
            closed: 1
        })
    );
    // The rig the case gives the helper is the one it gets back: one start.
    assert_eq!(
        starts("a_case_that_restarts").map(|case| case.started),
        Some(1)
    );
    // A start the case expects to be refused is none to close.
    assert_eq!(starts("a_case_that_expects_the_start_refused"), None);

    // The same case without its close is found: it starts one and closes none.
    let forgotten = SUITE.replacen("    rig.close()?;\n", "", 1);
    let found = startings_in(&forgotten)?;
    let left: Vec<&str> = found
        .iter()
        .filter(|function| function.started > function.closed)
        .map(|function| function.name.as_str())
        .collect();
    assert_eq!(left, ["a_case_that_takes_its_rig_from_a_helper"]);
    Ok(())
}

/// CI's gate job as a list of steps, each one's lines as written, the comments
/// and the list's dash left out.
fn ci_steps() -> Result<Vec<Vec<String>>, Box<dyn std::error::Error>> {
    let workflow = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/ci.yml");
    let mut steps: Vec<Vec<String>> = Vec::new();
    for line in fs::read_to_string(workflow)?.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        if line.starts_with("      - ") {
            steps.push(Vec::new());
        } else if !line.starts_with("        ") {
            continue;
        }
        if let Some(step) = steps.last_mut() {
            step.push(line.trim().trim_start_matches("- ").to_owned());
        }
    }
    Ok(steps)
}

/// Every step of `steps` with a line that holds `needle`.
fn steps_with<'a>(steps: &'a [Vec<String>], needle: &str) -> Vec<&'a [String]> {
    steps
        .iter()
        .filter(|step| step.iter().any(|line| line.contains(needle)))
        .map(Vec::as_slice)
        .collect()
}

/// The step of `steps` with a line that holds `needle`.
fn step_with<'a>(
    steps: &'a [Vec<String>],
    needle: &str,
) -> Result<&'a [String], Box<dyn std::error::Error>> {
    steps
        .iter()
        .find(|step| step.iter().any(|line| line.contains(needle)))
        .map(Vec::as_slice)
        .ok_or_else(|| format!("CI has no step with {needle}").into())
}

/// Whether the upload action's `pattern` finds `path`: as it reads one on Windows,
/// where both slashes part a path and its case does not matter, and `*` is any
/// run of characters of a name.
fn finds(pattern: &str, path: &str) -> Result<bool, regex::Error> {
    let slashes = |text: &str| text.replace('\\', "/");
    let parts: Vec<String> = slashes(pattern).split('*').map(regex::escape).collect();
    let case = if cfg!(windows) { "(?i)" } else { "" };
    Ok(Regex::new(&format!("{case}^{}$", parts.join("[^/]*")))?.is_match(&slashes(path)))
}

/// The path the upload step collects from, `RIG_TEMP` read as the step that names it
/// says it is.
const COLLECTED: &str = "path: ${{ env.RIG_TEMP }}/consensflow-integration-*-left.txt";

/// The path the other upload collects from, relative to the workspace, which the action
/// reads it against: where a rig built for the real harnesses keeps its report
/// (`live::rig`), and where the cases CI runs on such a rig, with the stand-in, keep it.
const COLLECTED_LIVE: &str =
    "path: app/src-tauri/target/live-reports/consensflow-integration-*-left.txt";

#[test]
fn ci_uploads_for_a_failed_job_the_reports_a_failed_case_keeps_and_no_other_file() -> Outcome {
    let steps = ci_steps()?;
    let names = step_with(&steps, "RIG_TEMP=")?;
    let uploads = steps_with(&steps, "uses: actions/upload-artifact@");
    assert_eq!(
        uploads.len(),
        2,
        "one upload for the temp folder's reports and one for the live rigs': {uploads:?}"
    );
    let upload_of = |path: &str| {
        uploads
            .iter()
            .find(|step| step.iter().any(|line| line == path))
            .copied()
            .ok_or_else(|| format!("no upload has `{path}`: {uploads:?}"))
    };
    let (temp_upload, live_upload) = (upload_of(COLLECTED)?, upload_of(COLLECTED_LIVE)?);

    // Only for a failed job, a pinned action, a week, and nothing said where there is nothing.
    for (what, step) in [
        ("the step that names the folder", names),
        ("the upload of the temp folder", temp_upload),
        ("the upload of the live rigs' reports", live_upload),
    ] {
        assert!(
            step.iter().any(|line| line == "if: failure()"),
            "{what} runs for a job that did not fail: {step:?}"
        );
    }
    let pinned = Regex::new(r"^uses: actions/upload-artifact@[0-9a-f]{40} # v\d+\.\d+\.\d+$")?;
    for (upload, name) in [
        (
            temp_upload,
            "name: rig-reports-${{ matrix.os }}-${{ strategy.job-index }}",
        ),
        (
            live_upload,
            "name: live-rig-reports-${{ matrix.os }}-${{ strategy.job-index }}",
        ),
    ] {
        assert!(
            upload.iter().any(|line| pinned.is_match(line)),
            "{upload:?}"
        );
        for line in ["if-no-files-found: ignore", "retention-days: 7", name] {
            assert!(
                upload.iter().any(|have| have == line),
                "no `{line}`: {upload:?}"
            );
        }
    }

    // The folder the step names is the one the rig keeps its reports in: Node's temp
    // folder, as the step asks it, is the Rust's, where there is a Node to ask.
    let node = Regex::new(r#"node -p "([^"]*)""#)?;
    let asked = names
        .iter()
        .find_map(|line| node.captures(line))
        .map(|found| found[1].to_owned())
        .ok_or("the step that names the folder asks no node")?;
    let rust = std::env::temp_dir().to_string_lossy().into_owned();
    let temp = match Run::new(Path::new("node"))
        .args(["-p", &asked])
        .inheriting_env()
        .run()
    {
        Ok(ran) if ran.code == Some(0) => ran.stdout.trim().to_owned(),
        _ => {
            eprintln!("there is no node here: the folder the step names is not asked of it");
            rust.clone()
        }
    };
    let bare = |folder: &str| folder.trim_end_matches(['/', '\\']).to_lowercase();
    assert_eq!(
        bare(&temp),
        bare(&rust),
        "node and the rig's temp folders differ"
    );

    // The report a failed case keeps is found, and neither the home beside it nor
    // another file of the folder.
    let mut home = None;
    assert!(a_case_that_waits_for_what_never_comes(&mut home).is_err());
    let home = home.ok_or("the case never started its rig")?;
    let report = report_beside(&home);
    let collected = COLLECTED
        .trim_start_matches("path: ")
        .replace("${{ env.RIG_TEMP }}", &temp);
    assert!(
        finds(&collected, &report.to_string_lossy())?,
        "`{collected}` does not find {}",
        report.display()
    );
    assert!(!finds(&collected, &home.to_string_lossy())?, "the home");
    let other = format!("{}.old", report.display());
    assert!(!finds(&collected, &other)?, "{other}");
    // The other upload, which the workspace holds, finds none of the temp folder's.
    let live = format!(
        "{}/{}",
        checkout::root().display(),
        COLLECTED_LIVE.trim_start_matches("path: ")
    );
    assert!(!finds(&live, &report.to_string_lossy())?, "{live}");
    fs::remove_file(report)?;

    // A rig built for the real harnesses keeps its report where that upload looks, and
    // the first finds none of it: the ten cases CI runs on such a rig with the stand-in
    // fail into it.
    let mut home = None;
    assert!(a_live_rigs_case_that_waits_for_what_never_comes(&mut home).is_err());
    let home = home.ok_or("the case never started its rig")?;
    let kept =
        checkout::path(LIVE_REPORTS).join(report_beside(&home).file_name().unwrap_or_default());
    let found = finds(&live, &kept.to_string_lossy())?;
    let in_the_temp_pattern = finds(&collected, &kept.to_string_lossy())?;
    let _ = fs::remove_file(&kept);
    assert!(found, "`{live}` does not find {}", kept.display());
    assert!(!in_the_temp_pattern, "{collected}");
    Ok(())
}

#[test]
fn the_reports_are_found_in_the_temp_folders_of_the_runners_of_both_platforms() -> Outcome {
    // The folders CI's run of 2026-10-21 said it kept reports in: the Mac's, as
    // Node says it (TMPDIR without its last slash), and Windows', as TEMP is.
    for (temp, slash) in [
        ("/var/folders/n6/hn37q6pj7sj_9dmqqc2113800000gn/T", '/'),
        (r"C:\Users\RUNNER~1\AppData\Local\Temp", '\\'),
    ] {
        let collected = COLLECTED
            .trim_start_matches("path: ")
            .replace("${{ env.RIG_TEMP }}", temp);
        let report = format!("{temp}{slash}consensflow-integration-AbC123-left.txt");
        assert!(
            finds(&collected, &report)?,
            "`{collected}` does not find {report}"
        );
        let home = format!("{temp}{slash}consensflow-integration-AbC123");
        assert!(!finds(&collected, &home)?, "{home}");
        let deeper = format!("{temp}{slash}other{slash}consensflow-integration-AbC123-left.txt");
        assert!(!finds(&collected, &deeper)?, "{deeper}");
    }
    // And the workspaces the live rigs' reports are kept in, which the other upload reads
    // its path against: the Mac's, and Windows' (another drive than its temp folder's,
    // which is why that is an upload of its own).
    for (workspace, slash) in [
        ("/Users/runner/work/consensflow/consensflow", '/'),
        (r"D:\a\consensflow\consensflow", '\\'),
    ] {
        let relative = COLLECTED_LIVE.trim_start_matches("path: ");
        let collected = format!("{workspace}/{relative}");
        let folder =
            format!("{workspace}{slash}app{slash}src-tauri{slash}target{slash}live-reports");
        let report = format!("{folder}{slash}consensflow-integration-AbC123-left.txt");
        assert!(
            finds(&collected, &report)?,
            "`{collected}` does not find {report}"
        );
        let deeper = format!("{folder}{slash}other{slash}consensflow-integration-AbC123-left.txt");
        assert!(!finds(&collected, &deeper)?, "{deeper}");
    }
    Ok(())
}
