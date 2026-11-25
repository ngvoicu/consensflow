//! Count the cursor queries a window prints, including those cut across frames.

const QUERY: [u8; 4] = [0x1b, b'[', b'6', b'n'];

#[derive(Debug, Default)]
pub(super) struct CursorQueries {
    matched: usize,
}

impl CursorQueries {
    /// Feed one frame, returning the number of complete queries in it.
    pub(super) fn take(&mut self, bytes: &[u8]) -> usize {
        let mut complete = 0;
        for &byte in bytes {
            if byte == QUERY[self.matched] {
                self.matched += 1;
                if self.matched == QUERY.len() {
                    complete += 1;
                    self.matched = 0;
                }
            } else {
                // Escape may itself begin the next query, including after a
                // partial query that was cancelled by another Escape.
                self.matched = usize::from(byte == QUERY[0]);
            }
        }
        complete
    }

    pub(super) fn unfinished(&self) -> bool {
        self.matched != 0
    }
}

#[cfg(test)]
mod tests {
    use super::CursorQueries;

    #[test]
    fn cursor_queries_count_complete_and_split_sequences() {
        let mut queries = CursorQueries::default();
        assert_eq!(queries.take(b"\x1b[6n\x1b[6n\x1b["), 2);
        assert!(queries.unfinished());
        assert_eq!(queries.take(b"6n\x1b\x1b[6n"), 2);
        assert!(!queries.unfinished());
    }
}
