//! The stand-in for the program in the window, for each of Claude Code, Devin and Muse Code: it
//! draws that harness's input box as the readers' recorded screens have it, echoes the human's keys
//! as such windows were seen to (in one read, in two with the cursor's move first, or with the
//! cursor's move alone while a faint hint still stands there), shows a paste, takes a message at an
//! Enter into a box that holds one, and keeps the truth the oracle reads.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use cf_proto::agents::Harness;

use super::sched;

/// The pty between the program and the host: what the program printed waits here until the
/// reader takes it, and the host asks it whether anything waits.
#[derive(Default)]
pub(crate) struct Pty {
    queue: Mutex<VecDeque<Vec<u8>>>,
    /// How many reads the program printed that the host has not yet counted.
    in_flight: std::sync::atomic::AtomicUsize,
    closed: Mutex<bool>,
}

impl Pty {
    pub(crate) fn emit(&self, bytes: Vec<u8>) {
        self.in_flight
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.queue.lock().unwrap().push_back(bytes);
    }

    /// Waits, as the pane's reader waits for its terminal, until the program has printed
    /// something or the pty is closed; takes nothing.
    pub(crate) fn wait(&self) {
        sched::wait_until("the program prints", || {
            self.holds() || *self.closed.lock().unwrap()
        });
    }

    /// The next read, which cannot wait: none where the pty is closed and empty.
    pub(crate) fn take(&self) -> Option<Vec<u8>> {
        self.queue.lock().unwrap().pop_front()
    }

    /// Whether the pty holds bytes not yet read: the host's question to the terminal.
    pub(crate) fn holds(&self) -> bool {
        !self.queue.lock().unwrap().is_empty()
    }

    /// The host counted a read.
    pub(crate) fn counted(&self) {
        self.in_flight
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// Whether every read the program printed is counted.
    pub(crate) fn empty(&self) -> bool {
        self.in_flight.load(std::sync::atomic::Ordering::SeqCst) == 0
    }

    pub(crate) fn close(&self) {
        *self.closed.lock().unwrap() = true;
    }
}

/// How the window echoes a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Echo {
    /// The box drawn again, in one read.
    Whole,
    /// The cursor's move in one read, and then the key in the next.
    Split,
    /// The cursor's move alone, a faint hint still where the key will be, and then the box drawn
    /// again.
    CursorOnly,
    /// The cursor's move to where it is, and then the key: the window's first read of an echo
    /// that says nothing yet (T-2139).
    Prefix,
    /// Nothing: a fixed schedule prints the echo itself, a read at a time.
    Silent,
}

const ROWS: u16 = 24;
/// The row of the box's first line, and its rules above and below.
const BOX: u16 = 20;

/// What the window knows and the oracle reads.
pub(crate) struct Program {
    harness: Harness,
    pty: Arc<Pty>,
    /// What the human typed and has not sent, as the window's box holds it.
    pub(crate) draft: String,
    /// The messages pasted into the box and not sent, in order.
    pub(crate) held: Vec<u32>,
    /// The messages the window took, in order.
    pub(crate) taken: Vec<u32>,
    /// What a rule says went wrong: nothing here while the checks hold.
    pub(crate) violations: Vec<String>,
    /// The next Enters are dropped: they came before the window had read the paste (T-760).
    pub(crate) swallow: u32,
    hint: bool,
    /// How wide the window draws its rules: its screen's width, as a program sizes them.
    pub(crate) rule: usize,
    pasting: Option<Vec<u8>>,
    echoes: VecDeque<Echo>,
    /// A dialog of the window's own waits for a key, in the box's place.
    pub(crate) dialog: bool,
    /// A picker of the window's own, in the box's place, with the filter typed into it: it
    /// takes keys until Escape or a digit, and draws each.
    picker: Option<String>,
    /// A window that has fallen behind on its input: what it has not taken waits here, in the
    /// order it came, and is all taken together when it goes on (the checks' assumption).
    stalled: Option<Vec<u8>>,
    /// The box holds a message of ours the window's own stop put back (N6).
    ours: bool,
    /// The window's box is one no reader reads: the screen draws no box at all (N6).
    boxless: bool,
}

