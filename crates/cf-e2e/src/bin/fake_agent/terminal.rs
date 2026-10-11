//! The window's terminal, read the way a TUI reads it: raw, a byte at a time,
//! a bracketed paste being text and Enter outside one submitting it. A paste
//! followed by Enter is one message. Escape, alone, stops the turn at work; the
//! first Control-C clears what is typed, as Claude's does, and a second one in
//! a row ends the window.

/// What starts a paste, and what ends it.
const PASTE_START: &str = "\u{1b}[200~";
const PASTE_END: &str = "\u{1b}[201~";

/// What the input asks of the window as it is read.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// A message was submitted.
    Submit(String),
    /// A paste ended, with this many characters in the buffer: a real TUI draws
    /// what was pasted, and a paste's Enter waits for that.
    Pasted(usize),
    /// Escape, by itself: the turn at work stops.
    Escape,
    /// Control-C a second time in a row: the window ends.
    Interrupt,
}

/// The state of reading the input: what is typed so far, whether a paste is
/// open, a marker or a key still arriving, and whether a Control-C is waiting
/// for another to end the window ("Press Ctrl-C again to exit", which anything
/// typed or pasted after it cancels).
#[derive(Debug, Default)]
pub struct Input {
    buffer: String,
    pasting: bool,
    pending: String,
    armed: bool,
}

impl Input {
    /// Reads `chunk` of input, and what it asked of the window, in order.
    pub fn feed(&mut self, chunk: &str) -> Vec<Event> {
        let mut events = Vec::new();
        self.pending.push_str(chunk);
        while !self.pending.is_empty() {
            if let Some(rest) = self.pending.strip_prefix(PASTE_START) {
                self.pasting = true;
                self.armed = false;
                self.pending = rest.to_owned();
            } else if let Some(rest) = self.pending.strip_prefix(PASTE_END) {
                self.pasting = false;
                self.pending = rest.to_owned();
                events.push(Event::Pasted(self.buffer.chars().count()));
            } else if self.pending.starts_with('\u{1b}') {
                // The start of a paste marker still arriving, which `quiet`
                // takes for the Escape key if nothing more comes; or a key
                // that is not one, which is dropped.
                if [PASTE_START, PASTE_END]
                    .iter()
                    .any(|marker| marker.starts_with(self.pending.as_str()))
                {
                    break;
                }
                self.pending.remove(0);
            } else {
                let Some(char) = self.pending.chars().next() else {
                    break;
                };
                self.pending.remove(0);
                match char {
                    '\r' if !self.pasting => {
                        self.armed = false;
                        if !self.buffer.is_empty() {
                            events.push(Event::Submit(std::mem::take(&mut self.buffer)));
                        }
                    }
                    '\u{3}' => {
                        self.buffer.clear();
                        if std::mem::replace(&mut self.armed, true) {
                            events.push(Event::Interrupt);
                        }
                    }
                    '\r' => self.buffer.push('\n'),
                    other => {
                        self.armed = false;
                        self.buffer.push(other);
                    }
                }
            }
        }
        events
    }

    /// Whether an Escape waits to be told from the start of a paste marker.
    pub fn holds_escape(&self) -> bool {
        self.pending == "\u{1b}"
    }

    /// Nothing more came after an Escape that could have begun a paste marker:
    /// it was the Escape key, as a TUI settles it after a moment.
    pub fn quiet(&mut self) -> Vec<Event> {
        if self.holds_escape() {
            self.pending.clear();
            return vec![Event::Escape];
        }
        Vec::new()
    }
}

/// Puts the terminal on the standard input in raw mode, as a TUI does: no echo,
/// no line editing, no signals from the keyboard, and the escape sequences
/// that a terminal sends for a paste passed on as they are. Nothing to do
/// where the input is no terminal.
#[cfg(unix)]
pub fn raw_mode() {
    use std::io::IsTerminal;

    use nix::sys::termios::{
        tcgetattr, tcsetattr, ControlFlags, InputFlags, LocalFlags, OutputFlags, SetArg,
        SpecialCharacterIndices,
    };

    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return;
    }
    // What Node's `setRawMode(true)` sets: libuv's raw mode.
    let Ok(mut terminal) = tcgetattr(&stdin) else {
        return;
    };
    terminal.input_flags.remove(
        InputFlags::BRKINT
            | InputFlags::ICRNL
            | InputFlags::INPCK
            | InputFlags::ISTRIP
            | InputFlags::IXON,
    );
    terminal.output_flags.insert(OutputFlags::ONLCR);
    terminal.control_flags.insert(ControlFlags::CS8);
    terminal
        .local_flags
        .remove(LocalFlags::ECHO | LocalFlags::ICANON | LocalFlags::IEXTEN | LocalFlags::ISIG);
    terminal.control_chars[SpecialCharacterIndices::VMIN as usize] = 1;
    terminal.control_chars[SpecialCharacterIndices::VTIME as usize] = 0;
    let _ = tcsetattr(&stdin, SetArg::TCSADRAIN, &terminal);
}

