//! The daemon's error output, which a person running `cf ui` reads and the app
//! keeps in its log.
//!
//! A reader that has gone from it is not a failure to say: `cf ui` runs the
//! stop then, and so does the daemon here, once, through the function it is
//! given.

use std::io::{self, ErrorKind, Write};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use crate::files::SETTLE;
use crate::halt::Halt;

/// Where the daemon says what it would say on its error output.
pub struct Console {
    out: Arc<Mutex<Box<dyn Write + Send>>>,
    broken: Box<dyn Fn()>,
    /// Once the daemon is ending (a stop, a break) a line is written on a thread off the
    /// runtime and waited for [`SETTLE`] at most: a reader that keeps the pipe open and
    /// drains nothing must keep nothing from stopping.
    halt: OnceLock<Arc<Halt>>,
}

impl Console {
    /// The process's own error output; `broken` is run when nobody reads it
    /// any more.
    pub fn stderr(broken: impl Fn() + 'static) -> Self {
        Self::to(io::stderr(), broken)
    }

    /// Any place for what is said: a test's buffer, a file.
    pub fn to(out: impl Write + Send + 'static, broken: impl Fn() + 'static) -> Self {
        Self {
            out: Arc::new(Mutex::new(Box::new(out))),
            broken: Box::new(broken),
            halt: OnceLock::new(),
        }
    }

    /// From the moment `halt` says the daemon is ending, nothing on the runtime writes here
    /// directly.
    pub fn ends_with(&self, halt: Arc<Halt>) {
        let _ = self.halt.set(halt);
    }

    /// A line. A failure to write it is nobody's to hear, except that a pipe
    /// with no reader is told to `broken`.
    pub fn line(&self, text: &str) {
        if self.halt.get().is_some_and(|halt| halt.stopping()) {
            return self.line_apart(text);
        }
        let written = writeln!(self.out.lock().unwrap_or_else(|e| e.into_inner()), "{text}");
        if let Err(failed) = written {
            if failed.kind() == ErrorKind::BrokenPipe {
                (self.broken)();
            }
        }
    }

    /// The line, written by a thread that is waited for a moment and let go; a thread that
    /// cannot be made loses the line.
    fn line_apart(&self, text: &str) {
        let (out, text) = (Arc::clone(&self.out), text.to_owned());
        let (done, heard) = mpsc::channel();
        let started = thread::Builder::new()
            .name("consensflow-console".to_owned())
            .spawn(move || {
                let _ = writeln!(out.lock().unwrap_or_else(|e| e.into_inner()), "{text}");
                let _ = done.send(());
            });
        if started.is_ok() {
            let _ = heard.recv_timeout(SETTLE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Said;
    use std::cell::Cell;
    use std::rc::Rc;

    /// An output nobody reads.
    struct Gone;

    impl Write for Gone {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(ErrorKind::BrokenPipe))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::from(ErrorKind::BrokenPipe))
        }
    }

    /// An output whose reader keeps the pipe and drains nothing: a write waits until it is let go.
    struct Stuck(Arc<(Mutex<bool>, std::sync::Condvar)>);

    impl Write for Stuck {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let (open, opened) = &*self.0;
            let mut open = open.lock().unwrap();
            while !*open {
                open = opened.wait(open).unwrap();
            }
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_line_said_while_the_daemon_ends_does_not_wait_for_a_pipe_nobody_drains() {
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let console = Console::to(Stuck(Arc::clone(&gate)), || {});
        let halt = Halt::new();
        console.ends_with(Arc::clone(&halt));
        halt.break_ledger();
        // Let go after a while, so that a plant ends late, not never.
        let letting_go = {
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(4));
                *gate.0.lock().unwrap() = true;
                gate.1.notify_all();
            })
        };
        let began = std::time::Instant::now();
        console.line("consensflow dispatcher: after the hook");
        console.line("and another");
        assert!(
            began.elapsed() < std::time::Duration::from_secs(2),
            "{:?}",
            began.elapsed()
        );
        letting_go.join().unwrap();
    }

    #[test]
    fn a_line_is_written_with_its_newline() {
        let said = Said::default();
        let console = Console::to(said.clone(), || {});
        console.line("consensflow dispatcher: boom");
        console.line("again");
        assert_eq!(said.text(), "consensflow dispatcher: boom\nagain\n");
    }

    #[test]
    fn a_pipe_nobody_reads_is_told_to_the_one_that_stops_the_daemon() {
        let told = Rc::new(Cell::new(0));
        let counted = Rc::clone(&told);
        let console = Console::to(Gone, move || counted.set(counted.get() + 1));
        console.line("nobody hears");
        assert_eq!(told.get(), 1);
    }

    #[test]
    fn any_other_failure_is_nobody_s_to_hear() {
        struct Full;
        impl Write for Full {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("full"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let told = Rc::new(Cell::new(0));
        let counted = Rc::clone(&told);
        Console::to(Full, move || counted.set(1)).line("lost");
        assert_eq!(told.get(), 0);
    }
}
