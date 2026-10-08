//! Whether text is whole JSON, the start of some, or neither: what tells the
//! last line of a JSONL file a live writer has not finished from one that never
//! will be JSON. It reads bytes: every token JSON has is ASCII, so a character
//! of more than one byte is a string's or no JSON's, as it was a string's or no
//! JSON's to JavaScript.

/// What some text is to JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Prefix {
    Complete,
    /// The start of some JSON: more may make it whole.
    Incomplete,
    Invalid,
}

/// Why a reading of the text stopped.
enum Halt {
    Incomplete,
    Invalid,
}

/// How deep a value may nest before the text is taken for no JSON: past it
/// JavaScript's own stack had given out, and so would this one's.
const DEEPEST: usize = 512;

/// What `source` is to JSON.
pub(crate) fn json_prefix_state(source: &str) -> Prefix {
    let mut reader = Reader {
        bytes: source.as_bytes(),
        at: 0,
    };
    let outcome = reader.value(0).map(|()| reader.skip_space());
    match outcome {
        Ok(()) if reader.at == reader.bytes.len() => Prefix::Complete,
        Ok(()) | Err(Halt::Invalid) => Prefix::Invalid,
        Err(Halt::Incomplete) => Prefix::Incomplete,
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

const fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(is_space) {
            self.at += 1;
        }
    }

    /// The next byte, which must be there: the text so far is a prefix.
    fn need(&self) -> Result<u8, Halt> {
        self.peek().ok_or(Halt::Incomplete)
    }

    fn digit(&self) -> bool {
        self.peek().is_some_and(|byte| byte.is_ascii_digit())
    }

    fn value(&mut self, depth: usize) -> Result<(), Halt> {
        if depth > DEEPEST {
            return Err(Halt::Invalid);
        }
        self.skip_space();
        match self.need()? {
            b'"' => self.string(),
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b't' => self.literal(b"true"),
            b'f' => self.literal(b"false"),
            b'n' => self.literal(b"null"),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(Halt::Invalid),
        }
    }

    fn string(&mut self) -> Result<(), Halt> {
        if self.need()? != b'"' {
            return Err(Halt::Invalid);
        }
        self.at += 1;
        while let Some(byte) = self.peek() {
            self.at += 1;
            match byte {
                b'"' => return Ok(()),
                0..=0x1f => return Err(Halt::Invalid),
                b'\\' => {
                    let escape = self.need()?;
                    self.at += 1;
                    if b"\"\\/bfnrt".contains(&escape) {
                        continue;
                    }
                    if escape != b'u' {
                        return Err(Halt::Invalid);
                    }
                    for _ in 0..4 {
                        if !self.need()?.is_ascii_hexdigit() {
                            return Err(Halt::Invalid);
                        }
                        self.at += 1;
                    }
                }
                _ => {}
            }
        }
        Err(Halt::Incomplete)
    }

    fn number(&mut self) -> Result<(), Halt> {
        if self.peek() == Some(b'-') {
            self.at += 1;
            self.need()?;
        }
        if self.peek() == Some(b'0') {
            self.at += 1;
            if self.digit() {
                return Err(Halt::Invalid);
            }
        } else if self.peek().is_some_and(|byte| matches!(byte, b'1'..=b'9')) {
            while self.digit() {
                self.at += 1;
            }
        } else {
            return Err(Halt::Invalid);
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            self.need()?;
            if !self.digit() {
                return Err(Halt::Invalid);
            }
            while self.digit() {
                self.at += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.need()?, b'+' | b'-') {
                self.at += 1;
                self.need()?;
            }
            if !self.digit() {
                return Err(Halt::Invalid);
            }
            while self.digit() {
                self.at += 1;
            }
        }
        Ok(())
    }

    fn literal(&mut self, literal: &[u8]) -> Result<(), Halt> {
        for expected in literal {
            if self.need()? != *expected {
                return Err(Halt::Invalid);
            }
            self.at += 1;
        }
        Ok(())
    }

    fn object(&mut self, depth: usize) -> Result<(), Halt> {
        self.at += 1;
        self.skip_space();
        if self.need()? == b'}' {
            self.at += 1;
            return Ok(());
        }
        loop {
            self.string()?;
            self.skip_space();
            if self.need()? != b':' {
                return Err(Halt::Invalid);
            }
            self.at += 1;
            self.value(depth + 1)?;
            self.skip_space();
            match self.need()? {
                b'}' => {
                    self.at += 1;
                    return Ok(());
                }
                b',' => {
                    self.at += 1;
                    self.skip_space();
                    self.need()?;
                }
                _ => return Err(Halt::Invalid),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<(), Halt> {
        self.at += 1;
        self.skip_space();
        if self.need()? == b']' {
            self.at += 1;
            return Ok(());
        }
        loop {
            self.value(depth + 1)?;
            self.skip_space();
            match self.need()? {
                b']' => {
                    self.at += 1;
                    return Ok(());
                }
                b',' => {
                    self.at += 1;
                    self.skip_space();
                    self.need()?;
                }
                _ => return Err(Halt::Invalid),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_json_is_complete_and_its_every_cut_is_a_prefix() {
        let whole = r#" {"a":[1,-2.5e+3,true,false,null,"x\"\\\/\b\f\n\r\tAé"],"b":{}} "#;
        assert_eq!(json_prefix_state(whole), Prefix::Complete);
        // Every cut of it, but the empty one and the space alone, is the start of JSON.
        for end in 1..whole.len() - 1 {
            if !whole.is_char_boundary(end) {
                continue;
            }
            let cut = &whole[..end];
            if cut.trim().is_empty() {
                continue;
            }
            assert_eq!(json_prefix_state(cut), Prefix::Incomplete, "{cut:?}");
        }
    }

    #[test]
    fn text_no_more_can_make_json_is_invalid() {
        for text in [
            "x",
            "{,",
            "[1,]",
            "{\"a\" 1}",
            "01",
            "-a",
            "1.e5",
            "1e+x",
            "\"\u{1}\"",
            "\"\\x\"",
            "\"\\u12g4\"",
            "tru e",
            "nulL",
            "{} {}",
            "[1] x",
            "\u{a0}",
            "{\"type\":\u{b}",
            "{\"type\":\u{c}",
        ] {
            assert_eq!(json_prefix_state(text), Prefix::Invalid, "{text:?}");
        }
    }

    #[test]
    fn what_more_could_finish_is_incomplete_and_nothing_at_all_is_too() {
        for text in [
            "",
            " ",
            "{\"type\":\"event_msg\"",
            "[",
            "-",
            "1.",
            "1e",
            "1e-",
            "\"\\u12",
            "tr",
            "{\"a\":",
        ] {
            assert_eq!(json_prefix_state(text), Prefix::Incomplete, "{text:?}");
        }
    }

    #[test]
    fn nesting_too_deep_for_a_stack_is_no_json_not_a_crash() {
        let deep = "[".repeat(100_000);
        assert_eq!(json_prefix_state(&deep), Prefix::Invalid);
        let shallow = format!("{}{}", "[".repeat(400), "]".repeat(400));
        assert_eq!(json_prefix_state(&shallow), Prefix::Complete);
    }
}
