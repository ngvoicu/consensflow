//! `create::mints_distinct_ids_on_two_calls` failed twice in gate runs with
//! "opencode serve exited early" (T-302, T-330), under load, and passed alone.
//! This is the case as it was, run again and again by threads beside each
//! other, as the tests of the binary are, and kept to be shown by hand:
//!
//! `cargo test -p cf-harness --features test-support --test channels --
//! --ignored under_load --nocapture`
//!
//! With `CF_LOAD_THREADS=12 CF_LOAD_BURNERS=4` on a Mac, 23 of 46,800 rounds
//! failed. The stand-in said it could not listen on its port and ended with 1,
//! and the port was listened on by a sibling or more: programs this process
//! started on other threads, which were left the listener that learned the port
//! (a listener is made close-on-exec in a second call on macOS), and kept it
//! as long as they ran. One thread alone, beside computing threads or beside
//! threads drawing 200,000 ports a second, never failed: it takes programs
//! started while a port is being learned.
//!
//! The stand-in is run through a note-taker, because `Streams::Quiet` never
//! keeps what a server prints: for each start it leaves what the server said
//! on its error stream and the status it ended with, and, if it ended on its
//! own, who held its port at that moment. A round that fails prints them with
//! the port the round gave the server. macOS only: that is where the port is
//! held, and where `lsof` is at the path the note-taker names.
//!
//! What it is told by the environment (all optional):
//! - `CF_LOAD_ROUNDS`: how many times the case is run on each thread (100);
//! - `CF_LOAD_THREADS`: how many threads run it at once (1);
//! - `CF_LOAD_BURNERS`: threads computing for each core, as a build does (0);
//! - `CF_LOAD_DRAWS`: loopback ports other programs draw and let go each
//!   second (0);
//! - `CF_LOAD_DIRECT`: 1 to start the stand-in itself, with no note-taker
//!   between, so that nothing but the product's own start is in the way (0);
//! - `CF_LOAD_NOTES`: a folder to keep the failures' notes in, which are
//!   never committed.

#![cfg(target_os = "macos")]
// A load test shows what it found, by hand.
#![allow(clippy::print_stdout)]

use std::fs;
use std::hint::black_box;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cf_base::env::Env;
use tempfile::TempDir;

use crate::create::Setup;
use crate::support::run;

/// The stand-in `opencode`.
const FAKE: &str = env!("CARGO_BIN_EXE_fake-opencode");

/// Runs the stand-in beside a note of how it ended, and of who held the port
/// it was given if it ended on its own at once. A server asked to end ends its
/// child, as `opencode` does its own.
const NOTE_TAKER: &str = r#"#!/bin/sh
note="$CF_LOAD_NOTES/start.$$"
port=
for word in "$@"; do
    [ "$last" = "--port" ] && port=$word
    last=$word
done
"$CF_LOAD_REAL" "$@" 2>"$note.said" &
child=$!
trap 'kill -TERM "$child" 2>/dev/null' TERM
wait "$child"
status=$?
if kill -0 "$child" 2>/dev/null; then
    wait "$child"
    status=$?
fi
echo "$status" >"$note.status"
if [ "$status" != 0 ] && [ "$status" -lt 128 ]; then
    /usr/sbin/lsof -nP -iTCP:"$port" >"$note.holders" 2>&1
    for holder in $(/usr/sbin/lsof -nP -t -iTCP:"$port" 2>/dev/null); do
        /bin/ps -o pid=,ppid=,etime=,command= -p "$holder" >>"$note.holders" 2>&1
    done
fi
exit "$status"
"#;

/// What the environment says of `name`, or `default`.
fn knob(name: &str, default: usize) -> usize {
    Env::from_process()
        .text(name)
        .and_then(|given| given.parse().ok())
        .unwrap_or(default)
}