/// Puts the console on the standard input in raw mode with virtual terminal
/// input, as libuv does for Node: the keys, and a paste's escape sequences,
/// come as the characters a terminal sends. Nothing to do where the input is
/// no console.
#[cfg(windows)]
pub fn raw_mode() {
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_VIRTUAL_TERMINAL_INPUT,
        ENABLE_WINDOW_INPUT, STD_INPUT_HANDLE,
    };

    // SAFETY: the standard input handle is the process's own and stays open;
    // `GetConsoleMode` writes one `u32` through a pointer to a local, and fails
    // with a zero for a handle that is no console, which ends this.
    unsafe {
        let input = GetStdHandle(STD_INPUT_HANDLE);
        let mut mode = 0_u32;
        if GetConsoleMode(input, &mut mode) == 0 {
            return;
        }
        // No line input, no echo, no processing of the keys: only these.
        let _ = SetConsoleMode(input, ENABLE_WINDOW_INPUT | ENABLE_VIRTUAL_TERMINAL_INPUT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(chunks: &[&str]) -> Vec<Event> {
        let mut input = Input::default();
        chunks.iter().flat_map(|chunk| input.feed(chunk)).collect()
    }

    #[test]
    fn enter_outside_a_paste_submits_what_was_typed_and_an_empty_line_submits_nothing() {
        assert_eq!(
            feed_all(&["hel", "lo\r\r", "bye\r"]),
            [Event::Submit("hello".into()), Event::Submit("bye".into())]
        );
    }

    #[test]
    fn a_paste_is_text_whatever_it_holds_and_its_enter_afterwards_submits_it_whole() {
        let events = feed_all(&["\u{1b}[200~one\rtwo\nthree\u{1b}[201~", "\r"]);
        assert_eq!(
            events,
            [Event::Pasted(13), Event::Submit("one\ntwo\nthree".into())]
        );
    }

    #[test]
    fn a_marker_that_arrives_in_pieces_is_waited_for_and_a_key_is_dropped() {
        let events = feed_all(&["\u{1b}", "[20", "0~pasted\u{1b}[201", "~\r"]);
        assert_eq!(events, [Event::Pasted(6), Event::Submit("pasted".into())]);
        // An arrow key is an Escape and two characters: the Escape is dropped, the rest is text.
        assert_eq!(feed_all(&["a\u{1b}[Ab\r"]), [Event::Submit("a[Ab".into())]);
    }

    #[test]
    fn text_is_read_whole_in_any_language() {
        assert_eq!(
            feed_all(&["[ConsensFlow m-1 · T-1]\r"]),
            [Event::Submit("[ConsensFlow m-1 · T-1]".into())]
        );
    }

    #[test]
    fn the_first_control_c_clears_what_is_typed_and_a_second_in_a_row_ends_the_window() {
        assert_eq!(feed_all(&["\u{3}"]), []);
        assert_eq!(feed_all(&["\u{3}\u{3}"]), [Event::Interrupt]);
        // What was typed is gone, and what is pasted after it goes in alone.
        assert_eq!(
            feed_all(&["old\u{3}", "\u{1b}[200~new\u{1b}[201~", "\r"]),
            [Event::Pasted(3), Event::Submit("new".into())]
        );
        // A paste, or anything typed, between two of them makes the second a first again.
        assert_eq!(
            feed_all(&["\u{3}", "\u{1b}[200~x\u{1b}[201~", "\u{3}"]),
            [Event::Pasted(1)]
        );
        assert_eq!(feed_all(&["\u{3}", "x", "\u{3}", "\r"]), []);
    }

    #[test]
    fn an_escape_that_nothing_follows_is_the_escape_key_and_one_that_begins_a_paste_is_not() {
        let mut input = Input::default();
        assert_eq!(input.feed("\u{1b}"), []);
        assert!(input.holds_escape());
        assert_eq!(input.quiet(), [Event::Escape]);
        assert!(!input.holds_escape());
        assert_eq!(input.quiet(), [], "an Escape is pressed once");

        // The same Escape, with the rest of a paste marker coming in time.
        let mut input = Input::default();
        assert_eq!(input.feed("\u{1b}"), []);
        assert_eq!(
            input.feed("[200~hi\u{1b}[201~\r"),
            [Event::Pasted(2), Event::Submit("hi".into())]
        );
        assert_eq!(input.quiet(), []);

        // Half a marker is no Escape key: nothing says what it was.
        let mut input = Input::default();
        assert_eq!(input.feed("\u{1b}[20"), []);
        assert_eq!(input.quiet(), []);
    }
}
