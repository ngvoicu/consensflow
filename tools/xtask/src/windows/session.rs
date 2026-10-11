//! One run of ssh, as it goes: what it writes is written on as it comes, to the
//! stream it came on; what it exits with is the run's own status; and when this
//! program is stopped, or what the run says can no longer be written, it is ended
//! whole before anything else is done.
//!
//! ssh's input is closed (the run has none to give it), and its two streams are
//! pipes, each read on a thread of its own that hands what it reads on, whole, to
//! the one that waits: the writes to the two streams (`Console`) are made here
//! alone, in the order they arrived. That one also asks the [`Interrupt`] as it
//! waits, which no blocked read could.

use std::io::{self, Read, Write};
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cf_base::env::Env;

use super::interrupt::Interrupt;
use super::Error;
use crate::dispatch::Console;
use crate::process::{self, Invocation};

/// How often the wait looks at the stop again.
const LOOK: Duration = Duration::from_millis(50);

/// How much of a stream is read at once.
const CHUNK: usize = 16 * 1024;

/// How a run of ssh ended.
#[derive(Debug)]
pub enum Ended {
    /// With this status, the remote command's own.
    Exited(i32),
    /// Because this program was stopped: ssh is gone, the run may not be.
    Stopped,
    /// Because what it wrote could not be written: ssh is gone, the run may not be.
    Unwritten(io::Error),
}

/// The stream a chunk was written to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stream {
    Out,
    Err,
}

/// What a pipe's reader hands on.
type Chunk = (Stream, Vec<u8>);

/// ssh as it runs: ended if the run is left in any way before it has been waited for.
struct Running(Child);

impl Running {
    /// Ends it, and waits for it to be gone.
    fn end(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

/// Runs `invocation` (ssh) to its end, or to a stop: its output is written to
/// `console` as it comes.
pub fn stream(
    invocation: &Invocation,
    env: &Env,
    console: &mut Console,
    interrupt: &Interrupt,
) -> Result<Ended, Error> {
    let mut child = process::spawn(invocation, env, Stdio::null(), false)?;
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        return Err(process::Failure::Refused {
            program: invocation.program.to_string_lossy().into_owned(),
            cause: io::Error::other("its output could not be read"),
        }
        .into());
    };
    let mut running = Running(child);
    let (to, arriving) = mpsc::channel();
    let pumps = [
        pump(stdout, Stream::Out, to.clone()),
        pump(stderr, Stream::Err, to),
    ];
    match watch(&arriving, console, interrupt) {
        Watched::Stop => {
            running.end();
            Ok(Ended::Stopped)
        }
        Watched::Unwritten(cause) => {
            running.end();
            Ok(Ended::Unwritten(cause))
        }
        // Both streams are closed: ssh has gone, or is going.
        Watched::Closed => {
            for pump in pumps {
                let _ = pump.join();
            }
            let Some(status) = wait(&mut running, interrupt)? else {
                return Ok(Ended::Stopped);
            };
            Ok(exited(status.code(), interrupt))
        }
    }
}

/// What ssh's ending with `code` (none: a signal ended it, which a program that
/// leaves no code is told as 1) came to. ssh gets the Ctrl-C the terminal gives
/// this program too, and its own 255, and a signal, are what that leaves: they
/// are waited on for the stop to be heard. Any other status is the remote
/// command's, whatever is heard after it.
fn exited(code: Option<i32>, interrupt: &Interrupt) -> Ended {
    if matches!(code, None | Some(255)) && interrupt.heard_after_failure() {
        Ended::Stopped
    } else {
        Ended::Exited(code.unwrap_or(1))
    }
}

/// What the wait for output came to.
enum Watched {
    /// Both streams have closed, and all they said has been written.
    Closed,
    Stop,
    Unwritten(io::Error),
}

/// Writes what arrives until the streams close, a stop is heard, or a write fails.
fn watch(arriving: &Receiver<Chunk>, console: &mut Console, interrupt: &Interrupt) -> Watched {
    loop {
        if interrupt.heard() {
            return Watched::Stop;
        }
        match arriving.recv_timeout(LOOK) {
            Ok((stream, bytes)) => {
                let to: &mut dyn Write = match stream {
                    Stream::Out => &mut *console.out,
                    Stream::Err => &mut *console.err,
                };
                if let Err(cause) = to.write_all(&bytes).and_then(|()| to.flush()) {
                    return Watched::Unwritten(cause);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Watched::Closed,
        }
    }
}

/// Waits for ssh to be gone: its status, or none if a stop was heard first (and ssh ended).
fn wait(
    running: &mut Running,
    interrupt: &Interrupt,
) -> Result<Option<std::process::ExitStatus>, Error> {
    loop {
        match running.0.try_wait() {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) if interrupt.heard() => {
                running.end();
                return Ok(None);
            }
            Ok(None) => thread::sleep(LOOK),
            Err(cause) => {
                running.end();
                return Err(Error::Wait(cause));
            }
        }
    }
}

/// Reads `source` to its end on a thread of its own, handing on what it reads.
fn pump(
    mut source: impl Read + Send + 'static,
    stream: Stream,
    to: Sender<Chunk>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0_u8; CHUNK];
        loop {
            match source.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if to.send((stream, buffer[..count].to_vec())).is_err() {
                        break;
                    }
                }
                Err(cause) if cause.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    })
}

#[cfg(test)]
mod tests;
