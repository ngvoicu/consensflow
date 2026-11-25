//! A call into the system that can wait (a macOS permission query, a read of Claude Code's
//! configuration) never holds the daemon's one thread (T-2203: the owner's board stood
//! still for 14 minutes while a launch waited). The runtime here is the daemon's: one
//! thread, a real clock, and a ticker that says whether it went on.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use cf_base::env::Env;
use cf_engine::seams::{ComputerUse, Waited};
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
        Arc::new(|| {
            std::thread::sleep(Duration::from_millis(1200));
            None
        }),
    );
    let (asked, still) = beside_a_ticker(async move { call::permissions(&machine).await });
    assert_eq!(asked, Ok(None));
    assert!(
        still < MOST_STILL,
        "the thread stood still for {still:?} while the system was asked"
    );
}

#[test]
fn a_permission_query_that_never_ends_is_given_up_and_the_thread_goes_on() {
    let (release, held) = mpsc::channel::<()>();
    let held = std::sync::Mutex::new(held);
    let machine = MachineComputerUse::granting(
        Env::from_vars([("HOME", "/nonexistent")]),
        Arc::new(move || {
            let _ = held.lock().unwrap().recv();
            None
        }),
    )
    .waiting(Duration::from_millis(400));
    let ((asked, took), still) = beside_a_ticker(async move {
        let began = Instant::now();
        // A wait with no limit of the machine's own fails here and does not hang the test.
        let asked = tokio::time::timeout(Duration::from_secs(10), call::permissions(&machine))
            .await
            .expect("the system was waited for with no limit");
        (asked, began.elapsed())
    });
    drop(release);
    assert_eq!(asked, Err(Waited(Duration::from_millis(400))));
    assert!(took < Duration::from_secs(3), "given up after {took:?}");
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
    nix::unistd::mkfifo(&pipe, nix::sys::stat::Mode::from_bits_truncate(0o600)).unwrap();
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
        Arc::new(|| None),
    );
    let (asked, still) = beside_a_ticker(async move { call::claude(&machine, "/work/app").await });
    writer.join().unwrap();
    assert!(!asked.unwrap().on);
    assert!(
        still < MOST_STILL,
        "the thread stood still for {still:?} while the file was read"
    );
}

/// The calls as the engine makes them, in one place for the test to name.
mod call {
    use super::*;
    use cf_engine::seams::{ScreenPermissions, Waited};

    pub(super) async fn permissions(
        machine: &MachineComputerUse,
    ) -> Result<Option<ScreenPermissions>, Waited> {
        machine.permissions().await
    }

    #[cfg(unix)]
    pub(super) async fn claude(
        machine: &MachineComputerUse,
        directory: &str,
    ) -> Result<cf_harness::claude::computer_use::ComputerUse, Waited> {
        machine.claude(directory).await
    }
}
