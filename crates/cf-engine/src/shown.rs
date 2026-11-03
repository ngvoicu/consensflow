//! What a window last showed, for the note that says why it did not come up,
//! and for the pause note of a task whose window closed under it. A window that
//! fails says why on its own screen (Pi without a login: "No API key found for
//! the selected model … Use /login"; a harness that crashes, its last words),
//! and the screen is gone with the window. The pane host keeps it, and how the
//! program ended, and tells them with `pane.exit` or to a `pane.snapshot` that
//! asks (`cf_proto::panes`); [`Shown`] is what the engine makes of that.
//!
//! The words are held to a few lines and a few hundred characters, and what
//! looks like a key or a token in them is hidden ([`mask`]): a screen can echo
//! one, and the note goes to the board, the chief's window and the log.
//!
//! **A line that names a secret is not quoted, and neither are the lines beside it.** A
//! credential is no token by its shape (`password: correct horse battery staple`, a value
//! in quotes that goes on over the next line, YAML's `'it''s'`), and no masker of the
//! syntaxes it is written in holds (both reviews of 2026-10-16 found one gap after another).
//! So a line that names a secret-like word ([`secret`]: a password, `pwd`, an API or private
//! key or a credential anywhere in a name, whatever its case or split, such as `PassWord` or
//! `MYSQL_PWD`; a secret, a token, `auth` or `bearer` as a word or a part of a name such as
//! `DB_SECRET`, never a piece of a word such as `author`) is left out of the quote with the
//! rows next to it (a long value wraps onto the next row; a name can end the row above), and
//! the rest of the lines the quote would show are quoted through the masker, the note saying
//! how many were left out. What a value may do over more rows is not split: a quote it opens
//! and does not close, a `\` that continues the row, a block it opens, and a private key's
//! block (its base64 rows name nothing) take the lines to where they end, or the end of the
//! screen ([`without_secrets`]). Where every line the quote would show is left out, the note
//! says that its last screen lines named a secret, so they are left out; how the program
//! ended is said all the same. Every line the host sent is looked at, not only those the
//! quote would keep: a value that goes on over a line keeps no name on the line it ends on.
//! The screens that name one and hold nothing else are the cost: Pi without a login says "No
//! API key found for the selected model", and its note says its lines were left out, with the
//! exit code (T-823: until then one line that named a secret left the whole screen out, and a
//! chief whose worker closed with exit 143 never learned why).
//!
//! **How the program ended** is said as the host said it: its signal where it sent one, and a
//! status above 128 as the signal it is (143 is SIGTERM, 137 SIGKILL, [`signalled`]). A close
//! ConsensFlow had no part in, none of its own under way, says that the signal did not come from
//! it ([`Shown::outside`]). **What the window was doing** is the last tool call its record
//! shows, said before its screen, masked and cut short ([`Shown::with_last_call`]).
//!
//! **A program the host could not end** (`survivor`) is said in place of how it
//! ended: it may still run ([`Shown::outlived`]). Nothing waits for it: a window
//! that opens on its conversation opens beside it, which the words warn of.

pub(crate) mod mask;
mod ours;
mod secret;
#[cfg(test)]
mod tests;

use std::sync::LazyLock;

use cf_ledger::MAY_STILL_RUN;
use cf_proto::panes::{BackgroundProcess, PaneExit, PossiblyLeft, SnapshotTail};
use regex::Regex;
use serde_json::Value;

use mask::mask;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use ours::ours;
pub(crate) use ours::Paste;
use ours::{owners, Owner};
use secret::names_a_secret;

/// The most lines of a screen a note quotes: the last ones.
const QUOTE_LINES: usize = 6;
/// The most characters a quote takes, between its quotation marks; the older
/// lines go first, and a last line longer than this is cut.
const QUOTE_CHARS: usize = 400;
/// What comes between the lines of a quote, which a note holds on one line.
const BETWEEN: &str = " / ";
/// The first exit code that is a Windows status (an `NTSTATUS` of a warning or
/// an error) rather than a number the program returned.
const STATUS_CODES: u32 = 0x8000_0000;

