//! The end of what a program wrote to its standard error: kept to say why it
//! failed, and read for the line that says where it listens.

use cf_base::text::utf16_len;

/// How much is kept, in UTF-16 code units: what `.slice(-4000)` kept.
const KEPT: usize = 4000;

/// The last [`KEPT`] units of text a program wrote, decoded as it came: a
/// character split between two reads is whole when the second arrives, and
/// bytes that are no UTF-8 read as U+FFFD, as Node read them.
#[derive(Debug, Default)]
pub(crate) struct Tail {
    text: String,
    /// The start of a character whose end has not come yet.
    carry: Vec<u8>,
}

impl Tail {
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Takes in what the program wrote next.
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        let mut input = std::mem::take(&mut self.carry);
        input.extend_from_slice(bytes);
        let mut rest = input.as_slice();
        loop {
            match std::str::from_utf8(rest) {
                Ok(valid) => {
                    self.text.push_str(valid);
                    break;
                }
                Err(error) => {
                    let (valid, after) = rest.split_at(error.valid_up_to());
                    self.text.push_str(&String::from_utf8_lossy(valid));
                    match error.error_len() {
                        Some(length) => {
                            self.text.push('\u{FFFD}');
                            rest = &after[length..];
                        }
                        // A character cut by the end of this read.
                        None => {
                            self.carry = after.to_vec();
                            break;
                        }
                    }
                }
            }
        }
        self.keep_the_end();
    }

    fn keep_the_end(&mut self) {
        if utf16_len(&self.text) <= KEPT {
            return;
        }
        let mut units = 0;
        let mut start = self.text.len();
        for (at, character) in self.text.char_indices().rev() {
            if units + character.len_utf16() > KEPT {
                break;
            }
            units += character.len_utf16();
            start = at;
        }
        self.text.drain(..start);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_what_was_written_as_it_was() {
        let mut tail = Tail::default();
        tail.push(b"not logged in\n");
        tail.push(b"try again\n");
        assert_eq!(tail.text(), "not logged in\ntry again\n");
    }

    #[test]
    fn keeps_only_the_last_4000_units() {
        let mut tail = Tail::default();
        tail.push("a".repeat(3000).as_bytes());
        tail.push("b".repeat(3000).as_bytes());
        assert_eq!(utf16_len(tail.text()), 4000);
        assert!(tail.text().starts_with('a') && tail.text().ends_with('b'));
        assert_eq!(tail.text().matches('b').count(), 3000);
    }

    #[test]
    fn counts_an_emoji_as_two_units_and_never_cuts_it() {
        let mut tail = Tail::default();
        tail.push("😀".repeat(2001).as_bytes());
        // 4002 units written; 4000 fit, so the first one goes whole.
        assert_eq!(tail.text(), "😀".repeat(2000));
    }

    #[test]
    fn a_character_split_between_two_reads_is_whole_when_the_second_comes() {
        let bytes = "listening é on".as_bytes();
        let cut = bytes.iter().position(|byte| *byte == 0xC3).unwrap() + 1;
        let mut tail = Tail::default();
        tail.push(&bytes[..cut]);
        assert_eq!(tail.text(), "listening ");
        tail.push(&bytes[cut..]);
        assert_eq!(tail.text(), "listening é on");
    }

    #[test]
    fn bytes_that_are_no_utf8_read_as_the_replacement_character() {
        let mut tail = Tail::default();
        tail.push(b"a\xFFb\xC3");
        tail.push(b"(c");
        assert_eq!(tail.text(), "a\u{FFFD}b\u{FFFD}(c");
    }
}
