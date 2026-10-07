//! What a window last showed, for the note that says why it did not come up.
//! A window that fails says why on its own screen (Pi without a login: "No
//! API key found for the selected model … Use /login"), and the screen is
//! gone with the window. The pane host keeps it, and how the program ended,
//! and tells them with `pane.exit` or to a `pane.snapshot` that asks
//! (`cf_proto::panes`); [`Shown`] is what the engine makes of that.
//!
//! The words are held to a few lines and a few hundred characters, and what
//! looks like a key or a token in them is hidden ([`mask`]): a screen can echo
//! one, and the note goes to the board, the chief's window and the log.

mod mask;
#[cfg(test)]
mod tests;

use cf_proto::panes::{PaneExit, SnapshotTail};
use serde_json::Value;

use mask::mask;

/// The most lines of a screen a note quotes: the last ones.
const QUOTE_LINES: usize = 6;
/// The most characters a quote takes, between its quotation marks; the older
/// lines go first, and a last line longer than this is cut.
const QUOTE_CHARS: usize = 400;
/// What comes between the lines of a quote, which a note holds on one line.
const BETWEEN: &str = " / ";

/// What the pane host said of a window: how its program ended where it had,
/// and the last lines its screen showed. A host that did not say is not a
/// screen that showed nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shown {
    code: Option<u32>,
    signal: Option<String>,
    tail: Option<Vec<String>>,
}

impl Shown {
    /// A pane's end, as the host sent it.
    pub fn from_exit(exit: PaneExit) -> Self {
        Self {
            code: exit.exit_code,
            signal: exit.signal,
            tail: exit.tail,
        }
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
    pub fn is_known(&self) -> bool {
        self.code.is_some() || self.signal.is_some() || self.tail.is_some()
    }

    /// `because`, and after it what the window showed and how it ended, as far
    /// as the host said: `the window closed (exit code 3); its screen ended
    /// with: "No API key found / Use /login"`. A screen with nothing on it
    /// says that. What was hidden in the quote is counted after it.
    pub fn after(&self, because: &str) -> String {
        let mut said = because.to_owned();
        if let Some(ended) = self.ended() {
            said.push_str(&format!(" ({ended})"));
        }
        let Some(tail) = &self.tail else {
            return said;
        };
        match Quote::of(tail) {
            None => said.push_str("; its screen was empty"),
            Some(quote) => {
                said.push_str(&format!("; its screen ended with: \"{}\"", quote.text));
                if quote.masked > 0 {
                    let (count, strings) = (quote.masked, plural(quote.masked));
                    said.push_str(&format!(" ({count} key- or token-like {strings} masked)"));
                }
            }
        }
        said
    }

    fn ended(&self) -> Option<String> {
        match (&self.signal, self.code) {
            (Some(signal), _) => Some(format!("ended by signal {signal}")),
            (None, Some(code)) => Some(format!("exit code {code}")),
            (None, None) => None,
        }
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        "string"
    } else {
        "strings"
    }
}

/// The words of a screen's last lines, held to the quote's bounds.
struct Quote {
    text: String,
    /// How many secrets were hidden in the lines that were kept.
    masked: usize,
}

impl Quote {
    /// The quote of `tail`: none where no line has a word in it. The last
    /// lines come first: each has its blanks made one, its secrets hidden and
    /// its quotation marks made apostrophes (a quote is not to end where the
    /// screen's text says it does), and they are kept while they fit.
    fn of(tail: &[String]) -> Option<Self> {
        let lines: Vec<String> = tail
            .iter()
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|line| !line.is_empty())
            .collect();
        let last = &lines[lines.len().saturating_sub(QUOTE_LINES)..];
        let mut kept: Vec<String> = Vec::new();
        let (mut masked, mut used) = (0, 0);
        for line in last.iter().rev() {
            let hidden = mask(line);
            let text = hidden.text.replace('"', "'");
            let between = if kept.is_empty() { 0 } else { BETWEEN.len() };
            let length = text.chars().count();
            if used + between + length > QUOTE_CHARS {
                // Only the last line may be cut; an older one that does not fit is left out.
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
        kept.reverse();
        (!kept.is_empty()).then(|| Self {
            text: kept.join(BETWEEN),
            masked,
        })
    }
}

/// The first characters of `text` that, with the mark that says it was cut,
/// make `limit`.
fn cut(text: &str, limit: usize) -> String {
    let kept: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{kept}…")
}
