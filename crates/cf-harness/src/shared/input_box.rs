//! What a harness's input box holds, as its screen shows it. The pane host counts
//! the human's keys for what they leave unsent and holds a paste while any are
//! (`unsent`), but keys are a guess: those typed into a harness's dialog or
//! picker and left there with Escape count, though the box is empty, and every
//! delivery to the window waited for words that were never there (T-288). A
//! harness whose box its adapter can read says whether it is empty; the count is
//! the guess only where it cannot. Escape does not clear the count: every harness
//! keeps words in its box at one Escape, and only Claude Code clears them at two
//! (`cargo xtask live escape-unsent`, 2026-10-23).

use cf_proto::agents::Harness;
use serde_json::Value;

use crate::contract::{Pane, PaneHost};

/// What an input box on the screen holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputBox {
    /// Nothing is typed in it.
    Empty,
    /// Something is, or something that may be.
    Holds,
}

/// Where the cursor is on a screen's last lines as drawn (the pane host's
/// `cursor`): the line, by its place among them, how many characters of it are
/// before the cursor, whether it is shown, and how many characters of the line
/// from the cursor on are drawn plain, not styled: a harness draws the hint in an
/// empty box in a colour or faint, and words typed there plain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cursor {
    pub line: usize,
    pub col: usize,
    pub shown: bool,
    pub plain_after: usize,
}

impl Cursor {
    /// The cursor a snapshot's `cursor` says, where it says one.
    pub(crate) fn of(value: Option<&Value>) -> Option<Self> {
        let value = value?;
        let number = |name: &str| {
            value
                .get(name)
                .and_then(Value::as_u64)
                .and_then(|number| usize::try_from(number).ok())
        };
        Some(Self {
            line: number("line")?,
            col: number("col")?,
            shown: value.get("shown").and_then(Value::as_bool)?,
            plain_after: number("plainAfter")?,
        })
    }
}

/// What the input box of a `harness` window holds, where `screen` (its last lines
/// as drawn, the pane host's `bottom`) shows it, the cursor where `cursor` says;
/// none where it shows no box the harness's reader knows, or the harness has no
/// reader (Codex and Pi take their messages through their own channels,
/// which no typing holds back).
pub(crate) fn read(
    harness: Harness,
    screen: &[String],
    cursor: Option<Cursor>,
) -> Option<InputBox> {
    match harness {
        Harness::Claude => crate::claude::input_box::read(screen, cursor),
        Harness::Devin => crate::devin::input_box::read(screen, cursor),
        Harness::Muse => crate::muse::read_input_box(screen, cursor),
        Harness::Codex | Harness::Pi => None,
    }
}

/// What the input box of a `harness` window shows on `screen`, the cursor where
/// `cursor` says, for the human told of a delivery held for it: empty where its reader
/// reads it so (a hint is no words), its words where it reads them, none where it reads
/// no box.
pub(crate) fn shown(harness: Harness, screen: &[String], cursor: Option<Cursor>) -> Option<String> {
    let mark = match harness {
        Harness::Claude => crate::claude::input_box::MARK,
        Harness::Devin => crate::devin::input_box::MARK,
        // Muse's rules are drawn shorter than Claude's and Devin's: its own reader.
        Harness::Muse => return crate::muse::input_box_words(screen, cursor),
        Harness::Codex | Harness::Pi => return None,
    };
    match read(harness, screen, cursor)? {
        InputBox::Empty => Some(String::new()),
        InputBox::Holds => boxed(screen, mark).map(|boxed| boxed.words),
    }
}

/// [`shown`], of a `harness` window's screen as drawn now, for the human told of a delivery held
/// for what they typed. It decides nothing.
pub(crate) async fn words_on_screen(
    host: &dyn PaneHost,
    pane: &Pane,
    harness: Harness,
) -> Option<String> {
    let snapshot = super::pane::snapshot_drawn(host, pane).await.ok()?;
    shown(
        harness,
        &bottom(&snapshot)?,
        Cursor::of(snapshot.get("cursor")),
    )
}

/// Whether `line` is a rule drawn across the screen: `─`, with a label in it at
/// most (Devin's says its permission mode), beginning and ending with the rule.
pub(crate) fn is_rule(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("────") && line.ends_with('─')
}

/// The last input box on `screen` whose first line begins with the prompt mark
/// `mark`, between a rule above it and the next rule below.
pub(crate) struct Boxed<'a> {
    /// Its first line's place on the screen.
    pub(crate) line: usize,
    /// How many characters of that line come before the text after the mark: where
    /// the cursor is at a box with nothing typed in it.
    pub(crate) start: usize,
    /// Its first line after the mark.
    pub(crate) typed: &'a str,
    /// Whether any line after that holds anything.
    pub(crate) more: bool,
    /// Its lines' text after the mark, a line break between two.
    pub(crate) words: String,
}

impl Boxed<'_> {
    /// Whether `cursor` says the box holds only a hint: it is at the box's start,
    /// right after the mark and its space, and nothing from it on is drawn plain. Words
    /// typed and the cursor moved back to their start (Home) are drawn plain.
    pub(crate) fn only_a_hint(&self, cursor: Option<Cursor>) -> bool {
        !self.more
            && cursor.is_some_and(|cursor| {
                cursor.line == self.line && cursor.col == self.start && cursor.plain_after == 0
            })
    }
}

