use super::parser::Parser;
use super::tracker::Tracker;
use super::*;

const MIB: usize = 1 << 20;

fn journal() -> Journal {
    Journal::new((24, 80))
}

/// One message: the read appended, then settled, as the consumer does.
fn take(journal: &Journal, bytes: &[u8]) {
    journal.append(bytes);
    journal.settle();
}

/// A page subscribes now (what the hub had handed is all the journal holds) and pulls every
/// piece from where it has nothing. The bytes, the pieces.
fn pulled(journal: &Journal) -> (Vec<u8>, Vec<Piece>) {
    journal.begin_page(journal.end());
    pulled_to_boundary(journal)
}

fn pulled_to_boundary(journal: &Journal) -> (Vec<u8>, Vec<Piece>) {
    let mut bytes = Vec::new();
    let mut pieces = Vec::new();
    let mut from = None;
    loop {
        let piece = journal.piece(from);
        bytes.extend_from_slice(&piece.bytes);
        from = Some(piece.from + piece.bytes.len() as u64);
        let done = piece.done;
        pieces.push(piece);
        if done {
            return (bytes, pieces);
        }
    }
}

/// Bytes that are not a sequence of any kind, one distinct value a position.
fn text(from: usize, len: usize) -> Vec<u8> {
    (from..from + len).map(|i| b'a' + (i % 23) as u8).collect()
}

/// A journal that has passed its cap: 12 MiB of plain text in 64 KiB messages after `opening`.
fn past_the_cap(opening: &[u8], _unused: &[(u64, (u16, u16))]) -> Journal {
    let journal = journal();
    take(&journal, opening);
    let chunk = text(0, 64 * 1024);
    while journal.end() < 12 * MIB as u64 {
        take(&journal, &chunk);
    }
    journal
}

#[test]
fn the_pieces_are_exactly_the_bytes_in_order_across_segment_edges() {
    let journal = journal();
    let mut expected = Vec::new();
    let mut at = 0;
    for size in [1, 7, 8190, 8192, 65_536, 1] {
        for _ in 0..40 {
            let read = text(at, size);
            at += size;
            take(&journal, &read);
            expected.extend_from_slice(&read);
        }
    }
    while expected.len() < 3 * MIB {
        let read = text(at, 8190);
        at += 8190;
        take(&journal, &read);
        expected.extend_from_slice(&read);
    }
    let (bytes, pieces) = pulled(&journal);
    assert_eq!(bytes.len(), expected.len());
    assert!(bytes == expected, "the bytes differ");
    assert!(pieces.len() >= 3, "a piece holds at most 1 MiB");
    assert!(pieces.iter().all(|piece| piece.bytes.len() <= PIECE_BYTES));
    assert!(pieces.iter().skip(1).all(|piece| !piece.reset));
}

#[test]
fn a_flood_keeps_eight_segments_at_most_and_starts_again_at_a_mark() {
    let journal = past_the_cap(b"\x1b[?2004h", &[]);
    journal.look(|inner| {
        assert!(inner.segments.len() <= MAX_SEGMENTS + 1);
        assert!(inner.start > 0);
        assert!(inner.marks.iter().any(|mark| mark.offset == inner.start));
    });
    let (_, pieces) = pulled(&journal);
    let first = &pieces[0];
    assert!(first.reset && first.truncated);
    assert_eq!(first.from, journal.look(|inner| inner.start));
    assert_eq!(first.seed, b"\x1b[?2004h".to_vec(), "only the modes");
}

#[test]
fn two_million_one_byte_reads_hold_two_segments_not_a_buffer_a_read() {
    let journal = journal();
    for i in 0..2 * MIB + 1 {
        take(&journal, &[b'a' + (i % 23) as u8]);
    }
    let capacities = journal.look(|inner| {
        inner
            .segments
            .iter()
            .map(Vec::capacity)
            .collect::<Vec<usize>>()
    });
    assert_eq!(capacities.len(), 3);
    assert!(capacities.iter().all(|capacity| *capacity == MIB));
}