impl Program {
    pub(crate) fn new(harness: Harness, pty: &Arc<Pty>) -> Self {
        Self {
            harness,
            pty: Arc::clone(pty),
            draft: String::new(),
            held: Vec::new(),
            taken: Vec::new(),
            violations: Vec::new(),
            swallow: 0,
            hint: true,
            rule: 40,
            pasting: None,
            echoes: VecDeque::new(),
            dialog: false,
            picker: None,
            stalled: None,
            ours: false,
            boxless: false,
        }
    }

    /// The window's own stop put our message back in its box (N6): the box holds it as a paste
    /// the window drew, and where `readable` is false the screen draws no box a reader knows.
    pub(crate) fn put_back(&mut self, readable: bool) {
        self.ours = true;
        self.boxless = !readable;
        self.emit(self.frame());
    }

    /// The window stops taking its input; it goes on printing.
    pub(crate) fn stall(&mut self) {
        self.stalled.get_or_insert_with(Vec::new);
    }

    /// The window takes all the input that waits, and draws what it did with it.
    pub(crate) fn resume(&mut self) {
        if let Some(waiting) = self.stalled.take() {
            if !waiting.is_empty() {
                self.take_input(&waiting);
            }
        }
    }

    /// The window takes the first `bytes` of the input that waits, and stays behind on the
    /// rest: what the checks assume no window does (the pinned limits).
    pub(crate) fn resume_taking(&mut self, bytes: usize) {
        let Some(mut waiting) = self.stalled.take() else {
            return;
        };
        let rest = waiting.split_off(bytes.min(waiting.len()));
        self.take_input(&waiting);
        self.stalled = Some(rest);
    }

    /// The dialog closes by itself, and the box is drawn again.
    pub(crate) fn close_dialog(&mut self) {
        self.dialog = false;
        self.emit(self.frame());
    }

    /// The window moves its cursor on the box's line, and prints nothing else.
    pub(crate) fn move_cursor(&self, column: usize) {
        self.emit(format!("\x1b[{BOX};{column}H").into_bytes());
    }

    /// A dialog of the window's own is shown in the box's place, which waits for a key.
    pub(crate) fn show_dialog(&mut self) {
        self.dialog = true;
        self.emit(self.frame());
    }

    /// A picker of the window's own is shown in the box's place.
    pub(crate) fn show_picker(&mut self) {
        self.picker = Some(String::new());
        self.emit(self.frame());
    }

    /// The styles the next keys are echoed in.
    pub(crate) fn echo_as(&mut self, styles: impl IntoIterator<Item = Echo>) {
        self.echoes.extend(styles);
    }

    fn mark(&self) -> char {
        match self.harness {
            Harness::Devin => '❭',
            _ => '❯',
        }
    }

