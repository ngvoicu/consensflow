//! The bytes a program prints, read as a terminal reads them: text, control
//! characters, and the escape sequences that move the cursor, erase and
//! style. [`Parser`] turns them into [`Action`]s and keeps no screen; what an
//! action does is the grid's. The strings a terminal answers with (title,
//! device attributes) are read and dropped: nothing here talks back.

/// The most bytes of an OSC, DCS, APC, PM or SOS string that are read before
/// the string is given up: one that never ends must not hide what follows.
const MAX_STRING: usize = 8 * 1024;
/// The most parameters of a CSI sequence that are kept.
const MAX_PARAMS: usize = 32;
/// The largest value a parameter takes.
const MAX_PARAM: usize = 0xFFFF;

const ESC: u8 = 0x1B;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1A;
const BEL: u8 = 0x07;

/// What a run of bytes said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Action {
    /// A character to put at the cursor.
    Print(char),
    /// A C0 control character other than ESC: a carriage return, a line feed.
    Control(u8),
    /// `ESC`, an intermediate if there was one, and a final byte: `ESC 7`,
    /// `ESC M`, `ESC ( B`.
    Escape { intermediate: Option<u8>, last: u8 },
    /// `ESC [`, a marker (`?`, `>`, `<`, `=`), its parameters (0 for one left
    /// out), an intermediate and a final byte.
    Csi {
        marker: Option<u8>,
        params: Vec<usize>,
        intermediate: Option<u8>,
        last: u8,
    },
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    Escape,
    EscapeIntermediate,
    Csi,
    /// A CSI sequence that cannot be one: read to its final byte and dropped.
    CsiIgnore,
    /// An OSC, DCS, APC, PM or SOS string, dropped.
    Str,
    /// An ESC inside such a string: `ESC \` ends it.
    StrEscape,
}

/// Where a run of bytes is: possibly between the halves of a character or of
/// an escape sequence, as a program's output arrives in chunks of any size.
#[derive(Debug, Default)]
pub(super) struct Parser {
    state: State,
    /// The start of a UTF-8 character that has not arrived whole, and how long
    /// it is to be.
    partial: Vec<u8>,
    needed: usize,
    marker: Option<u8>,
    params: Vec<usize>,
    digits: Option<usize>,
    intermediate: Option<u8>,
    /// How much of a string has been dropped.
    dropped: usize,
}

impl Parser {
    /// Reads one byte, handing `perform` what it completes.
    pub(super) fn advance(&mut self, byte: u8, perform: &mut dyn FnMut(Action)) {
        match self.state {
            State::Ground => self.ground(byte, perform),
            State::Str => self.string(byte),
            State::StrEscape => self.string_escape(byte, perform),
            _ if byte == CAN || byte == SUB => self.state = State::Ground,
            _ if byte == ESC => self.begin_escape(),
            _ if byte < 0x20 => perform(Action::Control(byte)),
            State::Escape => self.escape(byte, perform),
            State::EscapeIntermediate => self.escape_intermediate(byte, perform),
            State::Csi => self.csi(byte, perform),
            State::CsiIgnore => {
                if (0x40..=0x7E).contains(&byte) {
                    self.state = State::Ground;
                }
            }
        }
    }

    fn begin_escape(&mut self) {
        self.state = State::Escape;
        self.intermediate = None;
    }

