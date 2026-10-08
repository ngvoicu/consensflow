//! The daemon's error output, which a person running `cf ui` reads and the app
//! keeps in its log.
//!
//! A reader that has gone from it is not a failure to say: `cf ui` runs the
//! stop then, and so does the daemon here, once, through the function it is
//! given.

use std::cell::RefCell;
use std::io::{self, ErrorKind, Write};

/// Where the daemon says what it would say on its error output.
pub struct Console {
    out: RefCell<Box<dyn Write>>,
    broken: Box<dyn Fn()>,
}

impl Console {
    /// The process's own error output; `broken` is run when nobody reads it
    /// any more.
    pub fn stderr(broken: impl Fn() + 'static) -> Self {
        Self::to(io::stderr(), broken)
    }

    /// Any place for what is said: a test's buffer, a file.
    pub fn to(out: impl Write + 'static, broken: impl Fn() + 'static) -> Self {
        Self {
            out: RefCell::new(Box::new(out)),
            broken: Box::new(broken),
        }
    }

    /// A line. A failure to write it is nobody's to hear, except that a pipe
    /// with no reader is told to `broken`.
    pub fn line(&self, text: &str) {
        let written = writeln!(self.out.borrow_mut(), "{text}");
        if let Err(failed) = written {
            if failed.kind() == ErrorKind::BrokenPipe {
                (self.broken)();
            }
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
