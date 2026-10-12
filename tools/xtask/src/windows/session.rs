//! One run of ssh, as it goes: what it writes is written on as it comes, to the
//! stream it came on; how the command ended is what the machine says of it in the
//! last line of the output (`== exit N`), and ssh's own status only where it says
//! nothing; and when this program is stopped, or what the run says can no longer
//! be written, it is ended whole before anything else is done.
//!
//! ssh's input is closed (the run has none to give it), and its two streams are
//! pipes, each read on a thread of its own that hands what it reads on, whole, to
//! the one that waits: the writes to the two streams (`Console`) are made here
//! alone, in the order they arrived, and the last line of the standard output is
//! kept as it goes by. That one also asks the [`Interrupt`] as it waits, which no
//! blocked read could.

use std::io::{self, Read, Write};
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cf_base::env::Env;

use super::interrupt::Interrupt;
use super::script::{exit_said, EXIT_MARK};
use super::tail::Tail;
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
    /// With this status, the remote command's own, as far as it is known: what the
    /// machine said of it, or else ssh's (`exited` says which, and why).
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
    let mut tail = Tail::default();
    match watch(&arriving, console, interrupt, &mut tail) {
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
            let said = tail.last_line().as_deref().and_then(exit_said);
            let (ended, note) = exited(status.code(), said, interrupt);
            if let Some(note) = note {
                if let Err(cause) = writeln!(console.err, "windows: {note}") {
                    return Ok(Ended::Unwritten(cause));
                }
            }
            Ok(ended)
        }
    }
}

/// What a run that ssh ended by itself came to, and what is to be said of it
/// besides (for a person, on the error stream).
///
/// `said` is the status the machine gave in the last line of the output, if it
/// gave one: it is the command's own, whatever ssh answered, which the machine's
/// shell may turn into another (zeewin's PowerShell makes every failure 1). Where
/// it gave none, nothing is known of how the command ended but what ssh answered,
/// and that is the answer if it is a failure ssh's own (ssh's 255 and a signal
/// first wait for the stop that may have made them: ssh gets the Ctrl-C the
/// terminal gives this program too). Never 0, however: a run that has not said
/// it ended well did not end well.
///
/// The answer is a status a program can end with, 0 to 255: what is outside it
/// (a Windows status such as -1073741819 is) is answered as 1, for a system that
/// keeps a byte of one would make a failure of 256 a success. The line in the
/// output says what it was.
fn exited(code: Option<i32>, said: Option<i32>, interrupt: &Interrupt) -> (Ended, Option<String>) {
    let holds = |status: i32| (0..=255).contains(&status);
    if let Some(said) = said {
        return if holds(said) {
            (Ended::Exited(said), None)
        } else {
            let note = format!(
                "the command ended with status {said}, which no status here can hold: answering 1"
            );
            (Ended::Exited(1), Some(note))
        };
    }
    if matches!(code, None | Some(255)) && interrupt.heard_after_failure() {
        return (Ended::Stopped, None);
    }
    let no_line = format!("no `{EXIT_MARK}N` line ended the output");
    match code {
        Some(code) if code != 0 && holds(code) => {
            let note = format!("{no_line}, so ssh's own status is the answer: {code}");
            (Ended::Exited(code), Some(note))
        }
        Some(0) => {
            let note = format!("{no_line}, and ssh's status 0 is not taken for a command's without it: answering 1");
            (Ended::Exited(1), Some(note))
        }
        Some(code) => {
            let note = format!("{no_line}, and ssh's status {code} is no status here: answering 1");
            (Ended::Exited(1), Some(note))
        }
        None => {
            let note = format!("{no_line}, and ssh was ended by a signal: answering 1");
            (Ended::Exited(1), Some(note))
        }
    }
}

/// What the wait for output came to.
enum Watched {
    /// Both streams have closed, and all they said has been written.
    Closed,
    Stop,
    Unwritten(io::Error),
}

/// Writes what arrives until the streams close, a stop is heard, or a write
/// fails; what the standard output says is kept at its end in `tail` as it goes.
fn watch(
    arriving: &Receiver<Chunk>,
    console: &mut Console,
    interrupt: &Interrupt,
    tail: &mut Tail,
) -> Watched {
    loop {
        if interrupt.heard() {
            return Watched::Stop;
        }
        match arriving.recv_timeout(LOOK) {
            Ok((stream, bytes)) => {
                if stream == Stream::Out {
                    tail.push(&bytes);
                }
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
