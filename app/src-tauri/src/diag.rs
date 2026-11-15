//! The app's one door to its error log. The app is windowed: a Finder-launched one has no
//! terminal and a Windows one no console, so what it has to tell a person who reads
//! `<home>/app/app.log` it writes to stderr, which `run` points at that file before anything
//! else is said (`redirect_stderr`), and the daemon it starts inherits it. A line is said as
//! it is given: the caller's own prefix (`consensflow: …`) is part of it.

use std::fmt::Display;
use std::io::{self, Write};

/// Writes `line` to the error log.
pub(crate) fn say(line: impl Display) {
    #[cfg(test)]
    tests::heard(line.to_string());
    write_line(io::stderr().lock(), line);
}

fn write_line(mut out: impl Write, line: impl Display) {
    let _ = writeln!(out, "{line}");
}

#[cfg(test)]
pub(crate) use tests::said;

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::Mutex;

    static HEARD: Mutex<Vec<String>> = Mutex::new(Vec::new());

    pub(super) fn heard(line: String) {
        HEARD
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(line);
    }

    /// Every line said so far in this test process, by every test: a test looks for the line
    /// it caused by something only it names (a folder of its own).
    pub(crate) fn said() -> Vec<String> {
        HEARD
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    #[test]
    fn an_unwritable_error_log_does_not_panic() {
        #[derive(Default)]
        struct FailingWriter {
            attempts: usize,
        }

        impl Write for FailingWriter {
            fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
                self.attempts += 1;
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut out = FailingWriter::default();
        let result = catch_unwind(AssertUnwindSafe(|| {
            super::write_line(&mut out, "diagnostic")
        }));
        assert!(out.attempts > 0, "the diagnostic was not attempted");
        assert!(result.is_ok(), "a failed diagnostic write panicked");
    }

    #[test]
    fn a_diagnostic_is_written_as_one_line() {
        let mut out = Vec::new();
        super::write_line(&mut out, "diagnostic");
        assert_eq!(out, b"diagnostic\n");
    }
}
