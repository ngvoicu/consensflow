//! A streaming UTF-8 decoder followed by the shipped xterm's VT500 parser.
//! A redraw may be cut only between codepoints and in its ground state.

const MAX_PARAMS: usize = 32;
const MAX_PARAM: usize = 0xFFFF;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Action {
    Print(char),
    Control(u8),
    Escape {
        intermediate: Option<u8>,
        last: u8,
    },
    Csi {
        marker: Option<u8>,
        params: Vec<usize>,
        intermediate: Option<u8>,
        last: u8,
    },
    /// A string dispatch clears xterm's preceding printable character for REP.
    ClearPreceding,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    Escape,
    EscapeIntermediate,
    CsiEntry,
    CsiParam,
    CsiIntermediate,
    CsiIgnore,
    Osc,
    SosPmApc,
    DcsEntry,
    DcsParam,
    DcsIntermediate,
    DcsIgnore,
    DcsPassthrough,
}

#[derive(Debug, Default)]
pub(super) struct Parser {
    state: State,
    partial: Vec<u8>,
    needed: usize,
    marker: Option<u8>,
    params: Vec<usize>,
    digits: Option<usize>,
    intermediate: Option<u8>,
    extra_intermediate: bool,
    /// In the sub-parameters of a parameter (after a colon): xterm keeps them apart from the
    /// parameters a mode or a margin is set by, so they are read past.
    sub: bool,
}

impl Parser {
    pub(super) fn at_rest(&self) -> bool {
        self.state == State::Ground && self.partial.is_empty()
    }

    /// Decode before interpreting controls: xterm accepts UTF-8 encoded C1,
    /// and a codepoint can be split in *any* parser state.
    pub(super) fn advance(&mut self, byte: u8, perform: &mut dyn FnMut(Action)) {
        if !self.partial.is_empty() {
            if (0x80..=0xbf).contains(&byte) {
                self.partial.push(byte);
                if self.partial.len() == self.needed {
                    let character = std::str::from_utf8(&self.partial)
                        .ok()
                        .and_then(|s| s.chars().next());
                    self.partial.clear();
                    if let Some(character) = character {
                        if character != '\u{feff}' {
                            self.codepoint(character as u32, perform);
                        }
                    }
                }
                return;
            }
            // xterm discards an interrupted sequence, then decodes this byte.
            self.partial.clear();
        }
        match byte {
            0x00..=0x7f => self.codepoint(u32::from(byte), perform),
            0xc0..=0xdf => self.start_character(byte, 2),
            0xe0..=0xef => self.start_character(byte, 3),
            0xf0..=0xf7 => self.start_character(byte, 4),
            _ => {} // xterm drops invalid starters and stray continuation bytes.
        }
    }

    fn start_character(&mut self, byte: u8, length: usize) {
        self.partial.push(byte);
        self.needed = length;
    }

    fn begin_escape(&mut self) {
        self.state = State::Escape;
        self.intermediate = None;
        self.extra_intermediate = false;
    }

    fn begin_csi(&mut self) {
        self.state = State::CsiEntry;
        self.marker = None;
        self.params.clear();
        self.digits = None;
        self.sub = false;
        self.intermediate = None;
        self.extra_intermediate = false;
    }

    fn intermediate(&mut self, byte: u8) {
        if self.intermediate.is_some() {
            self.extra_intermediate = true;
        }
        self.intermediate = Some(byte);
    }

