//! What ending a capture ends with it when its program is still starting programs: all of
//! them, and what it starts as it is ended. `taskkill /T` reads the tree and then ends what
//! it read, so a program that starts one every few milliseconds leaves the ones started
//! after the read, each with a dead parent (zeewin, T-322: 21 to 50 of a loop's pings
//! survived every one of 26 stops).
//!
//! Each test runs `keeps_starting_programs`, waits until it has started some, ends the
//! capture one of the ways it can end, and asks the system, by pid, what is still running
//! of all that program wrote down.

use std::pin::pin;

use super::tree::tree_env;
use super::*;
use crate::testing::{
    keeps_starting_programs, keeps_starting_programs_for_long, left_running, pids_so_far, Recorded,
    KEEPS_STARTING, STARTS_WITHIN,
};

/// How many pids the program has written (its own, and the programs it started) when the
/// end begins: it has far more to start.
const STARTED_BEFORE: usize = 4;

/// How long what a capture's end left is given to go: a few seconds, where the programs run
/// for twenty.
const ENDED_WITHIN: Duration = Duration::from_secs(5);

/// How long the runtime that ran a capture is given to shut down once the capture has
/// ended: it waits for every read of the program's streams, and a program that is left
/// running holds them for as long as it lives.
const SHUTS_DOWN_WITHIN: Duration = Duration::from_secs(5);

/// Waits until the program has started [`STARTED_BEFORE`] pids.
async fn started(file: &Path) {
    let until = Instant::now() + STARTS_WITHIN;
    while pids_so_far(file).len() < STARTED_BEFORE {
        assert!(Instant::now() < until, "the program never started any");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// How long the runtime took to shut down: it waits for every read of the program's streams.
fn shut_down(runtime: tokio::runtime::Runtime) -> Duration {
    let began = Instant::now();
    runtime.shutdown_timeout(SHUTS_DOWN_WITHIN * 2);
    began.elapsed()
}

/// What is said of the end: no process the program started is left running (`left`, asked
/// for once the capture was ended), and nothing it started held the runtime back.
fn assert_ended(file: &Path, left: &[u32], shut_down: Duration) {
    let all = pids_so_far(file);
    assert!(all.len() >= STARTED_BEFORE, "it started programs");
    assert!(
        all.len() <= KEEPS_STARTING + 1,
        "it started no more than it is bounded to"
    );
    assert!(
        left.is_empty(),
        "{} of the {} processes it started are still running {ENDED_WITHIN:?} after its end: {left:?}",
        left.len(),
        all.len()
    );
    assert!(
        shut_down < SHUTS_DOWN_WITHIN,
        "the runtime took {shut_down:?} to shut down: a program it started held its streams"
    );
}

#[test]
fn an_ender_ends_a_program_that_is_starting_programs_and_everything_it_started() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let (run, env) = (keeps_starting_programs(dir.path()), tree_env());
    let runtime = runtime();
    let _left = Recorded(&file);
    let kept: Rc<RefCell<Option<Ender>>> = Rc::default();
    let left = runtime.block_on(async {
        let handed = Rc::clone(&kept);
        let capturing = capture(&run, None, &env, ROOMY, move |ender| {
            *handed.borrow_mut() = Some(ender);
        });
        let mut capturing = pin!(capturing);
        tokio::select! {
            () = started(&file) => {}
            captured = &mut capturing => panic!("it ended before it started any: {captured:?}"),
        }
        // As a process on its way out ends its children.
        kept.borrow().as_ref().expect("handed over").force();
        // The capture is still there, and answers once its program has ended (a program that
        // is not reaped is a zombie, and reads as running); what the end left is what is asked.
        let _ = tokio::time::timeout(ENDED_WITHIN, &mut capturing).await;
        left_running(&file, Duration::from_secs(1)).await
    });
    assert_ended(&file, &left, shut_down(runtime));
}

#[test]
fn a_capture_dropped_ends_a_program_that_is_starting_programs_and_everything_it_started() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let (run, env) = (keeps_starting_programs(dir.path()), tree_env());
    let runtime = runtime();
    let _left = Recorded(&file);
    let left = runtime.block_on(async {
        let mut capturing = Box::pin(capture(&run, None, &env, ROOMY, |_| {}));
        tokio::select! {
            () = started(&file) => {}
            captured = &mut capturing => panic!("it ended before it started any: {captured:?}"),
        }
        drop(capturing);
        left_running(&file, ENDED_WITHIN).await
    });
    assert_ended(&file, &left, shut_down(runtime));
}

#[test]
fn a_capture_out_of_time_ends_a_program_that_is_starting_programs_and_everything_it_started() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let (run, env) = (keeps_starting_programs_for_long(dir.path()), tree_env());
    let runtime = runtime();
    let _left = Recorded(&file);
    // Out of time well after the program has begun (five seconds: far more than it needs on a
    // slow runner to have started some), while it is still starting programs (it goes on for
    // nine); what they print is read, and is no more than the limit.
    let limits = Limits {
        timeout: Duration::from_secs(5),
        max_buffer: 1024 * 1024,
    };
    let failed = runtime
        .block_on(capture(&run, None, &env, limits, |_| {}))
        .unwrap_err();
    assert!(failed.killed, "{failed:?}");
    let left = runtime.block_on(left_running(&file, ENDED_WITHIN));
    assert_ended(&file, &left, shut_down(runtime));
}
