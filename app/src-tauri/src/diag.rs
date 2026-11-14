//! The app's one door to its error log. The app is windowed: a Finder-launched one has no
//! terminal and a Windows one no console, so what it has to tell a person who reads
//! `<home>/app/app.log` it writes to stderr, which `run` points at that file before anything
//! else is said (`redirect_stderr`), and the daemon it starts inherits it. A line is said as
//! it is given: the caller's own prefix (`consensflow: …`) is part of it.

use std::fmt::Display;

/// Writes `line` to the error log.
#[allow(
    clippy::print_stderr,
    reason = "this is the app's door to its stderr, which is its error log"
)]
pub(crate) fn say(line: impl Display) {
    #[cfg(test)]
    tests::heard(line.to_string());
    eprintln!("{line}");
}

#[cfg(test)]
pub(crate) use tests::said;

#[cfg(test)]
mod tests {
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
}