/// Threads that do nothing but compute, and threads that draw a port from the
/// system and let it go, until they are dropped: to the machine, what other
/// builds and tests running beside this one are.
struct Others {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Others {
    fn start(burners_per_core: usize, draws_per_second: usize) -> Self {
        let cores = thread::available_parallelism().map_or(1, usize::from);
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();
        for _ in 0..cores * burners_per_core {
            let stop = Arc::clone(&stop);
            threads.push(thread::spawn(move || {
                let mut at = 1_u64;
                while !stop.load(Ordering::Relaxed) {
                    at = black_box(at.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1));
                }
            }));
        }
        if draws_per_second > 0 {
            let stop = Arc::clone(&stop);
            // Paced by the millisecond: that many draws, then a rest.
            let each = draws_per_second.div_ceil(1000);
            threads.push(thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    for _ in 0..each {
                        drop(black_box(TcpListener::bind(("127.0.0.1", 0))));
                    }
                    thread::sleep(Duration::from_millis(1));
                }
            }));
        }
        Self { stop, threads }
    }
}

impl Drop for Others {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// The note-taker, written into `dir`.
fn note_taker(dir: &Path) -> PathBuf {
    let path = dir.join("opencode");
    fs::write(&path, NOTE_TAKER).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// `setup`, told where the note-taker leaves its notes and what it runs.
fn noted(mut setup: Setup, notes: &Path) -> Setup {
    let notes = notes.join(format!("port-{}", setup.port));
    fs::create_dir_all(&notes).unwrap();
    let vars = setup
        .env
        .iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .chain([
            ("CF_LOAD_NOTES".into(), notes.into_os_string()),
            ("CF_LOAD_REAL".into(), FAKE.into()),
        ]);
    setup.env = Env::from_vars(vars);
    setup
}

/// What the note-taker left for the start on `port`: what the server said and
/// the status it ended with.
fn notes_of(notes: &Path, port: u16) -> String {
    let mut found = Vec::new();
    for entry in fs::read_dir(notes.join(format!("port-{port}")))
        .into_iter()
        .flatten()
    {
        let path = entry.unwrap().path();
        found.push(format!(
            "{}: {}",
            path.file_name().unwrap().to_string_lossy(),
            fs::read_to_string(&path).unwrap_or_default().trim()
        ));
    }
    found.sort();
    found.join(" | ")
}

/// One thread's rounds: the case as `mints_distinct_ids_on_two_calls` runs it,
/// the failures it met.
fn rounds(count: usize, program: &str, notes: &Path) -> Vec<String> {
    let mut failed = Vec::new();
    for round in 1..=count {
        let (a, b) = (
            noted(Setup::new("ok"), notes),
            noted(Setup::new("ok"), notes),
        );
        let first = run(a.create(program, None));
        let second = run(b.create(program, None));
        for (setup, made) in [(&a, first), (&b, second)] {
            if let Err(error) = made {
                println!(
                    "round {round}, port {}: {error}; {}",
                    setup.port,
                    notes_of(notes, setup.port)
                );
                failed.push(error);
            }
        }
    }
    failed
}

#[test]
#[ignore = "loads the machine: run by hand, with --nocapture"]
fn under_load_two_calls_of_a_throwaway_server_are_each_given_a_port_that_stays_free() {
    let count = knob("CF_LOAD_ROUNDS", 100);
    let threads = knob("CF_LOAD_THREADS", 1);
    let here = TempDir::new().unwrap();
    let kept = Env::from_process().text("CF_LOAD_NOTES").map(PathBuf::from);
    let notes = kept.as_deref().unwrap_or(here.path());
    let taker = note_taker(here.path());
    let program = if knob("CF_LOAD_DIRECT", 0) == 1 {
        FAKE
    } else {
        taker.to_str().unwrap()
    };
    let others = Others::start(knob("CF_LOAD_BURNERS", 0), knob("CF_LOAD_DRAWS", 0));
    let failed: Vec<String> = thread::scope(|scope| {
        let running: Vec<_> = (0..threads)
            .map(|_| scope.spawn(|| rounds(count, program, notes)))
            .collect();
        running
            .into_iter()
            .flat_map(|running| running.join().unwrap())
            .collect()
    });
    drop(others);
    println!("{} of {} rounds failed", failed.len(), count * threads);
    assert!(failed.is_empty(), "{failed:?}");
}