    fn ground(&mut self, byte: u8, perform: &mut dyn FnMut(Action)) {
        if !self.partial.is_empty() {
            if (0x80..=0xBF).contains(&byte) {
                self.partial.push(byte);
                if self.partial.len() == self.needed {
                    let character = std::str::from_utf8(&self.partial)
                        .ok()
                        .and_then(|text| text.chars().next())
                        .unwrap_or(char::REPLACEMENT_CHARACTER);
                    self.partial.clear();
                    perform(Action::Print(character));
                }
                return;
            }
            // The character was cut short by something else: it is lost.
            self.partial.clear();
            perform(Action::Print(char::REPLACEMENT_CHARACTER));
        }
        match byte {
            ESC => self.begin_escape(),
            CAN | SUB => {}
            0x00..=0x1F => perform(Action::Control(byte)),
            0x20..=0x7E => perform(Action::Print(char::from(byte))),
            0x7F => {}
            0xC2..=0xDF => self.start_character(byte, 2),
            0xE0..=0xEF => self.start_character(byte, 3),
            0xF0..=0xF4 => self.start_character(byte, 4),
            _ => perform(Action::Print(char::REPLACEMENT_CHARACTER)),
        }
    }

    fn start_character(&mut self, byte: u8, length: usize) {
        self.partial.push(byte);
        self.needed = length;
    }

    fn escape(&mut self, byte: u8, perform: &mut dyn FnMut(Action)) {
        match byte {
            b'[' => {
                self.state = State::Csi;
                self.marker = None;
                self.params.clear();
                self.digits = None;
                self.intermediate = None;
            }
            b']' | b'P' | b'X' | b'^' | b'_' => {
                self.state = State::Str;
                self.dropped = 0;
            }
            0x20..=0x2F => {
                self.intermediate = Some(byte);
                self.state = State::EscapeIntermediate;
            }
            0x30..=0x7E => {
                self.state = State::Ground;
                perform(Action::Escape {
                    intermediate: None,
                    last: byte,
                });
            }
            _ => self.state = State::Ground,
        }
    }

    fn escape_intermediate(&mut self, byte: u8, perform: &mut dyn FnMut(Action)) {
        match byte {
            0x20..=0x2F => self.intermediate = Some(byte),
            0x30..=0x7E => {
                self.state = State::Ground;
                perform(Action::Escape {
                    intermediate: self.intermediate.take(),
                    last: byte,
                });
            }
            _ => self.state = State::Ground,
        }
    }

    fn csi(&mut self, byte: u8, perform: &mut dyn FnMut(Action)) {
        match byte {
            b'0'..=b'9' => {
                let digit = usize::from(byte - b'0');
                let value = self.digits.unwrap_or(0).saturating_mul(10) + digit;
                self.digits = Some(value.min(MAX_PARAM));
            }
            // A sub-parameter (SGR's `38:2:…`) reads as a parameter of its own.
            b';' | b':' => {
                if self.params.len() < MAX_PARAMS {
                    self.params.push(self.digits.take().unwrap_or(0));
                }
                self.digits = None;
            }
            b'<'..=b'?' => {
                if self.marker.is_none() && self.params.is_empty() && self.digits.is_none() {
                    self.marker = Some(byte);
                } else {
                    self.state = State::CsiIgnore;
                }
            }
            0x20..=0x2F => self.intermediate = Some(byte),
            0x40..=0x7E => {
                if self.digits.is_some() || !self.params.is_empty() {
                    self.params.push(self.digits.take().unwrap_or(0));
                }
                self.state = State::Ground;
                perform(Action::Csi {
                    marker: self.marker.take(),
                    params: std::mem::take(&mut self.params),
                    intermediate: self.intermediate.take(),
                    last: byte,
                });
            }
            _ => self.state = State::Ground,
        }
    }

    fn string(&mut self, byte: u8) {
        self.dropped += 1;
        match byte {
            BEL => self.state = State::Ground,
            ESC => self.state = State::StrEscape,
            CAN | SUB => self.state = State::Ground,
            _ if self.dropped > MAX_STRING => self.state = State::Ground,
            _ => {}
        }
    }

    fn string_escape(&mut self, byte: u8, perform: &mut dyn FnMut(Action)) {
        if byte == b'\\' {
            self.state = State::Ground;
        } else {
            // Not the end of the string: the escape sequence of its own.
            self.begin_escape();
            self.advance(byte, perform);
        }
    }
}
