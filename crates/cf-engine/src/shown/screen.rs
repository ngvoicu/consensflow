//! Whether a window's last screen holds what may be a key with no word to say so.
//!
//! A screen that names a secret is withheld whole (`shown`), and a private key's block names
//! one in its delimiters. What is left is a key (or a token, a certificate's body, a hash)
//! cropped so that no delimiter and no name is in view: rows of base64 and nothing else, as
//! narrow as the window was (`MIIEvQIBADANBgkq` / `hkiG9w0BAQEFAASC`), with a short ending
//! (`YQ==`). No masker holds those: each row is below any token's size. So the rows are looked
//! at together (the review of `860010398`, P1), and the whole screen is withheld where they are
//! a run of rows that are base64 characters and nothing else (letters, digits, `+`, `/`, no
//! blank in them; `=` for padding):
//!
//! - a row of forty characters or more by itself, in any case (a hash, a token, a key's body);
//! - two rows in a row that both look encoded: a digit among letters, a `+`, a `/`, an `=`, or
//!   a capital after a small letter;
//! - two rows in a row or more that are twelve characters or more each and forty together.
//!
//! A false alarm (a hash, a long identifier) costs the quote of one screen; a miss costs a key.
//! The rows are the screen's own, chrome (a box's edge, a rule of `=`) left out, as a quote reads
//! them; a row of one `=` or two is a key's last row, and is not.

/// A row this long, of base64's characters alone, is a token or a key's body by itself.
const LONG: usize = 40;

/// Rows this long, two or more in a row, are a key's body together once they reach [`LONG`].
const BODY_ROW: usize = 12;

/// Whether `tail` holds rows that may be a key's or a token's.
pub(super) fn holds_a_key(tail: &[String]) -> bool {
    let rows: Vec<&str> = tail
        .iter()
        .map(|row| row.trim())
        .filter(|row| row.chars().any(char::is_alphanumeric) || is_padding(row))
        .collect();
    let mut run: Vec<&str> = Vec::new();
    for row in &rows {
        if is_base64(row) {
            run.push(row);
        } else {
            if suspect(&run) {
                return true;
            }
            run.clear();
        }
    }
    suspect(&run)
}

/// Whether a run of rows of base64's characters (adjacent rows of the screen) may be a key.
fn suspect(run: &[&str]) -> bool {
    if run.iter().any(|row| row.len() >= LONG) {
        return true;
    }
    if run
        .windows(2)
        .any(|pair| pair.iter().all(|row| looks_encoded(row)))
    {
        return true;
    }
    run.len() >= 2
        && run.iter().all(|row| row.len() >= BODY_ROW)
        && run.iter().map(|row| row.len()).sum::<usize>() >= LONG
}

/// A row of the padding of base64 alone: one `=` or two. A longer row of them is a rule that
/// draws a line across the screen, which is chrome.
fn is_padding(row: &str) -> bool {
    row == "=" || row == "=="
}

/// A row of base64's characters and nothing else.
fn is_base64(row: &str) -> bool {
    !row.is_empty()
        && row
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
}

/// Whether a row of base64's characters says by them that it is not a word: it has a `+`, a
/// `/` or an `=`, a digit among letters, or a capital after a small letter.
fn looks_encoded(row: &str) -> bool {
    if row.contains(['+', '/', '=']) {
        return true;
    }
    let letters = row.chars().any(|c| c.is_ascii_alphabetic());
    if letters && row.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    let chars: Vec<char> = row.chars().collect();
    chars
        .windows(2)
        .any(|pair| pair[0].is_ascii_lowercase() && pair[1].is_ascii_uppercase())
}

#[cfg(test)]
mod tests;
