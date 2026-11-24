//! A call into the system that can wait (a macOS permission query, a read of Claude Code's
//! configuration) never holds the daemon's one thread (T-2203: the owner's board stood
//! still for 14 minutes while a launch waited). The runtime here is the daemon's: one
//! thread, a real clock, and a ticker that says whether it went on.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use cf_base::env::Env;
use cf_engine::seams::ComputerUse;
use tokio::task::LocalSet;

use super::MachineComputerUse;

/// How long the ticker may go without running while the call waits.
const MOST_STILL: Duration = Duration::from_millis(300);

/// Runs `scene` on a one-thread runtime beside a ticker, and says the longest the ticker
/// went without running.
fn beside_a_ticker<T>(scene: impl std::future::Future<Output = T>) -> (T, Duration) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let still = Rc::new(RefCell::new(Duration::ZERO));
    let seen = Rc::clone(&still);
    let out = LocalSet::new().block_on(&runtime, async move {
        let ticking = tokio::task::spawn_local(async move {
            let mut last = Instant::now();
            loop {
                tokio::time::sleep(Duration::from_millis(10)).await;
                let now = Instant::now();
                let gap = now - last;
                if gap > *seen.borrow() {
                    *seen.borrow_mut() = gap;
                }
                last = now;
            }
        });
        // The ticker is running before the scene begins.
        tokio::time::sleep(Duration::from_millis(40)).await;
        let out = scene.await;
        // The ticker says how long it stood still when it next runs.
        tokio::time::sleep(Duration::from_millis(40)).await;
        ticking.abort();
        out
    });
    let longest = *still.borrow();
    (out, longest)
}

#[test]
fn a_permission_query_that_waits_leaves_the_thread_going_and_is_answered_when_it_ends() {
    let machine = MachineComputerUse::granting(
        Env::from_vars([("HOME", "/nonexistent")]),
        Rc::new(|| {
            std::thread::sleep(Duration::from_millis(1200));
            None
        }),
    );
    let (asked, still) = beside_a_ticker(async move { call::permissions(&machine).await });
    assert_eq!(asked, None);
    assert!(
        still < MOST_STILL,
        "the thread stood still for {still:?} while the system was asked"
    );
}

/// Claude Code's configuration is a file that can wait to be read: here a named pipe
/// whose writer comes late, as a file on a slow or unmounted disk answers late.
#[cfg(unix)]
#[test]
fn a_read_of_claude_codes_configuration_that_waits_leaves_the_thread_going() {
    let home = tempfile::tempdir().unwrap();
    let pipe = home.path().join(".claude.json");
    let made = std::process::Command::new("/usr/bin/mkfifo")
        .arg(&pipe)
        .status()
        .unwrap();
    assert!(made.success());
    // The size and the time the file is first stamped with are read before its text.
    let writer = {
        let pipe = pipe.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1200));
            std::fs::write(pipe, "{}").unwrap();
        })
    };
    let machine = MachineComputerUse::granting(
        Env::from_vars([("CLAUDE_CONFIG_DIR", home.path().to_str().unwrap())]),
        Rc::new(|| None),
    );
    let (asked, still) = beside_a_ticker(async move { call::claude(&machine, "/work/app").await });
    writer.join().unwrap();
    assert!(!asked.on);
    assert!(
        still < MOST_STILL,
        "the thread stood still for {still:?} while the file was read"
    );
}

/// The calls as the engine makes them, in one place for the test to name.
mod call {
    use super::*;
    use cf_engine::seams::ScreenPermissions;

    pub(super) async fn permissions(machine: &MachineComputerUse) -> Option<ScreenPermissions> {
        machine.permissions()
    }

    pub(super) async fn claude(
        machine: &MachineComputerUse,
        directory: &str,
    ) -> cf_harness::claude::computer_use::ComputerUse {
        machine.claude(directory)
    }
}