/// Marks only where xterm's parser is in its ground state between whole characters: a read
/// that ends inside any of these, with 1 MiB passed, is no mark, and the next that ends at rest is.
#[test]
fn no_mark_is_taken_inside_a_sequence_or_a_character_and_one_follows_its_end() {
    let cases: &[(&[u8], &[u8])] = &[
        (b"\x1b[?20", b"04h"),
        (b"\x1b[?20\x7f", b"04h"),
        (b"\x1b[C\xc2", b"\xa0"),
        (b"A\xe2\x80", b"\x94"),
        (b"\xe2\x82", b"\xac"),
        (b"\x1b]0;title ", b"x\x07"),
        (b"\x1bPq payload\x07", b"\x1b\\"),
    ];
    for (open, close) in cases {
        let journal = journal();
        take(&journal, &vec![b'x'; MIB - open.len()]);
        take(&journal, open);
        assert_eq!(journal.end(), MIB as u64);
        let marks = journal.look(|inner| inner.marks.iter().map(|m| m.offset).collect::<Vec<_>>());
        assert_eq!(marks, vec![0], "{open:?}: no mark inside it");
        take(&journal, close);
        let marks = journal.look(|inner| inner.marks.iter().map(|m| m.offset).collect::<Vec<_>>());
        assert_eq!(
            marks,
            vec![0, journal.end()],
            "{open:?}: a mark where it ended"
        );
    }
}

#[test]
fn a_mark_is_forced_after_two_mebibytes_without_a_rest() {
    let journal = journal();
    take(&journal, b"\x1b]0;");
    let filler = vec![b'x'; 64 * 1024];
    for _ in 0..33 {
        take(&journal, &filler);
    }
    let marks = journal.look(|inner| inner.marks.iter().map(|m| m.offset).collect::<Vec<_>>());
    assert_eq!(marks.len(), 2, "limit 5: the forced mark");
    assert!(marks[1] >= (2 * MIB) as u64);
}

#[test]
#[allow(clippy::disallowed_methods)]
fn the_parser_rests_where_xterm_does_for_every_next_byte() {
    let states: &[&[u8]] = &[
        b"",
        b"\x1b",
        b"\x1b(",
        b"\x1b[",
        b"\x1b[?20",
        b"\x1b[!",
        b"\x1b[??",
        b"\x1b]0;",
        b"\x1bX",
        b"\x1bP",
        b"\x1bP1",
        b"\x1bP!",
        b"\x1bP?1?",
        b"\x1bPq",
    ];
    let prefixes: Vec<Vec<u8>> = states
        .iter()
        .flat_map(|state| {
            [
                state.to_vec(),
                [*state, b"\xc2"].concat(),
                [*state, b"\xe2\x82"].concat(),
            ]
        })
        .collect();
    let entries: Vec<_> = prefixes
        .iter()
        .map(|prefix| {
            let mut cuts = Vec::with_capacity(256);
            for byte in 0..=255 {
                let mut parser = Parser::default();
                for b in prefix.iter().copied().chain(std::iter::once(byte)) {
                    parser.advance(b, &mut |_| {});
                }
                cuts.push(parser.at_rest());
            }
            serde_json::json!({"prefix": prefix, "cuts": cuts})
        })
        .collect();
    let actual = serde_json::to_vec(&entries).unwrap();
    if let Some(path) = std::env::var_os("CF_WRITE_P5B_FIXTURE") {
        let mut path = std::path::PathBuf::from(path);
        path.set_file_name("p5b-parser.json");
        std::fs::write(path, actual).unwrap();
    } else {
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../app/tests/fixtures/p5b-parser.json"
        ))
        .unwrap();
        assert_eq!(
            serde_json::Value::Array(entries),
            expected,
            "regenerate the parser fixture (CF_WRITE_P5B_FIXTURE)"
        );
    }
}

/// The seed a journal that starts past its first byte gives, for the stream that came before.
fn seed_of(stream: &[u8]) -> String {
    let journal = past_the_cap(stream, &[]);
    let (_, pieces) = pulled(&journal);
    String::from_utf8(pieces[0].seed.clone()).unwrap()
}

