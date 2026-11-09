//! Where a write has an Escape-led key in it (T-1336).
//!
//! A program takes a lone Escape for "stop". The console host a Windows window's input goes through
//! reads it 4,096 bytes at a time and takes an Escape that ends a read for the Escape key, whatever
//! follows (`pty/keys.rs`, `conpty`), so the bytes of a key that begins with an Escape have to be
//! written together into a pipe that holds nothing unread. This says where each such key begins.

/// The parts of `bytes` that begin at an Escape that does not follow another one, and what comes
/// before the first: each part is a key that begins with Escape (an arrow `ESC [ A`, Alt+x
/// `ESC x`, Alt+Enter `ESC CR`, a bracketed paste's markers, a reply of the terminal) with the text
/// that follows it up to the next, and a run of Escapes is one key (`ESC ESC [ A` is Alt+Up). Every byte is in exactly one part,
/// in order; no part is empty.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn parts(bytes: &[u8]) -> Vec<&[u8]> {
    let mut parts = Vec::new();
    let mut start = 0;
    for at in 1..bytes.len() {
        if bytes[at] == 0x1b && bytes[at - 1] != 0x1b {
            parts.push(&bytes[start..at]);
            start = at;
        }
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
    fn every_byte_is_in_one_part_in_order() {
        let bytes = b"x\x1b[A\x1b\x1b\x1bOPy\x1bz\x1b";
        let joined: Vec<u8> = parts(bytes).concat();
        assert_eq!(joined, bytes);
        assert!(parts(bytes).iter().all(|part| !part.is_empty()));
    }
}
