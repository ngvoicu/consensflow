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
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::{drain, ended_by, pid, Drain, Signal, HELD, POLL};
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
    errors: Arc<Mutex<Vec<u8>>>,
    /// The thread that keeps its error output, until an end of the program has
    /// waited for it.
    reader: Option<JoinHandle<()>>,
}

impl Spawned {
    /// The running `child` of `program`, with `first` queued to its input.
    pub(super) fn new(mut child: Child, program: String, first: Option<Vec<u8>>) -> Self {
        let errors_from = child.stderr.take();
        Self::reading(child, program, first, errors_from)
    }

    /// [`Spawned::new`], with the error output read from `errors_from`: the program's own,
    /// but for a test that reads it slowly.
    fn reading(
        mut child: Child,
        program: String,
        first: Option<Vec<u8>>,
        errors_from: Option<impl Read + Send + 'static>,
    ) -> Self {
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
        let Drain {
            read: errors,
            thread: reader,
        } = drain(errors_from);
        let output = child.stdout.take().map(BufReader::new);
        Self {
            child,
            program,
            input,
            output,
            errors,
            reader: Some(reader),
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

    /// Everything it has said on its error output so far: all of it once
    /// [`Spawned::wait`] has seen it end.
    pub fn errors(&self) -> String {
        String::from_utf8_lossy(&self.errors.lock().unwrap_or_else(PoisonError::into_inner))
            .into_owned()
    }

    /// How it ended, if it has. It is a look, not a wait: what it said on its error
    /// output may not all be kept yet, and a process it left that holds the pipe is
    /// not waited for ([`Spawned::wait`] does both, once).
    pub fn status(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// Whether it has ended ([`Spawned::status`]).
    pub fn has_exited(&mut self) -> bool {
        self.status().is_some()
    }

    /// Waits for it to end, up to `within`: how it ended, or none if it was
    /// still running then. An end is seen once what it said on its error output
    /// is all kept ([`Spawned::settle_errors`]).
    pub fn wait(&mut self, within: Duration) -> Result<Option<ExitStatus>> {
        let started = Instant::now();
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.settle_errors();
                    return Ok(Some(status));
                }
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

    /// Once it has ended, waits for the thread that keeps its error output to read
    /// to the end of the pipe: what it said is in the pipe by then, but a thread the
    /// system has not run yet has not kept it, and a case that read it at once found
    /// it short. A process it left that holds the pipe open is waited for [`HELD`]
    /// at most. Only the first end seen waits.
    fn settle_errors(&mut self) {
        if let Some(reader) = self.reader.take() {
            ended_by(reader, Instant::now() + HELD);
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

#[cfg(all(test, unix))]
mod tests {
    use std::process::Stdio;

    use super::*;

    /// `/bin/sh`, a case's own program, holding only its streams: the cases run side by side,
    /// and a shell (or the `sleep` it leaves) would hold what another closed.
    #[allow(clippy::disallowed_methods)] // The case's own program.
    fn sh() -> std::process::Command {
        let mut command = std::process::Command::new("/bin/sh");
        cf_process::hold_only_the_streams(&mut command);
        command
    }

    /// The shell the cases of this file start holds nothing of theirs but its streams.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_shell_a_case_starts_holds_nothing_of_the_cases_but_its_streams() {
        use std::os::fd::AsRawFd;

        use nix::fcntl::{fcntl, FcntlArg, FdFlag};

        let stray = std::fs::File::open("/dev/null").unwrap();
        fcntl(stray.as_raw_fd(), FcntlArg::F_SETFD(FdFlag::empty())).unwrap();
        let said = sh()
            .arg("-c")
            .arg(format!(
                "if [ -e /dev/fd/{} ]; then echo held; else echo clear; fi",
                stray.as_raw_fd()
            ))
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&said.stdout), "clear\n");
    }

    /// A reader that is slow to read what is there to read, as the thread that reads a
    /// program's error output may be on a loaded machine: it takes its time over the first.
    struct Slow<R>(R, bool);

    impl<R: Read> Read for Slow<R> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if !std::mem::replace(&mut self.1, true) {
                thread::sleep(Duration::from_millis(300));
            }
            self.0.read(buffer)
        }
    }

    /// A program says something on its error output and ends. The pipe holds its words
    /// before its status is known, and the thread that reads them is not the one that
    /// waits for the status: a case that reads `errors()` when `wait` answers must find
    /// them, whatever the reader was doing when the program ended.
    #[test]
    fn what_a_program_said_on_its_error_output_is_all_there_once_it_has_ended_though_the_reader_is_slow(
    ) {
        let mut child = sh()
            .args(["-c", "echo said >&2"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let slow = Slow(child.stderr.take().unwrap(), false);
        let mut program = Spawned::reading(child, "sh".to_owned(), None, Some(slow));
        let ended = program.wait(Duration::from_secs(60)).unwrap();
        assert!(ended.is_some());
        assert_eq!(program.errors(), "said\n");
    }

    /// A process the program left holds its error output open: the wait for the words is
    /// not for ever.
    #[test]
    fn a_program_that_left_a_process_holding_its_error_output_has_ended_all_the_same() {
        let mut program = Spawned::new(
            sh().args(["-c", "echo said >&2; /bin/sleep 60 & exit 0"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
            "sh".to_owned(),
            None,
        );
        let started = Instant::now();
        let ended = program.wait(Duration::from_secs(60)).unwrap();
        assert!(ended.is_some());
        assert_eq!(program.errors(), "said\n");
        assert!(started.elapsed() < HELD + Duration::from_secs(20));
    }
}
