//! Node's `path.join` and `path.normalize`, ported line by line from
//! `lib/path.js` of Node v26.8.1, the Node ConsensFlow runs today (MIT,
//! Copyright Joyent, Inc. and other Node contributors): the `posix` and the
//! `win32` flavour, each with its `join` and `normalize`, and the helpers they
//! call. Nothing else of `path.js` is here: no `resolve`, `relative`,
//! `dirname` or `parse`.
//!
//! Where the Node code joined paths with `path.join`, the Rust port must name
//! the same file for every input, the odd ones included, and `std::path` does
//! not. `PathBuf::join` leaves `C:` and `agents.json` as `C:agents.json`
//! where Node's `win32.join` gives `C:\agents.json`; a lexical normalization
//! over `Component`s drops the `..` of `C:..\cf` that Node keeps; and Node has
//! rules of its own for Windows' reserved device names and for CVE-2024-36139.
//! So this follows `path.js`. Both flavours are pure functions of text: they
//! compile and are tested on every system, and [`join`] is the one of the
//! system this is built for, as `path.join` is.
//!
//! JavaScript walks UTF-16 code units and Rust text is UTF-8. Every character
//! these functions test is ASCII (a separator, a dot, a colon, a letter), so
//! the port walks bytes, and cuts the text only at an ASCII character or at
//! an end: a cut is always between two characters. An index or a length is a
//! count of bytes throughout, as the text is cut by them. A text is empty in
//! bytes exactly when it is in UTF-16 units, so the tests for empty need no
//! word. Each other comparison of a length with a number says why bytes
//! answer as units do, or counts the units.

use crate::text::utf16_len;

pub mod posix;
pub mod win32;

#[cfg(test)]
mod tests;

/// `path.join` of the system this is built for: Windows' on Windows, POSIX's
/// elsewhere.
pub fn join(parts: &[&str]) -> String {
    if cfg!(windows) {
        win32::join(parts)
    } else {
        posix::join(parts)
    }
}

/// What `path.charCodeAt(index)` reads, as the byte there, and none where
/// JavaScript reads NaN, which no test below takes for any character. Every
/// test is for an ASCII character, and a byte of any other is 0x80 or more:
/// as far from every one of them as the UTF-16 unit it stands for.
fn code_at(text: &str, index: usize) -> Option<u8> {
    text.as_bytes().get(index).copied()
}

fn is_path_separator(code: Option<u8>) -> bool {
    matches!(code, Some(b'/' | b'\\'))
}

fn is_posix_path_separator(code: Option<u8>) -> bool {
    code == Some(b'/')
}

/// `text.slice(start, end)` for the indexes this module computes: the text
/// between them, and none of it when `end` is not after `start`, as in
/// JavaScript. Each index is that of an ASCII character or an end of the text,
/// so it is between two characters and `get` never refuses one.
fn slice(text: &str, start: usize, end: usize) -> &str {
    text.get(start..end).unwrap_or_default()
}

/// Resolves `.` and `..` elements in a path with directory names.
fn normalize_string(
    path: &str,
    allow_above_root: bool,
    separator: char,
    is_path_separator: fn(Option<u8>) -> bool,
) -> String {
    let mut res = String::new();
    // The length of the last segment of `res`, in bytes as `res.len()` is.
    let mut last_segment_length = 0;
    // Where the segment after the last separator starts: `lastSlash + 1` of
    // `path.js`, which is -1 + 1 before the first separator.
    let mut segment_start = 0;
    // The dots of the segment so far, or -1 once it holds anything else. One
    // is counted per byte at most, so it cannot overflow.
    let mut dots: isize = 0;
    let mut code = None;
    for i in 0..=path.len() {
        if i < path.len() {
            code = code_at(path, i);
        } else if is_path_separator(code) {
            break;
        } else {
            code = Some(b'/');
        }

        if is_path_separator(code) {
            // `lastSlash === i - 1`: no character between the last separator and
            // this one, which is no character in either count.
            if segment_start == i || dots == 1 {
                // NOOP
            } else if dots == 2 {
                // Unless the last segment of `res` is exactly `..`. `res.length <
                // 2` only guarded the reads of the last two units, which
                // `ends_with` needs no guard for. A dot is one byte and one UTF-16
                // unit, so a segment two long that ends in two dots is `..` by
                // either count, and `..` is both by either count. Alone,
                // `lastSegmentLength !== 2` differs by count for a lone `é` (one
                // unit, two bytes), which does not end in dots: the test as a
                // whole is the same.
                if last_segment_length != 2 || !res.ends_with("..") {
                    // `res.length > 2`, counted in UTF-16 units: `日本` is two units
                    // and six bytes. `lastSlashIndex` is in bytes, as `res` is cut.
                    if utf16_len(&res) > 2 {
                        // None is JavaScript's -1: `res` is one segment.
                        match res.len().checked_sub(last_segment_length + 1) {
                            None => {
                                res.clear();
                                last_segment_length = 0;
                            }
                            Some(last_slash_index) => {
                                res = slice(&res, 0, last_slash_index).to_owned();
                                last_segment_length =
                                    res.len() - res.rfind(separator).map_or(0, |at| at + 1);
                            }
                        }
                        segment_start = i + 1;
                        dots = 0;
                        continue;
                    } else if !res.is_empty() {
                        res.clear();
                        last_segment_length = 0;
                        segment_start = i + 1;
                        dots = 0;
                        continue;
                    }
                }
                if allow_above_root {
                    if !res.is_empty() {
                        res.push(separator);
                    }
                    res.push_str("..");
                    last_segment_length = 2;
                }
            } else {
                if !res.is_empty() {
                    res.push(separator);
                }
                res.push_str(slice(path, segment_start, i));
                last_segment_length = i - segment_start;
            }
            segment_start = i + 1;
            dots = 0;
        } else if code == Some(b'.') && dots != -1 {
            dots += 1;
        } else {
            dots = -1;
        }
    }
    res
}