/// [`Boxed`], where `screen` shows one: none where it shows no such box, for a
/// picker's mark on a choice has no rule right above it, and a box drawn halfway
/// has no rule below it yet.
pub(crate) fn boxed(screen: &[String], mark: char) -> Option<Boxed<'_>> {
    let first = screen
        .iter()
        .rposition(|line| line.trim_start().starts_with(mark))?;
    if first == 0 || !is_rule(&screen[first - 1]) {
        return None;
    }
    let end = first + screen[first..].iter().position(|line| is_rule(line))?;
    let line = &screen[first];
    let indent = line
        .chars()
        .take_while(|character| character.is_whitespace())
        .count();
    let typed = line.trim_start().trim_start_matches(mark).trim();
    let more = screen[first + 1..end]
        .iter()
        .any(|line| !line.trim().is_empty());
    let words = std::iter::once(typed)
        .chain(screen[first + 1..end].iter().map(|line| line.trim()))
        .collect::<Vec<_>>()
        .join("\n");
    Some(Boxed {
        line: first,
        start: indent + 2,
        typed,
        more,
        words: words.trim_end().to_owned(),
    })
}

/// How long the person's last key must be past before a paste is licensed: a key typed a
/// moment before the look may not be drawn yet. A hold only: no time licenses a paste over the
/// count (`shared::look`, evidence).
pub(crate) const SETTLED_MS: u64 = 1_000;

/// The screen's last lines as drawn that a snapshot holds (`bottom`).
fn bottom(snapshot: &Value) -> Option<Vec<String>> {
    snapshot
        .get("bottom")
        .and_then(Value::as_array)
        .map(|lines| {
            lines
                .iter()
                .filter_map(|line| line.as_str().map(str::to_owned))
                .collect()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::look::{BoxReading, Look, NoLook};

    /// A recorded snapshot as a host that names what it shows would answer it.
    fn recorded(path: &std::path::Path) -> Value {
        let mut snapshot: Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        snapshot["seen"] = Value::from("0123456789abcdef");
        // Recorded before the host asked the terminal: current, as these were.
        snapshot["outputCaughtUp"] = Value::from(true);
        snapshot
    }

    /// Screens recorded live (`cargo xtask live held-typed`), each the pane host's snapshot an
    /// adapter decided on: a box Claude Code drew empty, or with only its own words in it (its
    /// prompt suggestion; its recap above the box), the human's keys counted as unsent with
    /// nothing of them left (T-839). Each is read empty, and the human told of a box that shows
    /// nothing. The recording holds no screen kept at a key, so the count holds until Ctrl+U
    /// (N17).
    #[test]
    fn recorded_screens_of_a_box_with_no_words_of_the_humans_read_empty() {
        let folder =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/screens");
        let mut read = 0;
        for entry in std::fs::read_dir(&folder).expect("the recorded screens") {
            let path = entry.expect("a screen").path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let harness = match name.split('-').next() {
                Some("claude") => Harness::Claude,
                Some("devin") => Harness::Devin,
                _ => continue,
            };
            let snapshot = recorded(&path);
            let look = Look::of(&snapshot).expect("a look");
            assert_eq!(look.reads(harness), BoxReading::Empty, "{name}");
            assert_eq!(look.evidence(harness), None, "{name}");
            let lines = bottom(&snapshot).unwrap();
            assert_eq!(
                shown(harness, &lines, Cursor::of(snapshot.get("cursor"))).as_deref(),
                Some(""),
                "{name}"
            );
            read += 1;
        }
        assert!(read > 0, "no screen was read");
    }

    /// A look is current only when the screen shows every byte the host has read (N17,
    /// T-2132): a window that printed the cursor move of a key's echo in one read and the key
    /// in the next, the second not yet drawn, shows an empty box that is not its box. Where the
    /// counts differ there is no look; with them equal the box is read as before. A snapshot
    /// with no `seen` is no look either.
    #[test]
    fn a_snapshot_whose_screen_lacks_bytes_the_host_has_read_is_no_look() {
        let screens = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/screens/claude-2.1.312-keys-cleared-at-rest.json");
        let mut snapshot = recorded(&screens);
        snapshot["typedQuietMs"] = Value::from(1_200);
        snapshot["outputQuietMs"] = Value::from(0);
        snapshot["screenQuietMs"] = Value::from(1_500);
        assert!(Look::of(&snapshot).is_ok());
        snapshot["outputRead"] = Value::from(262);
        snapshot["outputDrawn"] = Value::from(261);
        assert_eq!(Look::of(&snapshot).unwrap_err(), NoLook::Behind);
        snapshot["outputDrawn"] = Value::from(262);
        assert!(Look::of(&snapshot).is_ok());
        snapshot.as_object_mut().unwrap().remove("seen");
        assert_eq!(Look::of(&snapshot).unwrap_err(), NoLook::Old);
    }
}