    fn hint_words(&self) -> &'static str {
        match self.harness {
            Harness::Devin => "Ask Devin anything about your code",
            _ => "Try \"how do I log an error?\"",
        }
    }

    /// The window's whole screen, as it first draws it.
    pub(crate) fn frame(&self) -> Vec<u8> {
        let rule = "─".repeat(self.rule);
        let (top, status) = match self.harness {
            Harness::Devin => (format!("{rule} (bypass permissions on) ─"), "SWE-2 Medium"),
            Harness::Muse => (
                "── Voice input (⌥ + v to start) ────────".to_owned(),
                "  echo · YOLO",
            ),
            _ => (
                rule.clone(),
                "  ⏵⏵ bypass permissions on (shift+tab to cycle)",
            ),
        };
        let mut bytes = b"\x1b[2J\x1b[1;1H\x1b[2K".to_vec();
        if let Some(filter) = &self.picker {
            bytes.extend(format!("\x1b[{};1HFilter: {filter}", BOX - 4).as_bytes());
        }
        if self.dialog || self.picker.is_some() {
            bytes.extend(
                format!(
                    "\x1b[1;1HThe conversation\x1b[{};1HDo you want to proceed?\x1b[{};1H❯ 1. Yes\x1b[{};1H  2. No\x1b[{};1HEsc to cancel · Tab to amend",
                    BOX - 3,
                    BOX - 2,
                    BOX - 1,
                    BOX
                )
                .as_bytes(),
            );
            return bytes;
        }
        if self.boxless {
            bytes.extend(format!("\x1b[1;1HThe conversation\x1b[{BOX};1H⏺ 4").as_bytes());
            return bytes;
        }
        bytes.extend(format!("\x1b[1;1HThe conversation\x1b[{};1H{top}", BOX - 1).as_bytes());
        bytes.extend(format!("\x1b[{};1H{rule}", BOX + 1).as_bytes());
        bytes.extend(format!("\x1b[{};1H{status}", BOX + 2).as_bytes());
        bytes.extend(self.box_line());
        bytes
    }

    /// The box's line as it is now, and the cursor where it is.
    fn box_line(&self) -> Vec<u8> {
        if self.boxless {
            return format!("\x1b[{BOX};1H\x1b[2K⏺ 4").into_bytes();
        }
        let mark = self.mark();
        let (body, column) = if !self.held.is_empty() || self.ours {
            ("[Pasted text #1]".to_owned(), 3 + 16)
        } else if self.draft.is_empty() && self.hint {
            (format!("\x1b[2m{}\x1b[0m", self.hint_words()), 3)
        } else {
            (self.draft.clone(), 3 + self.draft.chars().count())
        };
        format!("\x1b[{BOX};1H\x1b[2K{mark} {body}\x1b[{BOX};{column}H").into_bytes()
    }

    fn column(&self) -> usize {
        3 + self.draft.chars().count()
    }

    fn emit(&self, bytes: Vec<u8>) {
        self.pty.emit(bytes);
    }

    /// The window prints its box again where it is: a colour reset and the cursor put back.
    pub(crate) fn repaint(&self) {
        let mut bytes = b"\x1b[0m".to_vec();
        let at = self.cursor();
        bytes.extend(format!("\x1b[{BOX};{at}H").as_bytes());
        self.emit(bytes);
    }

    /// The window prints a colour reset and nothing else: output that says nothing of a key.
    pub(crate) fn colour_reset(&self) {
        self.emit(b"\x1b[39m".to_vec());
    }

    fn cursor(&self) -> usize {
        if !self.held.is_empty() {
            19
        } else if self.draft.is_empty() && self.hint {
            3
        } else {
            self.column()
        }
    }

    /// The window prints a line of text on row `row`, which may wrap.
    pub(crate) fn print_line(&self, row: u16, text: &str) {
        let at = self.cursor();
        let mut bytes = format!("\x1b[{row};1H\x1b[2K{text}").into_bytes();
        bytes.extend(format!("\x1b[{BOX};{at}H").as_bytes());
        self.emit(bytes);
    }

    /// The window scrolls its lines by one, says so on its first line, and draws its frame again.
    pub(crate) fn scroll(&self) {
        let mut bytes = format!("\x1b[{ROWS};1H\n").into_bytes();
        bytes.extend(self.frame());
        bytes.extend(b"\x1b[1;1H\x1b[2Kscrolled");
        bytes.extend(format!("\x1b[{BOX};{}H", self.cursor()).into_bytes());
        self.emit(bytes);
    }

    /// The window draws its frame again, as after a resize.
    pub(crate) fn redraw(&self) {
        self.emit(self.frame());
    }

    /// Reads that come faster than they are drawn: `reads` of them.
    pub(crate) fn flood(&self, reads: usize) {
        let at = self.cursor();
        for _ in 0..reads {
            self.emit(format!("\x1b[0m\x1b[{BOX};{at}H").into_bytes());
        }
    }

    /// What the human's keys and a delivery's bytes do in the window, once it takes them.
    pub(crate) fn input(&mut self, bytes: &[u8]) {
        match self.stalled.as_mut() {
            Some(waiting) => waiting.extend_from_slice(bytes),
            None => self.take_input(bytes),
        }
    }

    fn take_input(&mut self, bytes: &[u8]) {
        let mut at = 0;
        while at < bytes.len() {
            if let Some(paste) = self.pasting.as_mut() {
                if bytes[at..].starts_with(b"\x1b[201~") {
                    let body = self.pasting.take().unwrap_or_default();
                    self.pasted(&body);
                    at += 6;
                } else {
                    paste.push(bytes[at]);
                    at += 1;
                }
                continue;
            }
            if bytes[at..].starts_with(b"\x1b[200~") {
                self.pasting = Some(Vec::new());
                at += 6;
                continue;
            }
            if let Some(filter) = self.picker.as_mut() {
                match bytes[at] {
                    0x1b if bytes.get(at + 1) == Some(&b'[') => at += 2,
                    0x1b | b'0'..=b'9' => self.picker = None,
                    byte if byte >= 0x20 => filter.push(char::from(byte)),
                    _ => {}
                }
                self.emit(self.frame());
                at += 1;
                continue;
            }
            // A dialog takes the first key that comes and closes; nothing of it is typed.
            if self.dialog {
                self.dialog = false;
                self.emit(self.frame());
                at += 1;
                continue;
            }
            match bytes[at] {
                b'\r' => self.enter(),
                0x7f => self.key(None),
                0x03 | 0x15 => self.cleared(),
                // Ctrl+W: the word before the cursor.
                0x17 => self.erase_word(),
                // An escape sequence (an arrow) changes nothing here.
                0x1b => {
                    at += if bytes.get(at + 1) == Some(&b'[') {
                        3
                    } else {
                        1
                    };
                    continue;
                }
                byte if byte >= 0x20 => self.key(Some(char::from(byte))),
                _ => {}
            }
            at += 1;
        }
    }

    /// Ctrl+W: the word before the cursor goes.
    fn erase_word(&mut self) {
        let kept = self.draft.trim_end();
        let kept = kept.rfind(' ').map_or(0, |space| space + 1);
        self.draft.truncate(kept);
        self.emit(self.box_line());
    }

    /// A key of the human's: a character typed, or a Backspace.
    fn key(&mut self, typed: Option<char>) {
        let before = self.column();
        let faint = self.draft.is_empty() && self.hint && self.held.is_empty();
        match typed {
            Some(typed) => self.draft.push(typed),
            None => {
                self.draft.pop();
            }
        }
        let style = match typed {
            Some(_) => self.echoes.pop_front().unwrap_or(Echo::Whole),
            None => Echo::Whole,
        };
        match (style, typed) {
            (Echo::Split, Some(typed)) => {
                self.emit(format!("\x1b[{BOX};{}H", before + 1).into_bytes());
                self.emit(format!("\x1b[{BOX};{before}H{typed}").into_bytes());
            }
            (Echo::Silent, Some(_)) => {}
            (Echo::Prefix, Some(typed)) => {
                self.emit(format!("\x1b[{BOX};{before}H").into_bytes());
                self.emit(format!("\x1b[{BOX};{before}H{typed}").into_bytes());
            }
            (Echo::CursorOnly, Some(typed)) if faint => {
                // The key is drawn where it will be, faint, the cursor where it was; the cursor's
                // move and the box drawn again come after.
                self.emit(
                    format!("\x1b[{BOX};{before}H\x1b[2m{typed}\x1b[0m\x1b[{BOX};{before}H")
                        .into_bytes(),
                );
                self.emit(format!("\x1b[{BOX};{}H", before + 1).into_bytes());
                self.emit(self.box_line());
            }
            _ => self.emit(self.box_line()),
        }
    }

    /// A paste in brackets reached the window: a message of ours, a line it holds the words of.
    fn pasted(&mut self, body: &[u8]) {
        let text = String::from_utf8_lossy(body);
        let Some(id) = text
            .strip_prefix("MSG")
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|digits| digits.parse::<u32>().ok())
        else {
            return;
        };
        if !self.draft.is_empty() {
            self.violations.push(format!(
                "a paste of m-{id} reached the window over the human's unsent typing {:?}",
                self.draft
            ));
        }
        if self.ours {
            self.violations.push(format!(
                "a paste of m-{id} went in over the words our own stop put back, which it did not clear"
            ));
        }
        if self.held.contains(&id) {
            self.violations
                .push(format!("m-{id} is in the window's box twice"));
        }
        self.held.push(id);
        self.emit(self.box_line());
    }

    /// Enter: the window takes what its box holds, unless it came before the window read it.
    fn enter(&mut self) {
        // An Enter that comes before the window has read what was pasted is dropped; the human's
        // own is not.
        if self.swallow > 0 && !self.held.is_empty() {
            self.swallow -= 1;
            return;
        }
        if self.held.is_empty() && self.draft.is_empty() {
            return;
        }
        for id in std::mem::take(&mut self.held) {
            if self.taken.contains(&id) {
                self.violations
                    .push(format!("the window took m-{id} twice"));
            }
            self.taken.push(id);
        }
        self.draft.clear();
        self.hint = true;
        self.emit(self.box_line());
    }

    /// The key that empties the box.
    fn cleared(&mut self) {
        self.ours = false;
        self.held.clear();
        self.draft.clear();
        self.emit(self.box_line());
    }
}

/// The program as the pane's writer sees it: what is written into the pty goes to it.
pub(crate) struct Written(pub(crate) Arc<Mutex<Program>>);

impl Write for Written {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().input(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
