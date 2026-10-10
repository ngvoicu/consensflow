//! A program left running for a case to drive: the daemon and the pane host the
//! rig starts, a supervisor a case ends by a signal. Its three streams are
//! pipes. What the case writes is queued and written on a thread of its own, as
//! Node's streams queue it, so that writing never waits on a program that is
//! itself waiting for its output to be read; what it said on its error output
//! is kept as it arrives; its output is the case's to read, a line at a time.

use std::io::{BufReader, Read, Write};
use std::process::{Child, ChildStdout, ExitStatus};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use super::{pid, Signal, POLL};
use crate::{Error, Result};

/// What goes down the queue to a program's input.
enum Message {
    Bytes(Vec<u8>),
    /// The input is ended: everything before this is written first.
    End,
}

/// Where a program's input is written, from any thread. A clone writes to the
/// same program in the same order.
#[derive(Debug, Clone)]
pub struct Sink {
    queue: Sender<Message>,
}

impl Sink {
    /// Queues `bytes` for the program's input. A program whose input is ended,
    /// or gone, is written to no more; that is no matter to say.
    pub fn send(&self, bytes: impl Into<Vec<u8>>) {
        let _ = self.queue.send(Message::Bytes(bytes.into()));
    }

    /// Ends the input once everything queued is written, as Node's `end` does.
    pub fn end(&self) {
        let _ = self.queue.send(Message::End);
    }
}

/// A program that is running, and what a case does with it.
#[derive(Debug)]
pub struct Spawned {
    child: Child,
    program: String,
    /// None when the input was never a pipe.
    input: Option<Sink>,
    output: Option<BufReader<ChildStdout>>,
    errors: Arc<Mutex<String>>,
}

impl Spawned {
    /// The running `child` of `program`, with `first` queued to its input.
    pub(super) fn new(mut child: Child, program: String, first: Option<Vec<u8>>) -> Self {
        let input = child.stdin.take().map(|mut pipe| {
            let (queue, queued) = mpsc::channel::<Message>();
            thread::spawn(move || {
                for message in queued {
                    match message {
                        Message::Bytes(bytes) => {
                            // A program that is gone from its input ends the writing.
                            if pipe.write_all(&bytes).and_then(|()| pipe.flush()).is_err() {
                                break;
                            }
                        }
                        Message::End => break,
                    }
                }
                // The input is ended or broken: the pipe is dropped here, closed.
            });
            Sink { queue }
        });
        if let (Some(sink), Some(bytes)) = (&input, first) {
            sink.send(bytes);
        }
        let errors = Arc::new(Mutex::new(String::new()));
        if let Some(mut pipe) = child.stderr.take() {
            let said = Arc::clone(&errors);
            thread::spawn(move || {
                let mut chunk = [0_u8; 4096];
                while let Ok(read) = pipe.read(&mut chunk) {
                    if read == 0 {
                        break;
                    }
                    said.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push_str(&String::from_utf8_lossy(&chunk[..read]));
                }
            });
        }
        let output = child.stdout.take().map(BufReader::new);
        Self {
            child,
            program,
            input,
            output,
            errors,
        }
    }

    /// Its process id.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Where its input is written from, if it has an input to write to.
    pub fn sink(&self) -> Option<Sink> {
        self.input.clone()
    }

    /// Queues `bytes` for its input.
    pub fn send(&self, bytes: impl Into<Vec<u8>>) {
        if let Some(sink) = &self.input {
            sink.send(bytes);
        }
    }

    /// Ends its input once everything queued is written.
    pub fn end_input(&self) {
        if let Some(sink) = &self.input {
            sink.end();
        }
    }

    /// Hands over its output, to be read a line at a time on a thread of the
    /// case's own. It is given out once.
    pub fn take_output(&mut self) -> Option<BufReader<ChildStdout>> {
        self.output.take()
    }

    /// Everything it has said on its error output so far.
    pub fn errors(&self) -> String {
        self.errors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Whether it has ended.
    pub fn has_exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// Waits for it to end, up to `within`: how it ended, or none if it was
    /// still running then.
    pub fn wait(&mut self, within: Duration) -> Result<Option<ExitStatus>> {
        let started = Instant::now();
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => return Ok(Some(status)),
                Ok(None) => {}
                Err(source) => {
                    return Err(Error::Program {
                        action: "wait for",
                        program: self.program.clone(),
                        source,
                    })
                }
            }
            if started.elapsed() >= within {
                return Ok(None);
            }
            thread::sleep(POLL);
        }
    }

    /// Sends it `signal`. A program that is gone is no error.
    pub fn signal(&self, signal: Signal) {
        let _ = pid::signal(self.id(), signal);
    }

    /// Ends it at once, and waits for it to be gone.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Spawned {
    /// A program the case did not see to its end is not left behind.
    fn drop(&mut self) {
        self.kill();
    }
}