#[test]
fn a_mark_holds_the_modes_and_the_margins_and_nothing_drawn() {
    let seed = seed_of(
        b"text \x1b[2;6r\x1b[?2004h\x1b[?1h\x1b=\x1b[?1004h\x1b[?1002h\x1b[?1006h\x1b[?6h\x1b[?25l HELLO",
    );
    assert!(seed.contains("\x1b[2;6r"), "{seed:?}");
    for mode in [
        "\x1b[?2004h",
        "\x1b[?1h",
        "\x1b[?1004h",
        "\x1b[?1002h",
        "\x1b[?1006h",
        "\x1b[?6h",
        "\x1b[?25l",
    ] {
        assert!(seed.contains(mode), "{mode:?} in {seed:?}");
    }
    assert!(
        !seed.contains("HELLO") && !seed.contains("text"),
        "no screen text: {seed:?}"
    );
    assert!(
        !seed.contains('H') || !seed.contains("\x1b[1;1H"),
        "no cursor placement: {seed:?}"
    );
    assert_eq!(seed_of(b"\x1b[?2004h\x1b[?2004l"), "");
    assert_eq!(seed_of(b"\x1b[?2004h\x1bc"), "", "a full reset");
    assert_eq!(seed_of(b"\x1b[?2004h\x1b[!p"), "", "a soft reset");
    assert_eq!(
        seed_of(b"\x1b[2;6r\x1b[r"),
        "",
        "margins reset to the whole screen"
    );
    assert_eq!(
        seed_of(b"\x1b[6;2r"),
        "",
        "a region that is not one is refused"
    );
    assert!(seed_of(b"\x1b[?1049h").starts_with("\x1b[?47h"));
}

/// Red first (T-2121 finding 5): a size that came at byte 1, before any pull, then a margin the
/// program set after it. The later margin is the program's; a size taken after it clears it.
#[test]
fn a_mark_respects_a_margin_set_after_a_size_and_loses_one_set_before() {
    // 24 -> 10 rows at byte 1, then DECSTBM 2;6.
    let journal = journal();
    take(&journal, b"a");
    journal.resize(Some(1), (10, 80));
    take(&journal, b"\x1b[2;6r");
    let chunk = text(0, 64 * 1024);
    while journal.end() < 12 * MIB as u64 {
        take(&journal, &chunk);
    }
    let (_, pieces) = pulled(&journal);
    assert_eq!(String::from_utf8_lossy(&pieces[0].seed), "\x1b[2;6r");
    assert_eq!(pieces[0].size, (10, 80));
    // The same bytes, the size taken after the margins: xterm resets them.
    let journal = self::journal();
    take(&journal, b"a\x1b[2;6r");
    journal.resize(Some(journal.end()), (10, 80));
    while journal.end() < 12 * MIB as u64 {
        take(&journal, &chunk);
    }
    let (_, pieces) = pulled(&journal);
    assert_eq!(String::from_utf8_lossy(&pieces[0].seed), "");
    // A size at the very byte the margins ended: the margins came first, the size resets.
    let journal = self::journal();
    take(&journal, b"a\x1b[2;6r");
    journal.resize(Some(7), (10, 80));
    while journal.end() < 12 * MIB as u64 {
        take(&journal, &chunk);
    }
    let (_, pieces) = pulled(&journal);
    assert_eq!(String::from_utf8_lossy(&pieces[0].seed), "");
}

#[test]
fn eight_rows_then_ten_then_eight_at_one_byte_keeps_both_and_resets_the_margins() {
    let journal = Journal::new((8, 30));
    take(&journal, b"\x1b[2;6r");
    journal.resize(Some(journal.end()), (10, 30));
    journal.resize(Some(journal.end()), (8, 30));
    let records = journal.look(|inner| inner.resizes.clone());
    assert_eq!(records.len(), 2, "no height change is merged");
    let chunk = text(0, 64 * 1024);
    while journal.end() < 12 * MIB as u64 {
        take(&journal, &chunk);
    }
    let (_, pieces) = pulled(&journal);
    assert_eq!(pieces[0].size, (8, 30));
    assert_eq!(
        String::from_utf8_lossy(&pieces[0].seed),
        "",
        "xterm reset them twice"
    );
    // Past the cap too: margins set after the pair are kept.
    let journal = Journal::new((8, 30));
    take(&journal, b"x");
    journal.resize(Some(1), (10, 30));
    journal.resize(Some(1), (8, 30));
    take(&journal, b"\x1b[2;6r");
    while journal.end() < 12 * MIB as u64 {
        take(&journal, &chunk);
    }
    let (_, pieces) = pulled(&journal);
    assert_eq!(String::from_utf8_lossy(&pieces[0].seed), "\x1b[2;6r");
}

