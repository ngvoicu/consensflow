//! A look at a Claude window leaves the engine's thread (P22-1, T-2029): on Windows, a look read
//! every status file Claude Code had kept and opened each process it named, on the one thread
//! that also answers `ping` and runs every timer (a 10 ms timer ran up to 820 ms late, 1,140 of
//! 1,239 slow pings waited behind such a scan).
//!
//! A status file that is a named pipe stands for a scan that is held at a gate: opening it
//! waits until the test opens its other end. A thread that scans it cannot run another
//! task's timer late; the engine's thread, scanning it, can.

use std::os::unix::fs::OpenOptionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cf_harness::records::Thread;
use cf_harness::seams::Time;
use cf_harness::testing::ManualTime;
use jiff::tz::TimeZone;
use tokio::task::LocalSet;

use super::*;

/// The conversation the window shows.
const SESSION: &str = "4b6e2a91-7c3d-4f5e-9a8b-1c2d3e4f5a6b";

/// How long the scan is held.
const HELD: Duration = Duration::from_millis(400);

/// How late a 10 ms timer may be while a scan is held: the engine's own budget of the probe
/// was a worst of 250 ms, a scan on another thread costs it none.
const BUDGET: Duration = Duration::from_millis(100);

#[test]
fn a_status_scan_held_at_a_gate_stops_no_timer_of_the_engine_and_its_look_is_answered() {
    let home = Home::new();
    let sessions = path::join(&[home.var("CLAUDE_CONFIG_DIR"), "sessions"]);
    fs::create_dir_all(&sessions).unwrap();
    // The file of a live number, as Claude names it: a scan opens no other. The gate is its
    // opening: it waits until the test opens the other end.
    let gate = Path::new(&sessions).join(format!("{}.json", me()));
    nix::unistd::mkfifo(&gate, nix::sys::stat::Mode::from_bits_truncate(0o600)).unwrap();
    let env = home.env();
    let time: Rc<dyn Time> = Rc::new(ManualTime::new(0));
    let thread = Thread::new(
        env.clone(),
        TimeZone::UTC,
        Rc::clone(&time),
        Box::new(|_| {}),
    )
    .unwrap();
    let fakes = Fakes::new(&env);
    let services = cf_harness::seams::Services {
        records: Rc::new(thread),
        ..fakes.services(&env, Path::new(&home.root))
    };
    let adapter = ClaudeAdapter::new(&services);
    let window = window_of(&adapter);
    window.opened(Some(me()));
    // The other end, opened late and as often as the look asks: each scan of the folder
    // opens the file once, which waits until someone has it open to write.
    let row = json!({ "pid": me(), "sessionId": SESSION, "status": "idle" }).to_string();
    let done = Arc::new(AtomicBool::new(false));
    let opener = {
        let done = Arc::clone(&done);
        std::thread::spawn(move || {
            std::thread::sleep(HELD);
            while !done.load(Ordering::SeqCst) {
                // No reader yet: the system says so at once instead of waiting.
                let open = fs::OpenOptions::new()
                    .write(true)
                    .custom_flags(nix::libc::O_NONBLOCK)
                    .open(&gate);
                match open {
                    Ok(mut other) => {
                        let _ = std::io::Write::write_all(&mut other, row.as_bytes());
                        // Held open until the reader has the bytes: a pipe closed before
                        // its reader's open has returned gives it none.
                        std::thread::sleep(Duration::from_millis(50));
                        drop(other);
                        // And not opened again before that reader is done: its next read
                        // would be two rows.
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        })
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let (late, answered_after, answer) = LocalSet::new().block_on(&runtime, async {
        let started = Instant::now();
        let looking = tokio::task::spawn_local(async move { window.observe().await });
        // The engine's thread keeps its timers while the look waits at the gate: the time it
        // is late is measured for as long as the look is not answered.
        let mut late = Duration::ZERO;
        while !looking.is_finished() {
            let asked = Instant::now();
            tokio::time::sleep(Duration::from_millis(10)).await;
            late = late.max(asked.elapsed().saturating_sub(Duration::from_millis(10)));
        }
        let answer = looking.await.unwrap();
        (late, started.elapsed(), answer)
    });
    done.store(true, Ordering::SeqCst);
    opener.join().unwrap();
    assert!(
        answered_after >= HELD,
        "the look waited for the gate ({answered_after:?})"
    );
    assert!(
        late < BUDGET,
        "a 10 ms timer ran {late:?} late while a status scan was held"
    );
    // The look was answered with what the held file said, not with nothing.
    let observed = answer.expect("the look is answered");
    assert!(
        observed.at_prompt,
        "the status the gate let through says idle: {:?} {:?}",
        observed.settled, observed.switched
    );
}

/// The window of a new launch.
fn window_of(adapter: &ClaudeAdapter) -> Rc<dyn Window> {
    // A launch asks the records nothing; the window then follows the conversation.
    let window = window(adapter, &Request::default());
    window.follow(SESSION);
    window
}