/// What the pane host said of a window: how its program ended where it had,
/// the last lines its screen showed, and what it left running when the window
/// closed. A host that did not say is not a screen that showed nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shown {
    code: Option<u32>,
    signal: Option<String>,
    tail: Option<Vec<String>>,
    background: Vec<BackgroundProcess>,
    /// What the host named as possibly the window's, which it did not end.
    possibly_left: Vec<PossiblyLeft>,
    /// The window's program, where the host could not prove it gone (0 where its
    /// number was not known): it may still run.
    survivor: Option<u32>,
    /// Lines of the screen were ConsensFlow's own and are left out ([`Shown::without_pastes`]).
    pasted: bool,
    /// ConsensFlow had no stop or close of its own under way when the window ended
    /// ([`Shown::outside`]): a signal that ended it was not ConsensFlow's.
    outside: bool,
    /// The last tool call the window's record shows ([`Shown::with_last_call`]).
    last_call: Option<String>,
}

impl Shown {
    /// A pane's end, as the host sent it.
    pub fn from_exit(exit: PaneExit) -> Self {
        Self {
            code: exit.exit_code,
            signal: exit.signal,
            tail: exit.tail,
            background: exit.background,
            possibly_left: exit.possibly_left,
            survivor: exit.survivor,
            pasted: false,
            outside: false,
            last_call: None,
        }
    }

    /// What a note says of a window's program the host could not end, after the word that
    /// its window closed: the program may still run, in the ledger's words of it, by which
    /// a pause note that goes leaves the warning behind ([`MAY_STILL_RUN`]). None where it
    /// has gone.
    pub(crate) fn outlived(&self) -> Option<String> {
        let program = match self.survivor? {
            0 => "its program".to_owned(),
            pid => format!("its program (process {pid})"),
        };
        Some(format!(", but {program} {MAY_STILL_RUN}"))
    }

    /// What the window left running when it closed, which its close ended: the
    /// processes its turns started, as the host listed them.
    pub(crate) fn left_running(&self) -> &[BackgroundProcess] {
        &self.background
    }

    /// What the host named, as the window closed, as possibly the window's and not ended:
    /// it runs on, and nothing proved it the window's (`PaneExit::possibly_left`).
    pub(crate) fn possibly_left(&self) -> &[PossiblyLeft] {
        &self.possibly_left
    }

    /// A pane's snapshot, asked for its tail: the lines it answered. An answer
    /// that was a refusal, or that gave no tail, says nothing.
    pub fn from_snapshot(answer: &Value) -> Self {
        if answer.get("ok") != Some(&Value::Bool(true)) {
            return Self::default();
        }
        let tail = serde_json::from_value::<SnapshotTail>(answer.clone())
            .ok()
            .and_then(|answer| answer.tail);
        Self {
            tail,
            ..Self::default()
        }
    }

    /// The window's screen without what ConsensFlow itself typed into it, of
    /// the `pastes` it typed ([`owners`]): those lines are no reason of the window's.
    /// Where the newest paste's run is in view, only the window's lines after it
    /// are kept: they are what the window printed since the message its failed
    /// turn was on.
    ///
    /// Known limits (Calliope's re-check T-305):
    /// - P3-3: a newest paste the window shows as a placeholder has no run, so the
    ///   whole tail is read, older errors and all (no harness showed one after
    ///   sending, T-298);
    /// - P3-5: a window line that begins with the newest paste's header (a model
    ///   echoing it at a line's start) becomes the cut, and hides what is above it;
    /// - P3-1, filed as its own task: a paste still on its way (the harness queued
    ///   it in the failing turn) is the newest and becomes the cut; the cut is to be
    ///   bounded by the newest paste the window was given;
    /// - P3-2, filed as its own task: the pastes are ordered by id, which is not the
    ///   order a member's window took them in (a task's brief waits while the window
    ///   holds a task, and a later note goes first).
    pub(crate) fn without_pastes(mut self, pastes: &[Paste]) -> Self {
        if let Some(tail) = self.tail.as_mut() {
            let owners = owners(tail, pastes);
            // What the window printed since the newest paste, where its run is in
            // view: older pastes and the failures told of before it are not this
            // turn's (Calliope's re-check T-300; the chief's decision, T-304).
            let since = owners.iter().rposition(|owner| *owner == Owner::Paste(0));
            let before = tail.len();
            let mut at = 0;
            tail.retain(|_| {
                let kept = owners[at] == Owner::Window && since.is_none_or(|end| at > end);
                at += 1;
                kept
            });
            self.pasted = tail.len() < before;
        }
        self
    }

    /// What the screen says of a failure, to tell one outage from the next
    /// ([`outage_key`]): its last lines with a word in them.
    pub(crate) fn outage_key(&self) -> String {
        self.tail
            .as_deref()
            .map_or_else(String::new, |tail| outage_key(tail, Keep::Last))
    }