    fn codepoint(&mut self, code: u32, perform: &mut dyn FnMut(Action)) {
        // xterm's anywhere transitions, after UTF-8 decoding.
        match code {
            0x18 | 0x1a => {
                self.state = State::Ground;
                perform(Action::ClearPreceding);
                return;
            }
            0x1b => {
                if matches!(self.state, State::Osc | State::DcsPassthrough) {
                    perform(Action::ClearPreceding);
                }
                self.begin_escape();
                return;
            }
            0x9b => {
                self.begin_csi();
                return;
            }
            0x9d => {
                self.state = State::Osc;
                return;
            }
            0x90 => {
                self.state = State::DcsEntry;
                return;
            }
            0x98 | 0x9e | 0x9f => {
                self.state = State::SosPmApc;
                return;
            }
            0x9c => {
                if matches!(self.state, State::Osc | State::DcsPassthrough) {
                    perform(Action::ClearPreceding);
                }
                self.state = State::Ground;
                return;
            }
            0x80..=0x8f | 0x91..=0x97 | 0x99..=0x9a => {
                self.state = State::Ground;
                match code {
                    0x84 => perform(Action::Escape {
                        intermediate: None,
                        last: b'D',
                    }),
                    0x85 => perform(Action::Escape {
                        intermediate: None,
                        last: b'E',
                    }),
                    0x88 => perform(Action::Escape {
                        intermediate: None,
                        last: b'H',
                    }),
                    _ => perform(Action::ClearPreceding),
                }
                return;
            }
            _ => {}
        }
        let byte = if code < 0xa0 { code as u8 } else { 0xa0 };
        // Executable C0 controls stay in ESC/CSI; in string states they are
        // ignored, except BEL terminates OSC. DEL stays in the current state.
        if byte < 0x20 {
            match self.state {
                State::Osc if byte == 7 => {
                    self.state = State::Ground;
                    perform(Action::ClearPreceding);
                }
                State::Osc
                | State::SosPmApc
                | State::DcsEntry
                | State::DcsParam
                | State::DcsIntermediate
                | State::DcsIgnore
                | State::DcsPassthrough => {}
                _ => perform(Action::Control(byte)),
            }
            return;
        }
        if byte == 0x7f {
            return;
        }
        match self.state {
            State::Ground => {
                if let Some(character) = char::from_u32(code) {
                    perform(Action::Print(character));
                }
            }
            State::Escape => match byte {
                b'[' => self.begin_csi(),
                b']' => self.state = State::Osc,
                b'P' => self.state = State::DcsEntry,
                b'X' | b'^' | b'_' => self.state = State::SosPmApc,
                0x20..=0x2f => {
                    self.intermediate(byte);
                    self.state = State::EscapeIntermediate;
                }
                0x30..=0x7e => {
                    self.state = State::Ground;
                    perform(Action::Escape {
                        intermediate: None,
                        last: byte,
                    });
                }
                _ => self.state = State::Ground,
            },
            State::EscapeIntermediate => match byte {
                0x20..=0x2f => self.intermediate(byte),
                0x30..=0x7e => {
                    self.state = State::Ground;
                    if !self.extra_intermediate {
                        perform(Action::Escape {
                            intermediate: self.intermediate.take(),
                            last: byte,
                        });
                    } else {
                        perform(Action::ClearPreceding);
                    }
                }
                _ => self.state = State::Ground,
            },
            State::CsiEntry | State::CsiParam | State::CsiIntermediate | State::CsiIgnore => {
                self.csi(byte, perform)
            }
            State::Osc | State::DcsIgnore | State::DcsPassthrough => {}
            State::SosPmApc => {
                if byte >= 0xa0 {
                    self.state = State::Ground;
                }
            }
            State::DcsEntry | State::DcsParam | State::DcsIntermediate => self.dcs(byte),
        }
    }

    fn csi(&mut self, byte: u8, perform: &mut dyn FnMut(Action)) {
        if self.state == State::CsiIgnore {
            if (0x40..=0x7e).contains(&byte) {
                self.state = State::Ground;
            }
            return;
        }
        match byte {
            0x30..=0x3b if self.state != State::CsiIntermediate => {
                self.state = State::CsiParam;
                match byte {
                    b'0'..=b'9' if self.sub => {}
                    b':' => self.sub = true,
                    b'0'..=b'9' => {
                        let n =
                            self.digits.unwrap_or(0).saturating_mul(10) + usize::from(byte - b'0');
                        self.digits = Some(n.min(MAX_PARAM));
                    }
                    _ => {
                        if self.params.len() < MAX_PARAMS {
                            self.params.push(self.digits.take().unwrap_or(0));
                        }
                        self.digits = None;
                        self.sub = false;
                    }
                }
            }
            0x3c..=0x3f if self.state == State::CsiEntry => {
                self.marker = Some(byte);
                self.state = State::CsiParam;
            }
            0x30..=0x3f => self.state = State::CsiIgnore,
            0x20..=0x2f => {
                self.intermediate(byte);
                self.state = State::CsiIntermediate;
            }
            0x40..=0x7e => {
                if self.digits.is_some() || !self.params.is_empty() {
                    self.params.push(self.digits.take().unwrap_or(0));
                }
                self.state = State::Ground;
                if self.extra_intermediate {
                    perform(Action::ClearPreceding);
                } else {
                    perform(Action::Csi {
                        marker: self.marker.take(),
                        params: std::mem::take(&mut self.params),
                        intermediate: self.intermediate.take(),
                        last: byte,
                    });
                }
            }
            _ => self.state = State::Ground,
        }
    }

    fn dcs(&mut self, byte: u8) {
        match byte {
            0x20..=0x2f => self.state = State::DcsIntermediate,
            0x30..=0x3b if self.state != State::DcsIntermediate => self.state = State::DcsParam,
            0x3c..=0x3f if self.state == State::DcsEntry => self.state = State::DcsParam,
            0x30..=0x3f => self.state = State::DcsIgnore,
            0x40..=0x7e => self.state = State::DcsPassthrough,
            _ => self.state = State::Ground,
        }
    }
}
