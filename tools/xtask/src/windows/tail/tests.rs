use super::*;

/// What `chunks` come to when a stream says them one after the other.
fn last_of(chunks: &[&[u8]]) -> Option<String> {
    let mut tail = Tail::default();
    for chunk in chunks {
        tail.push(chunk);
    }
    tail.last_line()
}

#[test]
fn the_last_line_is_the_one_that_ends_the_stream_with_or_without_its_line_end() {
    assert_eq!(last_of(&[b"a\nb\n"]).as_deref(), Some("b"));
    assert_eq!(last_of(&[b"a\r\nb\r\n"]).as_deref(), Some("b"));
    assert_eq!(last_of(&[b"a\nb"]).as_deref(), Some("b"));
    assert_eq!(last_of(&[b"only"]).as_deref(), Some("only"));
    // A line that is no more than spaces and ends is not one that was said last.
    assert_eq!(last_of(&[b"a\n\n  \r\n\r\n"]).as_deref(), Some("a"));
    assert_eq!(last_of(&[b"== exit 7\r\n"]).as_deref(), Some("== exit 7"));
}

#[test]
fn a_stream_that_said_nothing_has_no_last_line() {
    assert_eq!(last_of(&[]), None);
    assert_eq!(last_of(&[b""]), None);
    assert_eq!(last_of(&[b"\r\n\r\n"]), None);
}

#[test]
fn chunks_that_end_where_they_like_say_the_same_line_as_the_stream_whole() {
    let said: &[u8] = b"first\r\nsecond line\r\n== exit 75\r\n";
    // Every way to cut it in two, which includes a cut between the CR and its LF
    // and one inside the last line.
    for at in 0..=said.len() {
        let (head, rest) = said.split_at(at);
        assert_eq!(
            last_of(&[head, rest]).as_deref(),
            Some("== exit 75"),
            "cut at {at}"
        );
    }
    // And one byte at a time.
    let bytes: Vec<&[u8]> = said.chunks(1).collect();
    assert_eq!(last_of(&bytes).as_deref(), Some("== exit 75"));
}

#[test]
fn a_character_cut_between_two_chunks_is_read_whole() {
    let said = "ü\n== exit 1\né".as_bytes();
    // The two bytes of the last é come apart.
    let (head, rest) = said.split_at(said.len() - 1);
    assert_eq!(last_of(&[head, rest]).as_deref(), Some("é"));
}

#[test]
fn what_is_kept_is_the_end_and_a_line_longer_than_it_is_not_known() {
    let long = "x".repeat(KEEP * 3);
    // Lines before are let go, and the last, which fits, is whole.
    let said = format!("{long}\n{long}\n== exit 3\n");
    assert_eq!(last_of(&[said.as_bytes()]).as_deref(), Some("== exit 3"));
    // Many chunks that each fit and together do not.
    let chunks: Vec<&[u8]> = said.as_bytes().chunks(100).collect();
    assert_eq!(last_of(&chunks).as_deref(), Some("== exit 3"));
    // A last line that was cut at its start is no line: its end, read alone, could
    // be the end of anything (`...== exit 5`).
    let one_line = format!("{long}== exit 5");
    assert_eq!(last_of(&[one_line.as_bytes()]), None);
    let chunks: Vec<&[u8]> = one_line.as_bytes().chunks(100).collect();
    assert_eq!(last_of(&chunks), None);
    // A short stream is whole, however many chunks it came in.
    assert_eq!(
        last_of(&[b"== ", b"exit ", b"5"]).as_deref(),
        Some("== exit 5")
    );
}

#[test]
fn only_the_end_is_kept() {
    let mut tail = Tail::default();
    for _ in 0..100 {
        tail.push(&[b'a'; 500]);
    }
    assert!(tail.kept.len() <= KEEP, "{}", tail.kept.len());
}
