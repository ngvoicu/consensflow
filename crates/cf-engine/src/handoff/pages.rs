//! Entries of the history packed into pages: `cf history`'s, and the pieces
//! of the history viewer's.

/// A page of `cf history` at its longest: under what Codex shows of a
/// command's output, the least of any harness (about 10 KiB and 256 lines).
const PAGE_BYTES: usize = 8_000;
const PAGE_LINES: usize = 200;

/// Room each page keeps for its own first and last lines.
const FRAME_BYTES: usize = 600;
const FRAME_LINES: usize = 6;

/// What a piece takes of a page, or what a page holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Size {
    pub(super) bytes: usize,
    pub(super) lines: usize,
}

impl Size {
    fn plus(self, other: Size) -> Size {
        Size {
            bytes: self.bytes + other.bytes,
            lines: self.lines + other.lines,
        }
    }

    fn within(self, room: Size) -> bool {
        self.bytes <= room.bytes && self.lines <= room.lines
    }
}

/// Pieces in the order they were said, packed into pages from the newest:
/// each page oldest first, the newest page first. A page takes pieces while
/// what they take is within `room`; `size` says what a piece takes given the
/// piece after it on its page (none for a page's newest), and the first piece
/// of a page is taken whatever it takes.
pub(super) fn pack<T>(
    pieces: Vec<T>,
    room: Size,
    size: impl Fn(&T, Option<&T>) -> Size,
) -> Vec<Vec<T>> {
    let mut pages = Vec::new();
    let mut page: Vec<T> = Vec::new();
    let mut used = Size::default();
    for piece in pieces.into_iter().rev() {
        let mut cost = size(&piece, page.last());
        if !page.is_empty() && !used.plus(cost).within(room) {
            page.reverse();
            pages.push(std::mem::take(&mut page));
            used = Size::default();
            cost = size(&piece, None);
        }
        page.push(piece);
        used = used.plus(cost);
    }
    if !page.is_empty() {
        page.reverse();
        pages.push(page);
    }
    pages
}

/// Entries in the order they were said, packed into pages from the newest:
/// each page oldest first, the newest page first.
pub(super) fn paginate(entries: &[String]) -> Vec<Vec<String>> {
    let room = Size {
        bytes: PAGE_BYTES - FRAME_BYTES,
        lines: PAGE_LINES - FRAME_LINES,
    };
    // A continuation line is added to every piece after the first.
    let pieces: Vec<String> = entries
        .iter()
        .flat_map(|entry| fit(entry, room.bytes - 16, room.lines - 1))
        .collect();
    pack(pieces, room, |piece, _| Size {
        bytes: piece.len() + 2,
        lines: lines(piece) + 1,
    })
}

/// How many lines `text` is.
fn lines(text: &str) -> usize {
    text.split('\n').count()
}

/// One entry cut to fit a page: by lines, and a line too long by characters.
fn fit(entry: &str, max_bytes: usize, max_lines: usize) -> Vec<String> {
    if entry.len() <= max_bytes && lines(entry) <= max_lines {
        return vec![entry.to_owned()];
    }
    let mut pieces = Vec::new();
    let mut piece = String::new();
    let flush = |pieces: &mut Vec<String>, piece: &mut String| {
        if !piece.is_empty() {
            pieces.push(std::mem::take(piece));
        }
    };
    for line in entry.split('\n') {
        let mut rest = line.to_owned();
        while rest.len() > max_bytes {
            let units: Vec<u16> = rest.encode_utf16().collect();
            let cut = cut_at(&units, max_bytes);
            flush(&mut pieces, &mut piece);
            pieces.push(String::from_utf16_lossy(&units[..cut]));
            rest = String::from_utf16_lossy(&units[cut..]);
        }
        let next = if piece.is_empty() {
            rest.clone()
        } else {
            format!("{piece}\n{rest}")
        };
        if next.len() > max_bytes || lines(&next) > max_lines {
            flush(&mut pieces, &mut piece);
            piece = rest;
        } else {
            piece = next;
        }
    }
    flush(&mut pieces, &mut piece);
    pieces
        .into_iter()
        .enumerate()
        .map(|(number, piece)| {
            if number == 0 {
                piece
            } else {
                format!("(continued)\n{piece}")
            }
        })
        .collect()
}

/// Where a line too long for a page is cut, in UTF-16 code units as the
/// JavaScript cut it: from the page's size in bytes, or the line's length in
/// units if that is less, a tenth less at a time until it fits. A cut between
/// the halves of a pair goes before the pair. A page holds thousands of bytes
/// and a unit takes three at most, so the cut is never none.
fn cut_at(units: &[u16], max_bytes: usize) -> usize {
    let mut cut = units.len().min(max_bytes);
    while utf8_len(&units[..cut]) > max_bytes {
        // A double's product, floored, as `Math.floor(cut * 0.9)`.
        cut = (cut as f64 * 0.9).floor() as usize;
    }
    if cut > 0 && (0xD800..0xDC00).contains(&units[cut - 1]) {
        cut -= 1;
    }
    cut
}

