//! What a release asks of the machine it runs on, behind one seam so that its
//! tests give it a GitHub of their own: the programs it runs (`git`, `gh`,
//! `curl`, `cargo`), each to its end with what it wrote kept, and the waits
//! between its looks at GitHub. [`Real`] is the machine; a test's is a script.

use std::time::Duration;

use cf_base::env::Env;

use super::Error;
use crate::process::{self, Captured, Invocation};

/// The machine a release is made from.
pub trait System {
    /// Runs `invocation` to its end and keeps what it wrote.
    fn capture(&mut self, invocation: &Invocation) -> Result<Captured, Error>;

    /// Waits `duration` before the next look.
    fn pause(&mut self, duration: Duration);
}

/// This machine: its programs, found on the environment xtask was started with.
pub struct Real<'a> {
    env: &'a Env,
}

impl<'a> Real<'a> {
    pub fn new(env: &'a Env) -> Self {
        Self { env }
    }
}

impl System for Real<'_> {
    fn capture(&mut self, invocation: &Invocation) -> Result<Captured, Error> {
        Ok(process::capture(invocation, self.env)?)
    }

    fn pause(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}
