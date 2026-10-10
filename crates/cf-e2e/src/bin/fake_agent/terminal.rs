//! The window's terminal, read the way a TUI reads it: raw, a byte at a time,
//! a bracketed paste being text and Enter outside one submitting it. A paste
//! followed by Enter is one message.

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
    /// Control-C: the window ends.
    Interrupt,
}

/// The state of reading the input: what is typed so far, whether a paste is
/// open, and a marker or a key still arriving.
#[derive(Debug, Default)]
pub struct Input {
    buffer: String,
    pasting: bool,
    pending: String,
}

impl Input {
    /// Reads `chunk` of input, and what it asked of the window, in order.
    pub fn feed(&mut self, chunk: &str) -> Vec<Event> {
        let mut events = Vec::new();
        self.pending.push_str(chunk);
        while !self.pending.is_empty() {
            if let Some(rest) = self.pending.strip_prefix(PASTE_START) {
                self.pasting = true;
                self.pending = rest.to_owned();
            } else if let Some(rest) = self.pending.strip_prefix(PASTE_END) {
                self.pasting = false;
                self.pending = rest.to_owned();
                events.push(Event::Pasted(self.buffer.chars().count()));
            } else if self.pending.starts_with('\u{1b}') {
                // The start of a paste marker still arriving, or a key: Escape, ignored.
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
                        if !self.buffer.is_empty() {
                            events.push(Event::Submit(std::mem::take(&mut self.buffer)));
                        }
                    }
                    '\u{3}' => events.push(Event::Interrupt),
                    '\r' => self.buffer.push('\n'),
                    other => self.buffer.push(other),
                }
            }
        }
        events
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
    fn control_c_asks_the_window_to_end_and_text_is_read_whole_in_any_language() {
        assert_eq!(feed_all(&["\u{3}"]), [Event::Interrupt]);
        assert_eq!(
            feed_all(&["[ConsensFlow m-1 · T-1]\r"]),
            [Event::Submit("[ConsensFlow m-1 · T-1]".into())]
        );
    }
}
