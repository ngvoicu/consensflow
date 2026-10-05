//! The input as lines: bytes in, whole lines out. A line is decoded only
//! once it is whole, so a character split across two reads is never broken,
//! and an endless line cannot fill memory: what is kept of an unterminated
//! one never passes the limit.

/// What the input held next.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Line {
    /// A line with its newline taken off, and the carriage return before it.
    /// It may be over the limit: it arrived whole, so it can be refused with
    /// an answer.
    Complete(Vec<u8>),
    /// An unterminated line grew past the limit. It is dropped, and so is
    /// the rest of it through its newline.
    Overflow,
}

/// Splits what the input reads into lines. A caller pushes a chunk and then
/// takes lines until none is left, before it pushes the next.
pub(super) struct Lines {
    max: usize,
    buffer: Vec<u8>,
    /// Where the next line starts in `buffer`.
    start: usize,
    /// How many bytes after `start` are known to hold no newline.
    scanned: usize,
    /// Dropping the rest of an overflowed line, through its newline.
    discarding: bool,
}

impl Lines {
    pub(super) fn new(max: usize) -> Self {
        Self {
            max,
            buffer: Vec::new(),
            start: 0,
            scanned: 0,
            discarding: false,
        }
    }

    pub(super) fn push(&mut self, chunk: &[u8]) {
        if self.discarding {
            let Some(newline) = chunk.iter().position(|byte| *byte == b'\n') else {
                return;
            };
            self.discarding = false;
            self.buffer.extend_from_slice(&chunk[newline + 1..]);
            return;
        }
        self.buffer.extend_from_slice(chunk);
    }

    pub(super) fn next_line(&mut self) -> Option<Line> {
        let from = self.start + self.scanned;
        let Some(offset) = self.buffer[from..].iter().position(|byte| *byte == b'\n') else {
            return self.keep_the_tail();
        };
        let end = from + offset;
        let mut line = self.buffer[self.start..end].to_vec();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        self.start = end + 1;
        self.scanned = 0;
        Some(Line::Complete(line))
    }

    /// No newline is left: what follows the last line is an unterminated one,
    /// kept for the next chunk while it fits.
    fn keep_the_tail(&mut self) -> Option<Line> {
        self.buffer.drain(..self.start);
        self.start = 0;
        if self.buffer.len() > self.max {
            self.buffer.clear();
            self.scanned = 0;
            self.discarding = true;
            return Some(Line::Overflow);
        }
        self.scanned = self.buffer.len();
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(lines: &mut Lines, chunk: &[u8]) -> Vec<Line> {
        lines.push(chunk);
        std::iter::from_fn(|| lines.next_line()).collect()
    }

    fn whole(text: &str) -> Line {
        Line::Complete(text.as_bytes().to_vec())
    }

    #[test]
    fn a_line_comes_out_with_its_newline_and_its_carriage_return_taken_off() {
        let mut lines = Lines::new(100);
        assert_eq!(
            take(&mut lines, b"one\ntwo\r\n"),
            [whole("one"), whole("two")]
        );
    }

    #[test]
    fn a_line_split_across_chunks_comes_out_when_its_newline_arrives() {
        let mut lines = Lines::new(100);
        assert_eq!(take(&mut lines, b"par"), []);
        assert_eq!(take(&mut lines, b"tial"), []);
        assert_eq!(take(&mut lines, b" line\nnext"), [whole("partial line")]);
        assert_eq!(take(&mut lines, b"\n"), [whole("next")]);
    }

    #[test]
    fn an_empty_line_comes_out_empty_for_the_caller_to_skip() {
        let mut lines = Lines::new(100);
        assert_eq!(
            take(&mut lines, b"\n\r\nx\n"),
            [whole(""), whole(""), whole("x")]
        );
    }

    #[test]
    fn only_the_carriage_return_before_the_newline_is_taken_off() {
        let mut lines = Lines::new(100);
        assert_eq!(take(&mut lines, b"a\rb\r\r\n"), [whole("a\rb\r")]);
    }

    #[test]
    fn a_complete_line_over_the_limit_arrives_whole_to_be_refused_with_an_answer() {
        let mut lines = Lines::new(10);
        let line = "x".repeat(50);
        assert_eq!(
            take(&mut lines, format!("{line}\nafter\n").as_bytes()),
            [whole(&line), whole("after")]
        );
    }

    #[test]
    fn an_unterminated_line_over_the_limit_overflows_once_and_is_dropped_through_its_newline() {
        let mut lines = Lines::new(10);
        assert_eq!(
            take(&mut lines, "x".repeat(30).as_bytes()),
            [Line::Overflow]
        );
        assert_eq!(take(&mut lines, "y".repeat(30).as_bytes()), []);
        assert_eq!(take(&mut lines, b"still the same line"), []);
        assert_eq!(take(&mut lines, b"end\nok\n"), [whole("ok")]);
        assert_eq!(take(&mut lines, b"next\n"), [whole("next")]);
    }

    #[test]
    fn an_overflow_comes_after_the_whole_lines_of_its_chunk() {
        let mut lines = Lines::new(10);
        let chunk = format!("one\ntwo\n{}", "z".repeat(40));
        assert_eq!(
            take(&mut lines, chunk.as_bytes()),
            [whole("one"), whole("two"), Line::Overflow]
        );
    }

    #[test]
    fn fragments_that_add_up_past_the_limit_overflow_when_they_do() {
        let mut lines = Lines::new(100);
        for _ in 0..3 {
            assert_eq!(take(&mut lines, "x".repeat(30).as_bytes()), []);
        }
        assert_eq!(
            take(&mut lines, "x".repeat(30).as_bytes()),
            [Line::Overflow]
        );
        assert_eq!(take(&mut lines, b"\n"), []);
        assert_eq!(take(&mut lines, b"fine\n"), [whole("fine")]);
    }

    #[test]
    fn an_unterminated_line_at_the_limit_is_kept() {
        let mut lines = Lines::new(10);
        assert_eq!(take(&mut lines, "x".repeat(10).as_bytes()), []);
        assert_eq!(take(&mut lines, b"\n"), [whole(&"x".repeat(10))]);
    }

    #[test]
    fn a_character_split_between_two_chunks_comes_out_whole() {
        let mut lines = Lines::new(100);
        assert_eq!(take(&mut lines, &[0xe2, 0x82]), []);
        assert_eq!(
            take(&mut lines, &[0xac, b'\n']),
            [Line::Complete(vec![0xe2, 0x82, 0xac])]
        );
    }
}
