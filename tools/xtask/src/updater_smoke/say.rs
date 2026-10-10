//! What the smoke says, in one place. The run is more than one thread (the case,
//! the feed's server, a reader of each app's output), and xtask's own streams
//! belong to the thread that was given them: the others hand their lines to the
//! one that holds them, which writes each as it comes.

use std::sync::mpsc;

/// One line, and the stream it is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// What the run reports: its progress and its result.
    Out(String),
    /// What went wrong beside it.
    Err(String),
}

/// A way to say a line, which any thread may have a copy of.
#[derive(Debug, Clone)]
pub struct Say(mpsc::Sender<Line>);

impl Say {
    /// A new way to say lines, and where they arrive, in order.
    pub fn channel() -> (Self, mpsc::Receiver<Line>) {
        let (sender, lines) = mpsc::channel();
        (Self(sender), lines)
    }

    /// Reports a line.
    pub fn out(&self, text: impl Into<String>) {
        // Nobody left to hear it is the run being over.
        let _ = self.0.send(Line::Out(text.into()));
    }

    /// Reports a line of what went wrong.
    pub fn err(&self, text: impl Into<String>) {
        let _ = self.0.send(Line::Err(text.into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_arrive_in_the_order_they_were_said_from_any_copy() {
        let (say, lines) = Say::channel();
        let other = say.clone();
        say.out("one");
        std::thread::spawn(move || other.err("two")).join().unwrap();
        say.out(String::from("three"));
        drop(say);
        assert_eq!(
            lines.iter().collect::<Vec<_>>(),
            [
                Line::Out("one".into()),
                Line::Err("two".into()),
                Line::Out("three".into())
            ]
        );
    }

    #[test]
    fn a_line_said_when_nobody_is_left_to_hear_is_no_failure() {
        let (say, lines) = Say::channel();
        drop(lines);
        say.out("nobody hears");
        say.err("nobody hears this either");
    }
}
