//! What a capture held whole has gone when it returns, Windows: a program that is out of its
//! time or says more than it may is ended with every process of its job, and the capture
//! returns once each has ended, not once they were told to: the plants' runner puts a run's
//! sources back when the capture returns, and a process of it that still holds a file of
//! them (it is ending, with a handle of its own) would be putting back over it.
//!
//! The program starts [`HOLDERS`] programs that each hold a file open shared with no one, and
//! a thousand threads (a process with many takes a moment to end), and says too much. The test
//! holds each of them by a handle of its own, opened before the end, which says when the process
//! has ended (an exit code is set as the end begins, and the job lists none of those that are
//! still ending). When the capture has returned, each is to have ended, and each file is removed
//! at once, with no second try.

use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{
    OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
};

use super::tree::tree_env;
use super::*;
use crate::testing::{
    pids_so_far, starts_holders_and_says_too_much, Recorded, HOLDERS, STARTS_WITHIN,
};

/// A handle of each holder, once the program has written their pids: the leader's is the
/// first pid, and the holders' the rest.
async fn holders_held(file: &Path) -> Vec<usize> {
    let until = Instant::now() + STARTS_WITHIN;
    loop {
        let pids = pids_so_far(file);
        if pids.len() > HOLDERS {
            return pids[1..=HOLDERS]
                .iter()
                .map(|&pid| {
                    #[allow(unsafe_code)]
                    // SAFETY: OpenProcess takes plain values and returns a handle or null.
                    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
                    assert!(!handle.is_null(), "{pid} is there");
                    handle as usize
                })
                .collect();
        }
        assert!(Instant::now() < until, "the program wrote {pids:?}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[test]
fn a_program_held_whole_that_says_too_much_has_every_process_of_its_job_ended_when_the_capture_returns(
) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let run = starts_holders_and_says_too_much(dir.path());
    let (runtime, env) = (runtime(), tree_env());
    let _left = Recorded(&file);
    let limits = Limits {
        timeout: Duration::from_secs(120),
        max_buffer: 1024,
    };
    let (failed, handles) = runtime.block_on(async {
        let mut capturing = Box::pin(capture_whole(&run, None, &env, limits, |_| {}));
        let handles = tokio::select! {
            handles = holders_held(&file) => handles,
            captured = &mut capturing => panic!("it ended before it started them: {captured:?}"),
        };
        (capturing.await, handles)
    });
    runtime.shutdown_timeout(Duration::from_secs(10));
    let failed = failed.unwrap_err();
    assert_eq!(
        failed.message, "stdout maxBuffer length exceeded",
        "{failed:?}"
    );
    let still_ending = handles
        .iter()
        .filter(|&&handle| {
            #[allow(unsafe_code)]
            // SAFETY: the handle is open, with SYNCHRONIZE, until it is closed below.
            let waited = unsafe { WaitForSingleObject(handle as _, 0) };
            waited != WAIT_OBJECT_0
        })
        .count();
    // At once: a file a process of the job still holds open is a process still ending.
    let held: Vec<String> = (0..HOLDERS)
        .filter_map(|index| {
            std::fs::remove_file(dir.path().join(format!("held-{index}")))
                .err()
                .map(|failed| format!("held-{index}: {failed}"))
        })
        .collect();
    for handle in handles {
        #[allow(unsafe_code)]
        // SAFETY: the handle is open, and closed once, here.
        unsafe {
            CloseHandle(handle as _);
        }
    }
    assert_eq!(
        still_ending, 0,
        "{still_ending} of {HOLDERS} processes of the job were still ending when the capture returned"
    );
    assert!(held.is_empty(), "files still held: {held:?}");
}