    /// The last lines of the screen the host sent: none where it sent none.
    pub(crate) fn lines(&self) -> &[String] {
        self.tail.as_deref().unwrap_or_default()
    }

    /// Whether the host said anything of the window.
    /// Whether the window's program is gone, as far as the host said: yes where
    /// it said how the window ended and named no program it could not end, no
    /// where it named one, none where it said nothing (the engine's own close
    /// settles its window before the host's word comes).
    pub(crate) fn gone(&self) -> Option<bool> {
        self.is_known().then_some(self.survivor.is_none())
    }

    pub fn is_known(&self) -> bool {
        self.code.is_some()
            || self.signal.is_some()
            || self.tail.is_some()
            || self.survivor.is_some()
    }

    /// `because`, and after it what the window showed and how it ended, as far
    /// as the host said: `the window closed (exit code 127); its screen ended
    /// with: "codex: command not found"`. A screen with nothing on it says that,
    /// and one that names a secret that its lines were left out. What was hidden
    /// in the quote is counted after it.
    pub fn after(&self, because: &str) -> String {
        self.after_with(because, "")
    }

    /// [`Shown::after`], with `besides` said between how the program ended and
    /// what its screen showed: `@zeus's window closed (exit code 3) with
    /// background work running, which its close ended: sleep 9; its screen ended with: "…"`.
    pub(crate) fn after_with(&self, because: &str, besides: &str) -> String {
        let mut said = because.to_owned();
        if let Some(outlived) = self.outlived() {
            said.push_str(&outlived);
        } else if let Some(ended) = self.ended() {
            said.push_str(&format!(" ({ended})"));
        }
        said.push_str(besides);
        if let Some(call) = &self.last_call {
            said.push_str(&last_call_words(call));
        }
        let Some(tail) = &self.tail else {
            return said;
        };
        let (kept, left_out) = without_secrets(tail);
        match Quote::of(&kept, Keep::Last) {
            None if left_out > 0 => {
                said.push_str("; its last screen lines named a secret, so they are left out");
            }
            None if self.pasted => said.push_str("; its screen showed nothing of its own"),
            None => said.push_str("; its screen was empty"),
            Some(quote) => {
                said.push_str(&quote.after("; its screen ended with"));
                said.push_str(&left_out_words(left_out));
            }
        }
        said
    }

    /// This exit as one ConsensFlow had no part in: it had no stop or close of its own under
    /// way, so a signal that ended the program was not its own.
    pub(crate) fn outside(&self) -> Self {
        Self {
            outside: true,
            ..self.clone()
        }
    }

    /// This exit with the last tool call the window's record shows, as a line
    /// (`Record::last_call`): what the window was doing when it closed, which the note says
    /// before its screen, masked and cut short. None where the record shows none.
    pub(crate) fn with_last_call(mut self, call: Option<String>) -> Self {
        self.last_call = call;
        self
    }

    /// How the program ended, in words. A Windows program that crashed, or was
    /// ended by its console's Ctrl+C, ends with the status that says why
    /// (`0xC0000005`, an access violation; `0xC000013A`, the Ctrl+C), which is
    /// written as Windows writes it: in decimal it is ten digits nobody knows.
    ///
    /// A status above 128 is a shell's word for a program a signal ended (143 is SIGTERM,
    /// 137 SIGKILL), and the note says which. Where ConsensFlow had no stop or close of its
    /// own under way ([`Shown::outside`]), it says that too: the signal came from outside it
    /// (the project's own tests killed a Claude Code worker's `cargo test` and then the
    /// worker, and the note said "exit code 143", T-823).
    fn ended(&self) -> Option<String> {
        let (said, signal) = match (&self.signal, self.code) {
            (Some(signal), _) => (format!("ended by signal {signal}"), true),
            (None, Some(code)) if code >= STATUS_CODES => {
                (format!("exit code {code:#010X}"), false)
            }
            (None, Some(code)) => match signalled(code, cfg!(unix)) {
                Some(name) => (
                    format!("exit code {code}, the status of a program ended by {name}"),
                    true,
                ),
                None => (format!("exit code {code}"), false),
            },
            (None, None) => return None,
        };
        Some(if signal && self.outside {
            format!("{said}, which ConsensFlow did not send: it had no stop or close of its own under way")
        } else {
            said
        })
    }
}

