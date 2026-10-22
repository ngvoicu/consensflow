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
//! **A screen that names a secret is not quoted at all.** A credential is no token
//! by its shape (`password: correct horse battery staple`, a value in quotes that
//! goes on over the next line, YAML's `'it''s'`), and no masker of the syntaxes it
//! is written in holds (both reviews of 2026-10-16 found one gap after another).
//! So where any of the lines the host sent of the screen's end names a secret-like
//! word ([`secret`]: a password, `pwd`, an API or private key or a credential anywhere
//! in a name, whatever its case or split, such as `PassWord` or `MYSQL_PWD`; a secret,
//! a token, `auth` or `bearer` as a word or a part of a name such as `DB_SECRET`, never
//! a piece of a word such as `author`), the note says that the lines were left out,
//! and quotes none of them; how the program ended is said all the same. Every line
//! the host sent is looked at, not only those the quote would keep: a value that
//! goes on over a line keeps no name on the line it ends on. The cost is the screens
//! that name one and hold none: Pi without a login says "No API key found for the
//! selected model", and its note now says its lines were left out, with the exit code;
//! so does a screen that shows the shell's `pwd`.
//!
//! **A program the host could not end** (`survivor`) is said in place of how it
//! ended: it may still run ([`Shown::outlived`]). Nothing waits for it: a window
//! that opens on its conversation opens beside it, which the words warn of.

pub(crate) mod mask;
mod secret;
#[cfg(test)]
mod tests;

use cf_proto::panes::{BackgroundProcess, PaneExit, SnapshotTail};
use serde_json::Value;

use mask::mask;
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
    /// The window's program, where the host could not prove it gone (0 where its
    /// number was not known): it may still run.
    survivor: Option<u32>,
}

impl Shown {
    /// A pane's end, as the host sent it.
    pub fn from_exit(exit: PaneExit) -> Self {
        Self {
            code: exit.exit_code,
            signal: exit.signal,
            tail: exit.tail,
            background: exit.background,
            survivor: exit.survivor,
        }
    }

    /// What a note says of a window's program the host could not end, after the word that
    /// its window closed: the program may still run. None where it has gone.
    pub(crate) fn outlived(&self) -> Option<String> {
        let program = match self.survivor? {
            0 => "its program".to_owned(),
            pid => format!("its program (process {pid})"),
        };
        Some(format!(
            ", but {program} could not be ended and may still run"
        ))
    }

    /// What the window left running when it closed, which its close ended: the
    /// processes its turns started, as the host listed them.
    pub(crate) fn left_running(&self) -> &[BackgroundProcess] {
        &self.background
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
    /// background work still running: sleep 9; its screen ended with: "…"`.
    pub(crate) fn after_with(&self, because: &str, besides: &str) -> String {
        let mut said = because.to_owned();
        if let Some(outlived) = self.outlived() {
            said.push_str(&outlived);
        } else if let Some(ended) = self.ended() {
            said.push_str(&format!(" ({ended})"));
        }
        said.push_str(besides);
        let Some(tail) = &self.tail else {
            return said;
        };
        if tail.iter().any(|line| names_a_secret(line)) {
            said.push_str("; its last screen lines named a secret, so they are left out");
            return said;
        }
        match Quote::of(tail, Keep::Last) {
            None => said.push_str("; its screen was empty"),
            Some(quote) => said.push_str(&quote.after("; its screen ended with")),
        }
        said
    }

    /// How the program ended, in words. A Windows program that crashed, or was
    /// ended by its console's Ctrl+C, ends with the status that says why
    /// (`0xC0000005`, an access violation; `0xC000013A`, the Ctrl+C), which is
    /// written as Windows writes it: in decimal it is ten digits nobody knows.
    fn ended(&self) -> Option<String> {
        match (&self.signal, self.code) {
            (Some(signal), _) => Some(format!("ended by signal {signal}")),
            (None, Some(code)) if code >= STATUS_CODES => Some(format!("exit code {code:#010X}")),
            (None, Some(code)) => Some(format!("exit code {code}")),
            (None, None) => None,
        }
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
