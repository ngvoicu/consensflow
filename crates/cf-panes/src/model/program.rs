//! The stand-in for the program in the window, for each of Claude Code, Devin and Muse Code: it
//! draws that harness's input box as the readers' recorded screens have it, echoes the human's keys
//! as such windows were seen to (in one read, in two with the cursor's move first, or with the
//! cursor's move alone while a faint hint still stands there), shows a paste, takes a message at an
//! Enter into a box that holds one, and keeps the truth the oracle reads.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cf_proto::agents::Harness;

use super::clock::ModelClock;
use super::sched;
use crate::arbiter::Clock as _;

/// What the program printed, in one read of the pty, and when it must have been read at the latest.
pub(crate) struct Chunk {
    pub(crate) bytes: Vec<u8>,
    pub(crate) due: Option<Duration>,
}

/// The pty between the program and the host: what the program printed waits here until the
/// reader takes it.
#[derive(Default)]
pub(crate) struct Pty {
    queue: Mutex<VecDeque<Chunk>>,
    /// The dues of the reads not yet counted: in the queue, or in the reader's hand.
    unread: Mutex<Vec<Duration>>,
    closed: Mutex<bool>,
}

impl Pty {
    pub(crate) fn emit(&self, bytes: Vec<u8>, due: Option<Duration>) {
        if let Some(due) = due {
            self.unread.lock().unwrap().push(due);
        }
        self.queue.lock().unwrap().push_back(Chunk { bytes, due });
    }

    /// The next read, once the program has printed one; none once the pty is closed and empty.
    pub(crate) fn next(&self) -> Option<Chunk> {
        sched::wait_until("the program prints", || {
            !self.queue.lock().unwrap().is_empty() || *self.closed.lock().unwrap()
        });
        self.queue.lock().unwrap().pop_front()
    }

    /// The host counted that read.
    pub(crate) fn counted(&self, due: Option<Duration>) {
        if let Some(due) = due {
            let mut unread = self.unread.lock().unwrap();
            if let Some(at) = unread.iter().position(|each| *each == due) {
                unread.swap_remove(at);
            }
        }
    }

    /// Whether a read is due before `time` and not yet counted.
    pub(crate) fn overdue(&self, time: Duration) -> bool {
        self.unread.lock().unwrap().iter().any(|due| *due < time)
    }

    pub(crate) fn empty(&self) -> bool {
        self.queue.lock().unwrap().is_empty()
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
}

const ROWS: u16 = 24;
const COLS: u16 = 80;
/// The row of the box's first line, and its rules above and below.
const BOX: u16 = 20;
const ECHO_WITHIN: Duration = Duration::from_millis(900);

/// What the window knows and the oracle reads.
pub(crate) struct Program {
    harness: Harness,
    clock: Arc<ModelClock>,
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
    pasting: Option<Vec<u8>>,
    echoes: VecDeque<Echo>,
}

impl Program {
    pub(crate) fn new(harness: Harness, clock: &Arc<ModelClock>, pty: &Arc<Pty>) -> Self {
        Self {
            harness,
            clock: Arc::clone(clock),
            pty: Arc::clone(pty),
            draft: String::new(),
            held: Vec::new(),
            taken: Vec::new(),
            violations: Vec::new(),
            swallow: 0,
            hint: true,
            pasting: None,
            echoes: VecDeque::new(),
        }
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

    fn due(&self) -> Option<Duration> {
        Some(self.clock.elapsed() + ECHO_WITHIN)
    }

    /// The window's whole screen, as it first draws it.
    pub(crate) fn frame(&self) -> Vec<u8> {
        let rule = "─".repeat(40);
        let (top, status) = match self.harness {
            Harness::Devin => (format!("{rule} (bypass permissions on) ─"), "SWE-2 Medium"),
            Harness::Muse => ("── Voice input (⌥ + v to start) ────────".to_owned(), "  echo · YOLO"),
            _ => (rule.clone(), "  ⏵⏵ bypass permissions on (shift+tab to cycle)"),
        };
        let mut bytes = b"\x1b[2J\x1b[1;1H\x1b[2K".to_vec();
        bytes.extend(format!("\x1b[1;1HThe conversation\x1b[{};1H{top}", BOX - 1).as_bytes());
        bytes.extend(format!("\x1b[{};1H{rule}", BOX + 1).as_bytes());
        bytes.extend(format!("\x1b[{};1H{status}", BOX + 2).as_bytes());
        bytes.extend(self.box_line());
        bytes
    }

    /// The box's line as it is now, and the cursor where it is.
    fn box_line(&self) -> Vec<u8> {
        let mark = self.mark();
        let (body, column) = if !self.held.is_empty() {
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

    fn emit(&self, bytes: Vec<u8>, due: Option<Duration>) {
        self.pty.emit(bytes, due);
    }

    /// The window prints its box again where it is: a colour reset and the cursor put back.
    pub(crate) fn repaint(&self) {
        let mut bytes = b"\x1b[0m".to_vec();
        let at = self.cursor();
        bytes.extend(format!("\x1b[{BOX};{at}H").as_bytes());
        self.emit(bytes, None);
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
        self.emit(bytes, None);
    }

    /// The window scrolls its lines by one and draws its frame again.
    pub(crate) fn scroll(&self) {
        let mut bytes = format!("\x1b[{ROWS};1H\n").into_bytes();
        bytes.extend(self.frame());
        self.emit(bytes, None);
    }

    /// The window draws its frame again, as after a resize.
    pub(crate) fn redraw(&self) {
        self.emit(self.frame(), None);
    }

    /// Reads that come faster than they are drawn: `reads` of them.
    pub(crate) fn flood(&self, reads: usize) {
        let at = self.cursor();
        for _ in 0..reads {
            self.emit(format!("\x1b[0m\x1b[{BOX};{at}H").into_bytes(), None);
        }
    }

    /// What the human's keys and a delivery's bytes do in the window.
    pub(crate) fn input(&mut self, bytes: &[u8]) {
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
            match bytes[at] {
                b'\r' => self.enter(),
                0x7f => self.key(None),
                0x03 => self.cleared(),
                byte if byte >= 0x20 => self.key(Some(char::from(byte))),
                _ => {}
            }
            at += 1;
        }
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
        let due = self.due();
        match (style, typed) {
            (Echo::Split, Some(typed)) => {
                self.emit(format!("\x1b[{BOX};{}H", before + 1).into_bytes(), due);
                self.emit(format!("\x1b[{BOX};{before}H{typed}").into_bytes(), due);
            }
            (Echo::CursorOnly, Some(_)) if faint => {
                self.emit(format!("\x1b[{BOX};{}H", before + 1).into_bytes(), due);
                self.emit(self.box_line(), due);
            }
            _ => self.emit(self.box_line(), due),
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
        if self.held.contains(&id) {
            self.violations
                .push(format!("m-{id} is in the window's box twice"));
        }
        self.held.push(id);
        let due = self.due();
        self.emit(self.box_line(), due);
    }

    /// Enter: the window takes what its box holds, unless it came before the window read it.
    fn enter(&mut self) {
        if self.swallow > 0 {
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
        let due = self.due();
        self.emit(self.box_line(), due);
    }

    /// The key that empties the box.
    fn cleared(&mut self) {
        self.held.clear();
        self.draft.clear();
        let due = self.due();
        self.emit(self.box_line(), due);
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