/// `because`, and after it what a harness said of it in its own record (an
/// API's error, a provider's message): `@zeus's harness reported a failure; it
/// said: "Selected model is at capacity."`. Its words are held to a quote's
/// bounds, from their first line, which is an error's headline: they go to the
/// board, the chief's window and the log. What looks like a key, a token or an
/// email address is masked in them, and they are quoted whatever secret they
/// name: an API's or an SDK's error names the key it lacks ("No API key found")
/// and never holds one in prose, and the login class is the failure whose fix
/// the note must say (the chief's decision of 2026-10-21, T-135). A screen can
/// echo anything, and keeps the stricter rule ([`Shown::after_with`]). None where
/// no line of the words has a letter or a digit.
pub(crate) fn after_words(because: &str, words: &str) -> Option<String> {
    let lines: Vec<String> = words.lines().map(str::to_owned).collect();
    Quote::of(&lines, Keep::First).map(|quote| format!("{because}{}", quote.after("; it said")))
}

/// What a harness's `words` say of a failure, to tell one outage from the next
/// ([`outage_key`]): their first lines with a word in them.
pub(crate) fn words_key(words: &str) -> String {
    let lines: Vec<String> = words.lines().map(str::to_owned).collect();
    outage_key(&lines, Keep::First)
}

/// The lines a quote of `lines` would keep, their blanks made one and what
/// changes by itself within one outage folded ([`folded`]).
fn outage_key(lines: &[String], keep: Keep) -> String {
    let lines: Vec<String> = lines
        .iter()
        .filter(|line| line.chars().any(char::is_alphanumeric))
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    let chosen: Vec<&str> = match keep {
        Keep::Last => lines
            .iter()
            .rev()
            .take(QUOTE_LINES)
            .rev()
            .map(String::as_str)
            .collect(),
        Keep::First => lines.iter().take(QUOTE_LINES).map(String::as_str).collect(),
    };
    folded(&chosen.join("\n"))
}

/// What changes by itself from one turn of an outage to the next, each with
/// what it is folded to: a UUID, an id-like token (a run of 16 letters and
/// digits or more with no separator, a digit among them: a request's id, a
/// completion's), a clock time, and a duration (a number joined to a unit of
/// time: a retry-after, a countdown). Every other number is the failure's own:
/// `gpt-5` is not `gpt-6`, a 500 is not a 502, and a model's id, its parts
/// joined by `-` (`claude-sonnet-5-5`, a dated one, `gpt-5-codex-high`), or a
/// code's joined by `_` (`ERR_HTTP2_GOAWAY_SESSION`) is no id (the chief's
/// decisions of 2026-10-22, T-286 and T-298).
///
/// A footer's counters, which a harness redraws each turn, fold too: a share
/// (`12%` of a context left), an amount of money (`$0.03`), and a count written
/// short or with thousands (`1.2k`, `12,345` tokens), but no count that ends a
/// model's id (`gpt-4-32k`, `o3-mini-2k`) and no list of numbers (`500,502,503`):
/// a thousands count only before a count's unit, a short one only where no `-`
/// joins it to what comes before. Claude Code, Codex and Pi
/// each draw some of them below the conversation; the probe of T-298
/// recorded counts of the brief's lines only, so which line holds which is not
/// shown there.
///
/// Known limit (Calliope's re-check T-300, P3-R6): a UUID joined to a word by
/// `_` (`session_123e4567-…`) is not folded, as no word boundary holds between
/// `_` and its first digit.
static CHANGING: LazyLock<[(Regex, &str); 7]> = LazyLock::new(|| {
    [
        (
            pattern(
                r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b",
            ),
            "#id",
        ),
        (pattern(r"[A-Za-z0-9]{16,}"), "#id"),
        (pattern(r"\$\d+(?:\.\d+)?"), "#money"),
        (pattern(r"\b\d+(?:\.\d+)?%"), "#share"),
        (
            pattern(
                r"\b(\d{1,3}(?:,\d{3})+)\s+(?:tokens?|chars?|characters|words?|lines?|bytes?)\b|(?:^|[^\w-])(\d+(?:\.\d+)?[kKM])\b",
            ),
            "#count",
        ),
        (pattern(r"\b\d{1,2}:\d{2}(?::\d{2})?\b"), "#time"),
        (
            pattern(
                r"(?i)\b\d+(?:\.\d+)?\s?(?:ms|milliseconds?|s|secs?|seconds?|m|mins?|minutes?|h|hrs?|hours?)\b",
            ),
            "#duration",
        ),
    ]
});

