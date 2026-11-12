#[cfg(test)]
use std::time::Instant;

#[cfg(test)]
use super::paste::PasteWatch;
use super::paste::Segment;

/// The kinds of bytes a write of the human's path holds, by the names a snapshot says
/// them in, which the daemon's trace reads them by.
const CLASSES: [&str; 9] = cf_proto::panes::HUMAN_WRITE_CLASSES;

/// Which of [`CLASSES`] a write holds, a bit each.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Classes(u16);

impl Classes {
    fn with(&mut self, class: &str) {
        if let Some(at) = CLASSES.iter().position(|known| *known == class) {
            self.0 |= 1 << at;
        }
    }

    pub(super) fn names(self) -> Vec<&'static str> {
        CLASSES
            .iter()
            .enumerate()
            .filter(|(at, _)| self.0 & (1 << at) != 0)
            .map(|(_, name)| *name)
            .collect()
    }
}

/// What a write of the human's path holds: how many characters its keys leave unsent
/// in the window's input box, and whether it holds any key at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Keys {
    pub(super) unsent: usize,
    pub(super) keyed: bool,
    pub(super) classes: Classes,
}

/// How many characters the human's keys leave unsent in a window's input
/// box, as near as keys tell ([`keys_of`]).
#[cfg(test)]
pub(super) fn unsent_after(unsent: usize, bytes: &[u8]) -> usize {
    let fed = PasteWatch::default().feed(bytes, Instant::now());
    keys_of(unsent, bytes, &fed.segments).unsent
}

/// What `bytes`, a write of the human's path, hold, `unsent` characters left unsent
/// before it, as near as keys tell: each character typed or pasted counts one,
/// Backspace takes one off, and Enter (sent), Ctrl+C or Ctrl+U (the line
/// cleared) leave none. Ctrl+V counts one, for what it pastes from the
/// clipboard (an image's placeholder, in Claude Code). A bracketed paste
/// counts its characters, its line breaks included. Other escape sequences
/// (arrows, function keys, Alt+Enter's new line) and control keys change
/// nothing. What the terminal reports of itself is no key (T-839): a string
/// (OSC, DCS, APC, PM, SOS), to its BEL or ST; a mouse report (`CSI <`, and the
/// legacy `CSI M` with its three bytes); a focus report (`CSI I`, `CSI O`).
///
/// `segments` say which of `bytes` are keys and which a paste ([`PasteWatch::feed`]): a paste
/// is its characters whatever write it is in, line breaks and control bytes among them, and
/// only the bytes outside it are read as keys.
pub(super) fn keys_of(unsent: usize, bytes: &[u8], segments: &[Segment]) -> Keys {
    let mut keys = Keys {
        unsent,
        keyed: false,
        classes: Classes::default(),
    };
    for segment in segments {
        match segment {
            Segment::Typed(range) => typed_keys(&mut keys, &bytes[range.clone()]),
            Segment::Pasted(characters) => {
                keys.classes.with("paste");
                keys.keyed = true;
                keys.unsent += characters;
            }
        }
    }
    keys
}

/// What the keys `bytes` leave unsent, added to `keys` ([`keys_of`]): none of them is in a
/// paste.
fn typed_keys(keys: &mut Keys, bytes: &[u8]) {
    let mut unsent = keys.unsent;
    let mut keyed = keys.keyed;
    let mut classes = keys.classes;
    let mut at = 0;
    while at < bytes.len() {
        let rest = &bytes[at..];
        match rest[0] {
            0x1b if rest.get(1).is_some_and(|kind| b"]P_^X".contains(kind)) => {
                classes.with("string");
                at += 2 + string_end(&rest[2..]);
                continue;
            }
            0x1b if rest.starts_with(b"\x1b[M") => {
                classes.with("mouse");
                at += 6.min(rest.len());
                continue;
            }
            0x1b if rest.starts_with(b"\x1b[<")
                || rest.starts_with(b"\x1b[I")
                || rest.starts_with(b"\x1b[O") =>
            {
                classes.with(if rest[2] == b'<' { "mouse" } else { "focus" });
                at += csi_end(rest);
                continue;
            }
            _ => keyed = true,
        }
        match rest[0] {
            b'\r' => {
                classes.with("enter");
                unsent = 0;
            }
            // A line feed is Ctrl+J: a new line of the draft in every harness (the page writes
            // Shift+Enter as one, T-1336), where Enter, a carriage return, sends it.
            b'\n' => {
                classes.with("text");
                unsent += 1;
            }
            0x03 => {
                classes.with("control");
                unsent = 0;
            }
            0x15 => {
                classes.with("erase");
                unsent = 0;
            }
            0x7f | 0x08 => {
                classes.with("erase");
                unsent = unsent.saturating_sub(1);
            }
            0x16 => {
                classes.with("control");
                unsent += 1;
            }
            0x1b => {
                classes.with("escape");
                at += match rest.get(1) {
                    Some(b'[') => csi_end(rest),
                    Some(b'O') => 3.min(rest.len()),
                    Some(_) => 2,
                    None => 1,
                };
                continue;
            }
            byte if byte < 0x20 => classes.with("control"),
            byte if byte & 0xC0 != 0x80 => {
                classes.with("text");
                unsent += 1;
            }
            _ => {}
        }
        at += 1;
    }
    *keys = Keys {
        unsent,
        keyed,
        classes,
    };
}

/// How many bytes of `rest`, which begins with a CSI (`ESC [`), are that sequence:
/// to its final byte, or all of them where the write ends first.
fn csi_end(rest: &[u8]) -> usize {
    rest[2..]
        .iter()
        .position(|byte| (0x40..=0x7e).contains(byte))
        .map_or(rest.len(), |end| end + 3)
}

/// How many bytes of `body`, a terminal string's after its opening, are the string
/// with its ending: to its BEL or ST (`ESC \\`), or all of them where the write ends
/// first.
fn string_end(body: &[u8]) -> usize {
    body.iter()
        .enumerate()
        .find_map(|(at, byte)| match byte {
            0x07 => Some(at + 1),
            0x1b if body.get(at + 1) == Some(&b'\\') => Some(at + 2),
            _ => None,
        })
        .unwrap_or(body.len())
}
