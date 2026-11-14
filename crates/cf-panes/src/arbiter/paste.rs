use std::borrow::Cow;
use std::ops::Range;
use std::time::{Duration, Instant};

/// How a bracketed paste begins and ends.
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

/// How many characters `text` holds: its bytes that begin one, which is every one but a
/// UTF-8 continuation byte, so that a character split between two writes is counted once.
pub(super) fn characters_in(text: &[u8]) -> usize {
    text.iter().filter(|byte| **byte & 0xC0 != 0x80).count()
}

/// Where the human's input stands in a bracketed paste, across the writes it comes in. The
/// page sends input in chunks of 32 KiB (`app/ui/terminal-link.js`), so a big paste is
/// several writes, with its opening marker in the first and its closing marker in the last,
/// and a chunk can end inside a marker. It keeps the characters of the paste now open,
/// counted from after its opening marker to its closing one over all the writes it takes,
/// and the last bytes of the writes so far when they could begin the marker looked for next.
/// Those bytes are scan state alone: they went to the window with their write, and are
/// counted as characters of the paste only once it is known they are none of a marker's.
///
/// What it says of each write serves two readers: where a paste ends in it, for the hold of
/// the keys that follow it ([`Fed::ends`]), and which of its bytes are typed keys and how
/// many characters are a paste's, for the count of what is unsent ([`Fed::segments`]): text
/// inside a paste is never keys, whatever write it is in.
#[derive(Debug)]
pub(super) struct PasteWatch {
    /// The characters of the open paste so far; none while no paste is open.
    open: Option<usize>,
    /// The end of the last write, when it could be the beginning of a marker.
    carried: Vec<u8>,
    /// When the pane last took a write of the human's.
    idle_since: Option<Instant>,
    /// How long it takes nothing before a paste left open is forgotten.
    pub(super) abandoned_after: Duration,
}

impl Default for PasteWatch {
    fn default() -> Self {
        Self {
            open: None,
            carried: Vec::new(),
            idle_since: None,
            abandoned_after: PASTE_ABANDONED_AFTER,
        }
    }
}

/// A paste cut off, its closing marker never sent (a page that went, a chunk it could not
/// send), is forgotten once the pane has taken nothing of the human's for this long: a chunk
/// of a paste follows the one before it within the time of a call, and otherwise what the
/// human types next would be taken for the rest of the paste, and an Enter among it would
/// send nothing.
const PASTE_ABANDONED_AFTER: Duration = Duration::from_secs(10);

/// A paste that ends in a write: where in it (just after the closing marker), and how many
/// characters the paste held, all of it, whatever writes it came in.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct PasteEnd {
    pub(super) at: usize,
    pub(super) characters: usize,
}

/// A piece of a write of the human's, in order.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Segment {
    /// Keys typed: these bytes of the write.
    Typed(Range<usize>),
    /// A paste, or part of one: the characters of it that came in this write, markers left
    /// out. None where only a marker came, which is the human's input all the same.
    Pasted(usize),
}

/// What a write of the human's holds, as [`PasteWatch::feed`] reads it against the paste the
/// writes before it left open.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Fed {
    pub(super) ends: Vec<PasteEnd>,
    pub(super) segments: Vec<Segment>,
}

impl Fed {
    fn typed(&mut self, from: usize, to: usize) {
        if from < to {
            self.segments.push(Segment::Typed(from..to));
        }
    }
}