/// The patterns are constants of this file, and a test folds with each.
#[allow(clippy::expect_used)]
fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("a pattern of the outage key")
}

/// `text` with what changes by itself within one outage folded.
fn folded(text: &str) -> String {
    let mut text = text.to_owned();
    for (at, (changing, to)) in CHANGING.iter().enumerate() {
        text = changing
            .replace_all(&text, |found: &regex::Captures<'_>| {
                let whole = found.get(0).map_or("", |whole| whole.as_str());
                // An id is one only with a digit in it: a long word is a word.
                if at == 1 && !whole.chars().any(|c| c.is_ascii_digit()) {
                    return whole.to_owned();
                }
                // A pattern that names what changes in a group of its own folds that
                // alone, and keeps what it was told apart by.
                let start = found.get(0).map_or(0, |whole| whole.start());
                match (1..found.len()).find_map(|group| found.get(group)) {
                    Some(part) => format!(
                        "{}{to}{}",
                        &whole[..part.start() - start],
                        &whole[part.end() - start..]
                    ),
                    None => (*to).to_owned(),
                }
            })
            .into_owned();
    }
    text
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        "string"
    } else {
        "strings"
    }
}

/// The words of a screen's last lines, or a harness's first, held to the
/// quote's bounds.
struct Quote {
    text: String,
    /// How many secrets were hidden in the lines that were kept.
    masked: usize,
}

impl Quote {
    /// The quote after `label`, and how many strings it hid.
    fn after(&self, label: &str) -> String {
        let mut said = format!("{label}: \"{}\"", self.text);
        if self.masked > 0 {
            let (count, strings) = (self.masked, plural(self.masked));
            said.push_str(&format!(" ({count} key- or token-like {strings} masked)"));
        }
        said
    }

    /// The quote of `lines`: none where no line has a word in it. A line of no
    /// letter or digit is a screen's chrome (a box's edge, a footer's rule) and
    /// is no line of it. The lines `keep` says come first, the last of a screen
    /// or the first of a harness's words: each has its blanks made one, its
    /// secrets hidden and its quotation marks made apostrophes (a quote is not to
    /// end where the text says it does), and they are kept while they fit.
    fn of(lines: &[String], keep: Keep) -> Option<Self> {
        let lines: Vec<String> = lines
            .iter()
            .filter(|line| line.chars().any(char::is_alphanumeric))
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect();
        let chosen: Vec<&String> = match keep {
            Keep::Last => lines.iter().rev().take(QUOTE_LINES).collect(),
            Keep::First => lines.iter().take(QUOTE_LINES).collect(),
        };
        let mut kept: Vec<String> = Vec::new();
        let (mut masked, mut used) = (0, 0);
        for line in chosen {
            let hidden = mask(line);
            let text = hidden.text.replace('"', "'");
            let between = if kept.is_empty() { 0 } else { BETWEEN.len() };
            let length = text.chars().count();
            if used + between + length > QUOTE_CHARS {
                // Only the line kept first may be cut; one after it that does not fit is left out.
                if kept.is_empty() {
                    kept.push(cut(&text, QUOTE_CHARS));
                    masked += hidden.count;
                }
                break;
            }
            used += between + length;
            masked += hidden.count;
            kept.push(text);
        }
        if keep == Keep::Last {
            kept.reverse();
        }
        (!kept.is_empty()).then(|| Self {
            text: kept.join(BETWEEN),
            masked,
        })
    }
}

/// Which lines a quote keeps when not all fit: a screen's last, where it said
/// what it said last; a harness's first, where an error says what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keep {
    First,
    Last,
}

