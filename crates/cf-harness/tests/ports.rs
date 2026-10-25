//! `seams::tests::a_free_port_on_loopback_is_one_a_listener_may_take` failed once
//! in a run of the crate's tests on a busy Mac (T-349): "Address already in use"
//! binding the port the seam had just named. It is that case, run as the
//! crate's tests run side by side: one thread names ports and binds them while
//! others start programs. Kept to be shown by hand:
//!
//! `cargo test -p cf-harness --features test-support --test ports -- --ignored
//! --nocapture`
//!
//! macOS cannot open a socket close-on-exec: std opens it and makes it so in a
//! second call. A program another thread starts between the two is left the
//! listener the seam opened to learn the port. The seam lets its own go, and the
//! port stays listened on by that program: for as long as it runs, or, where a
//! program is started holding only its streams, for the milliseconds it takes to
//! let go. Before the seam looked at the port it named (a connection to it, once
//! its own listener is gone) 43 to 64 of some 45,000 ports named beside eight
//! threads starting programs were refused the bind; with the look, none of
//! 134,000.
//! `CF_LOAD_SPAWN=product` starts the programs as the engine does
//! (`SystemProcesses`), `bare` as a test launcher does that holds more than its
//! streams.
//!
//! What it is told by the environment (all optional):
//! - `CF_LOAD_SECS`: how long ports are named and bound (10);
//! - `CF_LOAD_THREADS`: how many threads start programs meanwhile (8);
//! - `CF_LOAD_SPAWN`: `bare` or `product` (`bare`).

#![cfg(unix)]
// A load test shows what it found, by hand.
#![allow(clippy::print_stdout)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::ErrorKind;
use std::net::TcpListener;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cf_base::env::Env;
use cf_harness::seams::processes::{Ending, Program, Streams};
use cf_harness::seams::{LoopbackPorts, Ports, Processes, SystemProcesses};

/// What the environment says of `name`, or `default`.
fn knob(name: &str, default: usize) -> usize {
    Env::from_process()
        .text(name)
        .and_then(|given| given.parse().ok())
        .unwrap_or(default)
}

/// Starts a program and ends it a moment after, over and over until `stop`:
/// what a test that runs one does, beside the others.
fn start_programs(product: bool, stop: &AtomicBool) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    // Starting a program needs the runtime its waits are made on.
    let _inside = runtime.enter();
    let processes = SystemProcesses::new(Env::from_process());
    while !stop.load(Ordering::Relaxed) {
        if product {
            let program = Program {
                executable: "/bin/sleep".into(),
                args: vec!["5".to_owned()],
                cwd: None,
                env: Env::default(),
            };
            let child = processes.spawn(program, Streams::Quiet).unwrap();
            thread::sleep(Duration::from_millis(30));
            child.terminate(Ending::Forced);
            runtime.block_on(child.closed());
        } else {
            #[allow(clippy::disallowed_methods)] // A launcher that holds more than its streams.
            let mut child = Command::new("/bin/sleep").arg("5").spawn().unwrap();
            thread::sleep(Duration::from_millis(30));
            child.kill().unwrap();
            child.wait().unwrap();
        }
    }
}

#[test]
#[ignore = "loads the machine for seconds: run by hand, with --nocapture"]
fn under_load_a_port_the_seam_names_is_one_a_listener_may_take() {
    let product = Env::from_process().text("CF_LOAD_SPAWN") == Some("product");
    let stop = Arc::new(AtomicBool::new(false));
    let starters: Vec<_> = (0..knob("CF_LOAD_THREADS", 8))
        .map(|_| {
            let stop = Arc::clone(&stop);
            thread::spawn(move || start_programs(product, &stop))
        })
        .collect();
    let until = Instant::now() + Duration::from_secs(knob("CF_LOAD_SECS", 10) as u64);
    let (mut named, mut failed) = (0_u64, Vec::new());
    while Instant::now() < until {
        // The system runs out of buffers for sockets made this often; the
        // seam's naming is what is tried, so it is paced.
        thread::sleep(Duration::from_micros(200));
        let Ok(port) = LoopbackPorts.free_loopback() else {
            continue;
        };
        named += 1;
        if let Err(refused) = TcpListener::bind(("127.0.0.1", port)) {
            if refused.kind() != ErrorKind::AddrInUse {
                continue;
            }
            // How long the port stays taken: for the moment a program being
            // started holds the listener, or for as long as it runs.
            let began = Instant::now();
            while TcpListener::bind(("127.0.0.1", port)).is_err() && began.elapsed().as_secs() < 5 {
                thread::sleep(Duration::from_millis(1));
            }
            println!(
                "port {port} (the {named}th named): {refused}; taken for {:?} more",
                began.elapsed()
            );
            failed.push(port);
        }
    }
    stop.store(true, Ordering::Relaxed);
    for starter in starters {
        starter.join().unwrap();
    }
    println!(
        "{} of {named} ports named were taken ({} programs, {})",
        failed.len(),
        knob("CF_LOAD_THREADS", 8),
        if product { "product" } else { "bare" }
    );
    assert!(failed.is_empty(), "{failed:?}");
}
