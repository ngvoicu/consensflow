//! What the packaged smoke's stand-in harness runs to read a large paste off
//! its terminal: it says it is ready, takes every byte the terminal gives it up
//! to the end of a bracketed paste (`ESC [ 2 0 1 ~`), and says how many bytes
//! it had and the SHA-256 of them, in hex. The smoke compares both with what
//! the page sent. The harness has put the terminal in raw mode before it runs
//! this, so what comes in is what was written, and what goes out ends its lines
//! with the carriage return a raw terminal needs.
//!
//! A program of the suites' own, not a script: the bundle ships no Node, and the
//! smoke asks nothing of the machine it runs on but what the bundle brings.

use std::io::{self, Read, Write};
use std::process::ExitCode;

use sha2::{Digest, Sha256};

/// What ends a bracketed paste.
const PASTE_END: &[u8] = b"\x1b[201~";

fn main() -> ExitCode {
    let mut out = io::stdout().lock();
    if say(&mut out, "CFSMOKE-PASTE-READY\r\n").is_err() {
        return ExitCode::FAILURE;
    }
    match take_paste(&mut io::stdin().lock()) {
        Ok(Some(paste)) => {
            if say(&mut out, &report(&paste)).is_err() {
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        // The terminal closed before the paste ended: there is nothing to report.
        Ok(None) | Err(_) => ExitCode::FAILURE,
    }
}

/// Writes `text` and sends it on its way.
fn say(out: &mut impl Write, text: &str) -> io::Result<()> {
    out.write_all(text.as_bytes())?;
    out.flush()
}

/// Everything `input` gives, up to and including the end of a paste, as it is
/// when the last bytes read are the end of one; none if `input` ends first. The
/// end is looked for in all that was read, not in each piece: a terminal gives a
/// paste in pieces of any size.
fn take_paste(input: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut paste = Vec::new();
    let mut piece = [0_u8; 64 * 1024];
    loop {
        let read = input.read(&mut piece)?;
        if read == 0 {
            return Ok(None);
        }
        paste.extend_from_slice(&piece[..read]);
        if paste.ends_with(PASTE_END) {
            return Ok(Some(paste));
        }
    }
}

/// The line that says what was read: `CFSMOKE-PASTE <bytes> <sha256 in hex>`.
fn report(paste: &[u8]) -> String {
    format!(
        "CFSMOKE-PASTE {} {:x}\r\n",
        paste.len(),
        Sha256::digest(paste)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A terminal that gives its bytes a few at a time, as a pty does.
    struct Pieces<'a> {
        left: &'a [u8],
        size: usize,
    }

    impl Read for Pieces<'_> {
        fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
            let count = self.size.min(self.left.len()).min(into.len());
            into[..count].copy_from_slice(&self.left[..count]);
            self.left = &self.left[count..];
            Ok(count)
        }
    }

    #[test]
    fn a_paste_is_whatever_comes_up_to_its_end_however_the_terminal_cuts_it() {
        let paste = b"\x1b[200~\xe6\xbc\xa2\xe5\xad\x97 r\xc3\xa9sum\xc3\xa9 \r\x1b[201~";
        for size in [1, 2, 5, 6, 7, 1000] {
            let mut terminal = Pieces { left: paste, size };
            assert_eq!(
                take_paste(&mut terminal).unwrap().as_deref(),
                Some(paste.as_slice()),
                "pieces of {size}"
            );
        }
    }

    #[test]
    fn the_end_of_a_paste_is_found_across_the_pieces_it_was_cut_into() {
        // Cut in the middle of the end: only the bytes read so far, all of them, hold it.
        let text = b"\x1b[200~text\x1b[201~";
        let mut terminal = Pieces {
            left: text,
            size: 3,
        };
        assert_eq!(
            take_paste(&mut terminal).unwrap().as_deref(),
            Some(text.as_slice())
        );
    }

    #[test]
    fn a_paste_whose_terminal_closes_before_its_end_is_none() {
        // The end begun and not finished.
        let mut cut_short = Pieces {
            left: b"\x1b[200~a\x1b[20",
            size: 4,
        };
        assert_eq!(take_paste(&mut cut_short).unwrap(), None);
        assert_eq!(take_paste(&mut io::empty()).unwrap(), None);
    }

    #[test]
    fn the_report_is_the_size_and_the_sha256_in_hex_with_the_line_end_of_a_raw_terminal() {
        // The SHA-256 of "abc", from the standard's own examples.
        assert_eq!(
            report(b"abc"),
            "CFSMOKE-PASTE 3 ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\r\n"
        );
        assert_eq!(
            report(b""),
            "CFSMOKE-PASTE 0 e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\r\n"
        );
    }
}
