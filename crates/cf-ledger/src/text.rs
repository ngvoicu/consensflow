//! A text value as the ledger reads it.

/// A text's `bytes` as text: what is no UTF-8 in them (which no build writes,
/// each writing its language's own strings) is U+FFFD, never refused.
pub(crate) fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_no_utf_8_in_a_text_is_a_replacement() {
        assert_eq!(text_of("aé🙂".as_bytes()), "aé🙂");
        assert_eq!(text_of(b"a\xED\xA0\x80b"), "a\u{FFFD}\u{FFFD}\u{FFFD}b");
    }
}