/// `Buffer.byteLength` of `units` as text: half of a pair counts as the
/// replacement character, three bytes.
fn utf8_len(units: &[u16]) -> usize {
    char::decode_utf16(units.iter().copied())
        .map(|unit| unit.map_or(char::REPLACEMENT_CHARACTER.len_utf8(), char::len_utf8))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_within_both_limits_is_one_piece() {
        assert_eq!(fit("one\ntwo", 100, 2), ["one\ntwo"]);
        assert_eq!(fit(&"x".repeat(100), 100, 1), ["x".repeat(100)]);
    }

    #[test]
    fn an_entry_of_too_many_lines_is_cut_into_pieces_each_after_the_first_continued() {
        let entry = (1..=7)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            fit(&entry, 100, 3),
            ["1\n2\n3", "(continued)\n4\n5\n6", "(continued)\n7"]
        );
    }

    #[test]
    fn a_line_too_long_is_cut_a_tenth_at_a_time_from_the_pages_size() {
        // 25 units of one byte under a page of 10 bytes: 10 fits, and 15 are left.
        let pieces = fit(&"a".repeat(25), 10, 100);
        assert_eq!(
            pieces,
            [
                "a".repeat(10),
                format!("(continued)\n{}", "a".repeat(10)),
                format!("(continued)\n{}", "a".repeat(5)),
            ]
        );
        // 30 units of two bytes: 10 units are 20 bytes, then 9, 8, 7 and 6,
        // which is 12 bytes, then 5, which is 10.
        let pieces = fit(&"é".repeat(30), 10, 100);
        assert_eq!(pieces.len(), 6);
        assert!(pieces
            .iter()
            .all(|piece| piece.trim_start_matches("(continued)\n").len() <= 10));
    }

    #[test]
    fn a_cut_never_falls_between_the_halves_of_a_pair() {
        let units: Vec<u16> = "🙂".repeat(5).encode_utf16().collect();
        // 10 units, 20 bytes under a page of 7: 7 units are 3 emoji and a
        // half (15 bytes), then 6 (12), 5 (2 and a half: 11), 4 (8), and 3,
        // an emoji and a half, which is 7 bytes: it fits, ends in the middle
        // of the second emoji, and is cut back before it.
        assert_eq!(cut_at(&units, 7), 2);
        let cut = fit(&"🙂".repeat(100), 30, 100);
        assert!(cut.iter().all(|piece| !piece.contains('\u{FFFD}')));
        let whole: String = cut
            .iter()
            .map(|piece| piece.trim_start_matches("(continued)\n"))
            .collect();
        assert_eq!(whole, "🙂".repeat(100));
    }

    #[test]
    fn half_a_pair_counts_three_bytes_whole_pairs_four() {
        let pair: Vec<u16> = "🙂".encode_utf16().collect();
        assert_eq!(utf8_len(&pair), 4);
        assert_eq!(utf8_len(&pair[..1]), 3);
        assert_eq!(utf8_len(&[pair[0], 0x61]), 4);
        assert_eq!(utf8_len(&[pair[0], pair[0]]), 6);
        assert_eq!(utf8_len(&[pair[1], pair[0]]), 6);
        assert_eq!(utf8_len(&[0x7F, 0x80, 0x7FF, 0x800]), 1 + 2 + 2 + 3);
    }

    #[test]
    fn a_piece_takes_what_it_takes_beside_the_piece_after_it_and_more_at_the_head_of_a_page() {
        // Each costs 10, and 5 more as the newest of its run on its page (its heading).
        let size = |piece: &(u8, char), newer: Option<&(u8, char)>| Size {
            bytes: if newer.is_some_and(|newer| newer.0 == piece.0) {
                10
            } else {
                15
            },
            lines: 0,
        };
        let room = |bytes| Size { bytes, lines: 0 };
        let pieces = vec![(1, 'a'), (1, 'b'), (2, 'c'), (2, 'd')];
        // d heads its run (15), c follows it (10), b heads its own (15): 40.
        assert_eq!(
            pack(pieces.clone(), room(40), size),
            [vec![(1, 'b'), (2, 'c'), (2, 'd')], vec![(1, 'a')]]
        );
        assert_eq!(
            pack(pieces, room(39), size),
            [vec![(2, 'c'), (2, 'd')], vec![(1, 'a'), (1, 'b')]]
        );
        // A piece that heads a new page takes its heading again: at 20, no two fit.
        let run = vec![(1, 'a'), (1, 'b'), (1, 'c')];
        assert_eq!(
            pack(run, room(20), size),
            [vec![(1, 'c')], vec![(1, 'b')], vec![(1, 'a')]]
        );
    }

    #[test]
    fn pages_hold_what_the_frame_leaves_newest_first_each_in_order() {
        // Each piece costs its bytes and 2, and its lines and 1; a page holds 7,400 and 194.
        let entries: Vec<String> = (0..4)
            .map(|n| format!("{n}{}", "x".repeat(3_000)))
            .collect();
        let pages = paginate(&entries);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0], entries[2..]);
        assert_eq!(pages[1], entries[..2]);
        // 2 x (3,001 + 2) = 6,006; a third piece of 1,392 + 2 makes 7,400, which fits.
        let tight = vec!["a".repeat(3_001), "b".repeat(3_001), "c".repeat(1_392)];
        assert_eq!(paginate(&tight).len(), 1);
        let over = vec!["a".repeat(3_001), "b".repeat(3_001), "c".repeat(1_393)];
        assert_eq!(paginate(&over).len(), 2);
        // An empty piece is one line, and costs two with its separator: 97 make 194.
        let steps = vec![String::new(); 97];
        assert_eq!(paginate(&steps).len(), 1);
        let one_more = vec![String::new(); 98];
        assert_eq!(paginate(&one_more).len(), 2);
    }
}
