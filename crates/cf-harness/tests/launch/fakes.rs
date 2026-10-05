//! What the launch's tests stand the engine's side and the machine in with:
//! a step's work done at once, the records served on the test's own thread,
//! a pane host that answers as a test says, and a stand-in CLI.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use cf_base::env::Env;
use cf_base::time::{Clock, SystemClock};
use cf_harness::contract::{HostError, PaneHost, Records, Work};
use cf_harness::records::{self, Cache, Options, Reading, IDLE_MS};
use cf_proto::agents::Harness;
use jiff::tz::TimeZone;
use serde_json::Value;

/// The work a step does, done: nothing it is given ever waits.
pub fn done<T>(mut work: Work<'_, T>) -> T {
    match work.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("a step waited on nothing it was given"),
    }
}

/// The records as the engine serves them, read here on the test's thread
/// with a cache of readers, as the engine's worker keeps one.
pub struct Local {
    env: Env,
    cache: RefCell<Cache>,
}

impl Local {
    /// No case names a reset by a time of day alone, which a zone reads.
    pub fn new(env: Env) -> Self {
        let cache = Cache::new(records::open(TimeZone::UTC), IDLE_MS, SystemClock.now_ms());
        Self {
            env,
            cache: RefCell::new(cache),
        }
    }
}

impl Records for Local {
    fn look<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
        options: &'a Options,
    ) -> Work<'a, Arc<Reading>> {
        Box::pin(async move {
            let now = SystemClock.now_ms();
            self.cache
                .borrow_mut()
                .look(harness, session, &self.env, options, now)
        })
    }

    fn has_transcript<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
    ) -> Work<'a, Result<bool, String>> {
        Box::pin(async move { records::has_transcript(harness, session, &self.env) })
    }
}

/// A pane host that answers each request as `answer` says, and keeps what
/// it was asked.
pub struct Answering<F> {
    answer: F,
    pub asked: RefCell<Vec<(String, Value)>>,
}

impl<F: Fn(&str) -> Result<Value, HostError>> Answering<F> {
    pub fn new(answer: F) -> Self {
        Self {
            answer,
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl<F: Fn(&str) -> Result<Value, HostError>> PaneHost for Answering<F> {
    fn request<'a>(&'a self, op: &'a str, body: Value) -> Work<'a, Result<Value, HostError>> {
        Box::pin(async move {
            self.asked.borrow_mut().push((op.to_owned(), body));
            (self.answer)(op)
        })
    }
}

/// A stand-in CLI at `file` (`fakeExecutable`, tests/helpers.mjs): a shell
/// script on POSIX, a `.cmd` on Windows. The path it is found at.
pub fn fake_executable(file: &Path) -> PathBuf {
    if cfg!(windows) {
        let mut shim = file.as_os_str().to_owned();
        shim.push(".cmd");
        fs::write(&shim, "@echo off\r\nexit /b 0\r\n").unwrap();
        return PathBuf::from(shim);
    }
    fs::write(file, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    fs::set_permissions(file, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    file.to_path_buf()
}