/// The first characters of `text` that, with the mark that says it was cut,
/// make `limit`.
fn cut(text: &str, limit: usize) -> String {
    let kept: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// `text`, cut to `limit` characters with the mark that says so where it is longer.
fn cut_to(text: &str, limit: usize) -> String {
    if text.chars().count() > limit {
        cut(text, limit)
    } else {
        text.to_owned()
    }
}

/// The most characters the line of a tool call takes in a note, between its quotation marks.
const CALL_CHARS: usize = 200;

/// What a note says of the last tool call a window's record shows: the line, masked as a
/// screen's lines are, cut short, and its quotation marks made apostrophes; or that it named
/// a secret and is left out, as a screen's line that does is.
fn last_call_words(call: &str) -> String {
    let one: String = call.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.is_empty() {
        return String::new();
    }
    if names_a_secret(&one) {
        return "; its last tool call named a secret, so it is left out".to_owned();
    }
    let hidden = mask(&one);
    let text = hidden.text.replace('"', "'");
    let quote = Quote {
        text: cut_to(&text, CALL_CHARS),
        masked: hidden.count,
    };
    quote.after("; its last tool call was")
}

/// What a note says of the `count` lines of a screen it left out of its quote.
fn left_out_words(count: usize) -> String {
    match count {
        0 => String::new(),
        1 => " (1 line that named or lay beside a secret was left out)".to_owned(),
        count => format!(" ({count} lines that named or lay beside a secret were left out)"),
    }
}

/// The rows of `tail` a quote of it may show, and how many of the rows it would have shown
/// were left out. A quote shows the last [`QUOTE_LINES`] rows with a word in them
/// ([`Quote::of`]); every row the host sent is looked at for what lies beside it, as a value
/// that goes on over a row keeps no name on the row it ends on.
///
/// A row that names a secret ([`names_a_secret`]) is left out with the rows next to it: its
/// value may go on over the next row, as the terminal wraps a long one, and its name may end
/// the row above. Where its value may go on over more rows than that, nothing says where it
/// ends, and the rest of the screen goes: a quote it opens and does not close, a `\` that
/// continues the row, a block it opens (`{`, `[`, `(`, `|`, `>`). A private key's block
/// goes whole, its base64 rows among it, which name nothing on their own: from its `BEGIN` row
/// to its `END`, or the end of the screen; a block whose first row is above the screen, from
/// the top to its `END`. (T-823: one row of a Claude Code worker's screen named a secret and
/// the whole screen was left out, so the chief never learned why its window closed.)
fn without_secrets(tail: &[String]) -> (Vec<String>, usize) {
    let mut out = vec![false; tail.len()];
    let mut block = false;
    for (at, line) in tail.iter().enumerate() {
        let upper = line.to_ascii_uppercase();
        let key = upper.contains("PRIVATE KEY");
        if key && upper.contains("BEGIN") {
            block = true;
        }
        if block {
            out[at] = true;
        }
        if key && (upper.contains("-----END") || upper.contains(" END ")) {
            if !block {
                // Its first row is above the screen.
                out[..=at].fill(true);
            }
            block = false;
        }
        if names_a_secret(line) {
            out[at.saturating_sub(1)..=(at + 1).min(tail.len() - 1)].fill(true);
            if runs_on(line) {
                out[at..].fill(true);
            }
        }
    }
    let shown: Vec<usize> = (0..tail.len())
        .filter(|&at| tail[at].chars().any(char::is_alphanumeric))
        .rev()
        .take(QUOTE_LINES)
        .collect();
    let left_out = shown.iter().filter(|&&at| out[at]).count();
    let mut kept: Vec<String> = shown
        .into_iter()
        .filter(|&at| !out[at])
        .map(|at| tail[at].clone())
        .collect();
    kept.reverse();
    (kept, left_out)
}

/// Whether the value of a secret named on `line` may go on over more rows than the next: a
/// quote it opens and does not close (an odd number of `"`, or of `'`, none escaped), a `\`
/// that continues the row, or a block it opens.
fn runs_on(line: &str) -> bool {
    let unescaped = |quote: char| {
        let mut count = 0;
        let mut escaped = false;
        for character in line.chars() {
            match character {
                '\\' if !escaped => {
                    escaped = true;
                    continue;
                }
                found if found == quote && !escaped => count += 1,
                _ => {}
            }
            escaped = false;
        }
        count
    };
    unescaped('"') % 2 == 1
        || unescaped('\'') % 2 == 1
        || line.trim_end().ends_with(['\\', '{', '[', '(', '|', '>'])
}

/// The signal a status above 128 says ended the program (a shell's `128 + signal`): its name
/// where every system the app runs on agrees on it, its number otherwise. None for a status
/// that is no signal's, and for every status on Windows, which has no signals: there it is
/// the number the program returned.
fn signalled(code: u32, unix: bool) -> Option<String> {
    if !unix || !(129..=159).contains(&code) {
        return None;
    }
    let number = code - 128;
    let name = match number {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        5 => "SIGTRAP",
        6 => "SIGABRT",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        13 => "SIGPIPE",
        14 => "SIGALRM",
        15 => "SIGTERM",
        _ => return Some(format!("signal {number}")),
    };
    Some(name.to_owned())
}