#[test]
fn a_size_is_journaled_at_the_byte_the_page_took_it_and_the_wrong_ones_take_the_end() {
    let journal = journal();
    take(&journal, &text(0, 1000));
    journal.resize(Some(400), (24, 100));
    journal.resize(Some(300), (24, 90)); // before the last size taken: the end
    journal.resize(Some(5000), (24, 91)); // past the end: the end
    journal.resize(None, (24, 92)); // not the current page's: the end
    journal.resize(Some(1000), (24, 92)); // the same as the last: none
    let records = journal.look(|inner| inner.resizes.clone());
    assert_eq!(
        records.iter().map(|r| (r.at, r.cols)).collect::<Vec<_>>(),
        vec![(400, 100), (1000, 90), (1000, 91), (1000, 92)]
    );
    let (bytes, pieces) = pulled(&journal);
    assert_eq!(bytes.len(), 1000);
    assert_eq!(pieces[0].resizes.len(), 4);
    assert_eq!(pieces[0].size, (24, 80));
}

#[test]
fn a_size_before_the_boundary_takes_the_boundary() {
    let journal = journal();
    take(&journal, &text(0, 1000));
    journal.begin_page(600);
    take(&journal, &text(1000, 500));
    journal.resize(Some(100), (24, 100));
    let records = journal.look(|inner| inner.resizes.clone());
    assert_eq!(records.iter().map(|r| r.at).collect::<Vec<_>>(), vec![600]);
    let piece = journal.piece(None);
    assert_eq!(
        (piece.bytes.len(), piece.done, piece.boundary),
        (600, true, 600)
    );
    assert_eq!(piece.resizes.len(), 1, "a pull's bytes and its sizes agree");
}

/// Red first (T-2121 finding 8): 257 and 4,096 sizes at one byte. No byte is in the history
/// twice, none is missing, and the journal holds no more sizes than its bound; where it starts
/// again it is at a rest point.
#[test]
fn a_flood_of_sizes_at_one_byte_duplicates_and_loses_no_byte() {
    for count in [257, 300, 4096, 5000] {
        let journal = Journal::new((24, 80));
        let mut all = Vec::new();
        let mut at = 0;
        for i in 0..40 {
            let read = text(at, 997);
            at += 997;
            take(&journal, &read);
            all.extend_from_slice(&read);
            if i == 20 {
                for n in 0..count {
                    journal.resize(Some(journal.end()), (24, 81 + (n % 2) as u16));
                }
            }
        }
        let records = journal.look(|inner| inner.resizes.len());
        assert!(records <= MAX_RESIZES, "{count}: {records} sizes kept");
        let (bytes, pieces) = pulled(&journal);
        let start = pieces[0].from;
        assert!(start > 0, "{count}: the journal started again");
        assert!(
            bytes == all[start as usize..],
            "{count}: the bytes from {start} are the window's, once each"
        );
        // The new start is a mark at a rest point: the window's own stream from the start is
        // parsed from the ground state.
        let mut tracker = Tracker::default();
        tracker.feed(&all[..start as usize]);
        assert!(tracker.at_rest(), "{count}: not at rest at {start}");
    }
}