impl PasteWatch {
    /// The pastes that end in `bytes`, the human's next write, in order, and what it holds
    /// of keys and of paste. A paste that began in an earlier write is counted from its
    /// opening marker; one still open at the end of `bytes` goes on in the next write; a
    /// marker split by the end of a write is found.
    pub(super) fn feed(&mut self, bytes: &[u8], now: Instant) -> Fed {
        let cut_off = self
            .idle_since
            .is_some_and(|since| now.saturating_duration_since(since) > self.abandoned_after);
        if cut_off {
            self.open = None;
            self.carried.clear();
        }
        self.idle_since = Some(now);
        let carried = self.carried.len();
        let stream = if carried == 0 {
            Cow::Borrowed(bytes)
        } else {
            Cow::Owned([&self.carried[..], bytes].concat())
        };
        self.carried.clear();
        // Where a place in the stream is in `bytes`: what was carried was in the write before.
        let in_write = |index: usize| index.saturating_sub(carried);
        let mut fed = Fed::default();
        let mut at = 0;
        loop {
            let rest = &stream[at..];
            match self.open {
                None => {
                    let Some(start) = position_of(rest, PASTE_START) else {
                        self.carry(rest, &[PASTE_START]);
                        fed.typed(in_write(at), bytes.len());
                        return fed;
                    };
                    fed.typed(in_write(at), in_write(at + start));
                    fed.segments.push(Segment::Pasted(0));
                    self.open = Some(0);
                    at += start + PASTE_START.len();
                }
                Some(characters) => {
                    let closing = position_of(rest, PASTE_END);
                    let opening = position_of(rest, PASTE_START);
                    if let Some(end) =
                        closing.filter(|end| opening.is_none_or(|start| *end < start))
                    {
                        let content = characters_in(&rest[..end]);
                        at += end + PASTE_END.len();
                        fed.segments.push(Segment::Pasted(content));
                        fed.ends.push(PasteEnd {
                            at: in_write(at),
                            characters: characters + content,
                        });
                        self.open = None;
                    } else if let Some(start) = opening {
                        // An opening marker in a paste starts it again, as a terminal that was
                        // cut off and began another would.
                        fed.segments
                            .push(Segment::Pasted(characters_in(&rest[..start])));
                        self.open = Some(0);
                        at += start + PASTE_START.len();
                    } else {
                        let held = self.carry(rest, &[PASTE_END, PASTE_START]);
                        let content = characters_in(&rest[..rest.len() - held]);
                        fed.segments.push(Segment::Pasted(content));
                        self.open = Some(characters + content);
                        return fed;
                    }
                }
            }
        }
    }

    /// The pane has taken the human's last write now, whatever came of it: a paste cut off
    /// is forgotten once it has taken nothing for long.
    pub(super) fn idle(&mut self, now: Instant) {
        self.idle_since = Some(now);
    }

    /// Keeps the end of `rest` that could begin one of `markers`, as many bytes as it can, and
    /// says how many it kept.
    fn carry(&mut self, rest: &[u8], markers: &[&[u8]]) -> usize {
        let held = (1..PASTE_END.len())
            .rev()
            .find(|held| {
                rest.len() >= *held
                    && markers
                        .iter()
                        .any(|marker| marker.starts_with(&rest[rest.len() - held..]))
            })
            .unwrap_or(0);
        self.carried = rest[rest.len() - held..].to_vec();
        held
    }
}

/// Where `marker` first is in `bytes`.
fn position_of(bytes: &[u8], marker: &[u8]) -> Option<usize> {
    bytes
        .windows(marker.len())
        .position(|window| window == marker)
}

/// `bytes`, a write of the human's path, in parts that each end where a paste ends (`ends`,
/// from [`PasteWatch::feed`]), each with the characters of the paste it ends; and what follows
/// the last, if anything does. One part, `bytes` itself, where it ends no paste (or is empty).
pub(super) fn parts_ending_pastes<'a>(
    bytes: &'a [u8],
    ends: &[PasteEnd],
) -> Vec<(&'a [u8], Option<usize>)> {
    let mut parts = Vec::new();
    let mut from = 0;
    for end in ends {
        parts.push((&bytes[from..end.at], Some(end.characters)));
        from = end.at;
    }
    if from < bytes.len() || parts.is_empty() {
        parts.push((&bytes[from..], None));
    }
    parts
}
