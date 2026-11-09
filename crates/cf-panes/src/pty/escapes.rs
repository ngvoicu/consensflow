//! Where a write has an Escape-led key in it (T-1336).
//!
//! A program takes a lone Escape for "stop". The console host a Windows window's input goes through
//! reads it 4,096 bytes at a time and takes an Escape that ends a read for the Escape key, whatever
//! follows (`pty/keys.rs`, `conpty`), so the bytes of a key that begins with an Escape have to be
//! written together into a pipe that holds nothing unread. This says where each such key begins.

/// The parts of `bytes` that begin at an Escape that does not follow another one, and what comes
/// before the first: each part is a key that begins with Escape (an arrow `ESC [ A`, Alt+x
/// `ESC x`, Alt+Enter `ESC CR`, a bracketed paste's markers, a reply of the terminal) with the text
/// that follows it up to the next, and a run of Escapes is one key (`ESC ESC [ A` is Alt+Up). **A
/// terminal string is one key, its terminator with it:** what a terminal answers a program's
/// questions with (OSC `ESC ] … BEL` or `ESC ] … ESC \`, DCS `ESC P … ESC \`, APC, PM, SOS) holds an
/// `ESC \`, which ends the string and begins no key; an Escape inside a string that is not the
/// start of that terminator ends the string and begins a key. Every byte is in exactly one part,
/// in order; no part is empty.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn parts(bytes: &[u8]) -> Vec<&[u8]> {
    let mut parts = Vec::new();
    let mut start = 0;
    // The kind (`]`, `P`, `_`, `^`, `X`) of the terminal string the bytes seen so far are in.
    let mut string: Option<u8> = None;
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if string.is_some() {
            if byte == 0x07 && string == Some(b']') {
                string = None;
                at += 1;
                continue;
            }
            if byte == 0x1b && bytes.get(at + 1) == Some(&b'\\') {
                string = None;
                at += 2;
                continue;
            }
            if byte != 0x1b {
                at += 1;
                continue;
            }
            // An Escape that is not ST's: the string was cut short, and a key begins.
            string = None;
        }
        if byte == 0x1b {
            if at > 0 && bytes[at - 1] != 0x1b {
                parts.push(&bytes[start..at]);
                start = at;
            }
            if let Some(kind) = bytes.get(at + 1).filter(|kind| b"]P_^X".contains(kind)) {
                string = Some(*kind);
                at += 1;
            }
        }
        at += 1;
    }
    if start < bytes.len() {
        parts.push(&bytes[start..]);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::parts;

    #[test]
    fn a_write_is_cut_where_an_escape_led_key_begins() {
        assert_eq!(parts(b""), Vec::<&[u8]>::new());
        assert_eq!(parts(b"abc"), [&b"abc"[..]]);
        assert_eq!(parts(b"\x1b\r"), [&b"\x1b\r"[..]]);
        assert_eq!(parts(b"abc\x1b[A"), [&b"abc"[..], b"\x1b[A"]);
        assert_eq!(
            parts(b"\x1b[200~text\x1b[201~"),
            [&b"\x1b[200~text"[..], b"\x1b[201~"],
            "a bracketed paste's two markers are two keys"
        );
        assert_eq!(
            parts(b"\x1b[A\x1b[B\x1bx\n"),
            [&b"\x1b[A"[..], b"\x1b[B", b"\x1bx\n"]
        );
        assert_eq!(
            parts(b"\x1b\x1b[A"),
            [&b"\x1b\x1b[A"[..]],
            "a run of Escapes is one key: Alt+Up"
        );
        assert_eq!(parts(b"\x1b"), [&b"\x1b"[..]], "an Escape alone");
    }

    #[test]
    fn a_terminal_string_is_one_key_with_its_terminator() {
        // What a terminal answers a program's questions with: `ESC \` ends a string, it is not
        // a key that begins one (CI, 2026-11-09).
        let osc = b"\x1b]11;rgb:1e1e/1e1e/1e1e\x1b\\";
        let dcs = b"\x1bP>|xterm.js(6.0.0)\x1b\\";
        assert_eq!(parts(osc), [&osc[..]], "an OSC colour ended by ST");
        assert_eq!(parts(dcs), [&dcs[..]], "a DCS name ended by ST");
        assert_eq!(
            parts(b"\x1b]10;rgb:cccc/cccc/cccc\x07\x1b[A"),
            [&b"\x1b]10;rgb:cccc/cccc/cccc\x07"[..], b"\x1b[A"],
            "one ended by BEL, and a key after it"
        );
        assert_eq!(
            parts(b"x\x1bP>|a\x1b\\abc\x1b[B"),
            [&b"x"[..], b"\x1bP>|a\x1b\\abc", b"\x1b[B"],
            "the words after the terminator stay with it, as with any key"
        );
        assert_eq!(
            parts(b"\x1b]x\x07\x1b\\"),
            [&b"\x1b]x\x07"[..], b"\x1b\\"],
            "a BEL ends a string, and an `ESC \\` after it is a key (Alt+\\)"
        );
        assert_eq!(
            parts(b"\x1b]0;title\x1b[A"),
            [&b"\x1b]0;title"[..], b"\x1b[A"],
            "an Escape that does not begin ST ends the string: a key of its own"
        );
        assert_eq!(
            parts(b"\x1b]0;title\x1b"),
            [&b"\x1b]0;title"[..], b"\x1b"],
            "and so does an Escape at the end"
        );
    }

    #[test]
    fn every_byte_is_in_one_part_in_order() {
        let bytes = b"x\x1b[A\x1b\x1b\x1bOPy\x1bz\x1b]a\x1b\\b\x1bP\x07\x1b";
        let joined: Vec<u8> = parts(bytes).concat();
        assert_eq!(joined, bytes);
        assert!(parts(bytes).iter().all(|part| !part.is_empty()));
    }
}