/// Red first (T-2121 finding 8): a record is not cut inside a paste-mode sequence at byte 263.
#[test]
fn a_flood_of_sizes_inside_a_sequence_starts_again_at_the_last_mark_not_inside_it() {
    let journal = Journal::new((24, 80));
    take(&journal, &vec![b'x'; MIB + 10]); // a mark at 1 MiB + 10
    let mark = journal
        .look(|inner| inner.marks.back().map(|m| m.offset))
        .unwrap();
    take(&journal, b"\x1b[?20"); // inside "ESC [ ? 2 0 0 4 h"
    for n in 0..300 {
        journal.resize(Some(journal.end()), (24, 81 + (n % 2) as u16));
    }
    let start = journal.look(|inner| inner.start);
    assert_eq!(start, mark, "starts at the last mark, which is at rest");
    take(&journal, b"04h");
    let journal_end = journal.end();
    let (bytes, _) = pulled(&journal);
    assert_eq!(bytes.len() as u64, journal_end - mark);
    assert!(bytes.ends_with(b"\x1b[?2004h"));
    // The sequence is whole in what the page replays: a fresh tracker that reads the seed and
    // the bytes after the mark sees paste mode on.
    let mut tracker = Tracker::default();
    tracker.feed(&bytes);
    assert!(tracker.at_rest() && tracker.tracked().modes.paste);
}

#[test]
fn a_flood_of_sizes_inside_one_sequence_past_the_hard_bound_starts_again_at_the_end() {
    let journal = Journal::new((24, 80));
    take(&journal, b"abc\x1b[?20");
    for n in 0..5000 {
        journal.resize(Some(journal.end()), (24, 81 + (n % 2) as u16));
    }
    let records = journal.look(|inner| inner.resizes.len());
    assert!(records <= HARD_MAX_RESIZES, "{records}");
    assert!(journal.look(|inner| inner.start) > 0);
}

#[test]
fn an_ended_window_keeps_its_last_mebibyte_and_a_continuation_from_it_finds_its_bytes() {
    let journal = journal();
    let mut all = Vec::new();
    for i in 0..(6 * 16) {
        let read = text(i * 64 * 1024, 64 * 1024);
        take(&journal, &read);
        all.extend_from_slice(&read);
    }
    let end = journal.end();
    journal.begin_page(end);
    let behind = end - 65_536 - 1;
    let before = journal.piece(Some(behind));
    assert!(!before.reset);
    journal.finish();
    let after = journal.piece(Some(behind));
    assert!(!after.reset, "the tail still holds where the page was");
    assert_eq!(
        before.bytes, after.bytes,
        "the same bytes before and after the exit"
    );
    assert!(after.bytes == all[behind as usize..]);
    // The journal keeps at least the last mebibyte and not much more than three.
    let held = journal.look(|inner| inner.end - inner.start);
    assert!(
        (MIB as u64..=3 * MIB as u64 + 65_536).contains(&held),
        "{held}"
    );
    // Asked again, the same.
    assert_eq!(journal.piece(Some(behind)), after);
}

#[test]
fn a_page_behind_the_start_of_the_journal_is_given_a_reset_piece() {
    let journal = past_the_cap(b"", &[]);
    journal.begin_page(journal.end());
    let piece = journal.piece(Some(10));
    assert!(piece.reset && piece.truncated);
    let none = journal.piece(None);
    assert_eq!(piece, none);
}

/// A restart past the boundary: the page is given the start and told it is done there.
#[test]
fn a_journal_that_started_again_past_the_boundary_says_so() {
    let journal = journal();
    take(&journal, &text(0, 1000));
    journal.begin_page(1000);
    take(&journal, &text(1000, 500));
    for n in 0..300 {
        journal.resize(Some(journal.end()), (24, 81 + (n % 2) as u16));
    }
    let piece = journal.piece(None);
    assert_eq!((piece.from, piece.boundary), (1500, 1000));
    assert!(piece.reset && piece.done && piece.bytes.is_empty());
}

