//! The daemon answers `ping` while a Claude Code status scan is held (P22-1, T-2029): on
//! Windows the daemon's own thread read every status file Claude Code had kept, and a ping
//! waited behind it (its p95 was 173 ms where 30 ms is the same machine with the scan on a
//! worker thread).
//!
//! The scan is the one the engine's windows ask of the records worker (`Records::claude_statuses`,
//! the first thing a look at a Claude window does). A status file that is a named pipe stands for
//! it held at a gate: opening it waits until the test opens its other end.

use std::os::unix::fs::OpenOptionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use cf_harness::contract::Records;
use cf_harness::records::Thread;
use cf_harness::testing::ManualTime;

use super::*;

/// How long the scan is held.
const HELD: Duration = Duration::from_millis(400);

/// How long a `ping` may take while it is: the daemon's own budget, a scan on its thread
/// holds it for the whole of `HELD`.
const BUDGET: Duration = Duration::from_millis(100);

#[tokio::test]
async fn a_claude_status_scan_held_at_a_gate_holds_up_no_ping_of_the_daemon() {
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let folder = rig.home.path().join("sessions");
            std::fs::create_dir_all(&folder).unwrap();
            // The file of a live number, as Claude names it: a scan opens no other.
            let me = std::process::id();
            let gate = folder.join(format!("{me}.json"));
            nix::unistd::mkfifo(&gate, nix::sys::stat::Mode::from_bits_truncate(0o600)).unwrap();
            let row = json!({ "pid": me, "sessionId": "S", "status": "idle" }).to_string();
            let done = Arc::new(AtomicBool::new(false));
            let opener = {
                let done = Arc::clone(&done);
                std::thread::spawn(move || {
                    std::thread::sleep(HELD);
                    while !done.load(Ordering::SeqCst) {
                        // No reader yet: the system says so at once instead of waiting.
                        let open = std::fs::OpenOptions::new()
                            .write(true)
                            .custom_flags(nix::libc::O_NONBLOCK)
                            .open(&gate);
                        match open {
                            Ok(mut other) => {
                                let _ = std::io::Write::write_all(&mut other, row.as_bytes());
                                // Held open until the reader has the bytes, and not opened
                                // again before it is done with them.
                                std::thread::sleep(Duration::from_millis(50));
                                drop(other);
                                std::thread::sleep(Duration::from_millis(50));
                            }
                            Err(_) => std::thread::sleep(Duration::from_millis(5)),
                        }
                    }
                })
            };
            let thread = Thread::new(
                Env::default(),
                cf_harness::records::machine_zone(),
                Rc::new(ManualTime::new(0)),
                Box::new(|_| {}),
            )
            .unwrap();
            let folder = folder.to_string_lossy().into_owned();
            let started = Instant::now();
            let scan =
                tokio::task::spawn_local(async move { thread.claude_statuses(&folder).await });
            // The daemon answers while the scan waits at the gate, as often as it is asked.
            let (mut pings, mut slowest) = (0, Duration::ZERO);
            while !scan.is_finished() {
                let asked = Instant::now();
                assert_eq!(ask(&app, "ping", json!({})).await, json!({ "ok": true }));
                slowest = slowest.max(asked.elapsed());
                pings += 1;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let answer = scan.await.unwrap();
            done.store(true, Ordering::SeqCst);
            opener.join().unwrap();
            assert!(started.elapsed() >= HELD, "the scan waited for the gate");
            assert!(
                pings >= 10,
                "{pings} pings answered while the scan was held"
            );
            assert!(
                slowest < BUDGET,
                "a ping took {slowest:?} while a status scan was held"
            );
            assert_eq!(answer.len(), 1, "the scan was answered: {answer:?}");
            assert_eq!(answer[0].0, me);
        })
        .await;
}