/// Found by the randomized gate (T-2126): xterm's tab stops depend on the widths it has been, so
/// a piece that starts an emulator clean gives the size at the journal's start and every size
/// taken since, those at the first byte too. A continuation gives those after where it starts.
#[test]
fn a_reset_piece_gives_every_size_since_the_start_and_a_continuation_those_after_it() {
    let journal = Journal::new((8, 30));
    for (rows, cols) in [(5, 52), (14, 9), (21, 71)] {
        journal.resize(Some(0), (rows, cols));
    }
    take(&journal, b"abc");
    journal.resize(Some(2), (21, 40));
    journal.begin_page(3);
    let first = journal.piece(None);
    assert!(first.reset && !first.truncated);
    assert_eq!(first.size, (8, 30), "the size the window opened at");
    assert_eq!(
        first
            .resizes
            .iter()
            .map(|r| (r.at, r.cols))
            .collect::<Vec<_>>(),
        vec![(0, 52), (0, 9), (0, 71), (2, 40)]
    );
    let next = journal.piece(Some(2));
    assert!(!next.reset);
    assert!(
        next.resizes.is_empty(),
        "the one at byte 2 is the page's own: it has it"
    );
}

/// T-2171, blocker 1: xterm resets its margins on every size it takes, a width alone included.
#[test]
fn a_size_that_changes_only_the_width_resets_the_margins() {
    let journal = Journal::new((12, 40));
    take(&journal, b"\x1b[2;9r");
    journal.resize(Some(journal.end()), (12, 50));
    let chunk = text(0, 64 * 1024);
    while journal.end() < 12 * MIB as u64 {
        take(&journal, &chunk);
    }
    let (_, pieces) = pulled(&journal);
    assert_eq!(String::from_utf8_lossy(&pieces[0].seed), "");
    // And 257 width changes, with no cap passed: the journal starts again at the 257th.
    let journal = Journal::new((12, 40));
    take(&journal, b"\x1b[2;9rabc");
    for i in 0..257 {
        journal.resize(Some(journal.end()), (12, 41 + (i % 2) as u16));
    }
    let (_, pieces) = pulled(&journal);
    assert!(pieces[0].truncated);
    assert_eq!(String::from_utf8_lossy(&pieces[0].seed), "");
}

/// Blocker 2: margins are kept for each screen, as xterm keeps them.
#[test]
fn margins_are_kept_for_each_screen() {
    // Set on the normal screen, then the alternate screen shows: its own are the whole screen.
    assert_eq!(seed_of(b"\x1b[2;9r\x1b[?1049h"), "\x1b[2;9r\x1b[?47h");
    // Set on the alternate screen and left: dropped, the normal screen's are the whole screen.
    assert_eq!(seed_of(b"\x1b[?1049h\x1b[2;9r\x1b[?1049l"), "");
    // Set on the alternate screen while it shows.
    assert_eq!(seed_of(b"\x1b[?1049h\x1b[2;9r"), "\x1b[?47h\x1b[2;9r");
    // Both, the normal screen's written before the switch and the alternate's after it.
    assert_eq!(
        seed_of(b"\x1b[3;7r\x1b[?1049h\x1b[2;9r"),
        "\x1b[3;7r\x1b[?47h\x1b[2;9r"
    );
    // A soft reset resets the screen that shows only.
    assert_eq!(
        seed_of(b"\x1b[3;7r\x1b[?1049h\x1b[2;9r\x1b[!p"),
        "\x1b[3;7r\x1b[?47h"
    );
}

/// Blocker 3: a region xterm refuses keeps the one it had. The host does not know the rows, so
/// the settings since the screen's margins were last the whole screen are written in order and
/// the page's xterm refuses at its own rows what the old one did.
#[test]
fn a_region_the_screen_may_refuse_is_written_after_the_one_it_keeps() {
    assert_eq!(seed_of(b"\x1b[2;9r\x1b[20;30r"), "\x1b[2;9r\x1b[20;30r");
    assert_eq!(seed_of(b"\x1b[2;9r\x1b[12r"), "\x1b[2;9r\x1b[12r");
    // A region that no screen takes is not kept; the whole screen forgets the ones before.
    assert_eq!(seed_of(b"\x1b[2;9r\x1b[9;2r"), "\x1b[2;9r");
    assert_eq!(seed_of(b"\x1b[2;9r\x1b[1;0r"), "");
    // A few at most.
    let many: Vec<u8> = (2..40)
        .flat_map(|top| format!("\x1b[{top};50r").into_bytes())
        .collect();
    let seed = seed_of(&many);
    assert_eq!(seed.matches('r').count(), 16);
    assert!(seed.ends_with("\x1b[39;50r"));
}
