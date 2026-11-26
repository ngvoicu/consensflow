//! The reason a harness's model list failed, as it may be kept and told outside a screen.
//!
//! A reason is the harness's own description and, where it started a program that loads code of
//! the human's (Pi: its extensions and packages), after [`IT_SAID`] the words of that program:
//! what Pi's error stream held, what `pi update --models` said, the `error` of Pi's refusal.
//! Those words can carry what the program was given (an extension's error that echoes a header,
//! a package's URL), and they are kept in the ledger, which the page reads, and told in the
//! note of a chief whose model is gone.
//!
//! **A reason with no mark is ConsensFlow's, or another harness's own words, and is masked as
//! a note masks** ([`mask`]): the login, key and billing messages of Claude Code, Codex, Devin
//! and Muse tell the user what to do (`Invalid API key · Please run /login`, `Not logged in.
//! Run `devin auth login``, `No Meta credentials were found. Run `muse login``) and are not
//! looked at as a program's words (the third reading of T-1270, F9: the rule of a screen,
//! applied to them, withheld each of those).
//!
//! **The words after the mark are screened whole, and cut after** (F2 of the third reading).
//! Line by line, a value that ran over lines had its first line withheld and its continuations
//! kept, and a cut to the last lines made before the look took the line that named a
//! credential and left the lines of its value. So the block is taken as one text, **every
//! line of it, a stack frame among them** (T-1276, `frame-label-continuation`: `at password:
//! correct (native)` is a frame by its shape and the label of the lines below it by its words),
//! and if anything in it may hold a credential, **none of it is quoted** and [`WORDS_WITHHELD`]
//! stands in its place. A frame ([`FRAME`]: `at fn (file:1:2)`, `at file:1:2`,
//! `at fn (<anonymous>)`; prose that begins with `at` is none) is set aside from what is
//! quoted, not from what is looked at; and the lines that are no frame are looked at on their
//! own as well, for a name or a token the wrap cut in two around a frame is whole only without
//! it (`s` / `at load (native)` / `k-fake…`). The description before the mark, the exit code in it and
//! the names of what Pi was loading are kept. Only a block that passed is cut, to its last
//! [`QUOTE_LINES`] lines that are no frame within [`QUOTE_CHARS`] characters, and masked.
//!
//! What may hold a credential is the screens' own rule on the lines as one text
//! ([`screen::withheld`]: a secret-like word on any line, a token's beginning, a key's body, a
//! value in groups, whatever the wrap does to them) and a credential's name with a value after
//! it **whatever the value is** ([`screen::names_a_value`]: `key: abc`, `"secret": "a b"`,
//! `API_KEY=…`, `token:` and the value below, in any case, the name cut by a line or written in
//! percent signs). The cost is the blocks that name a credential and hold none
//! (`Invalid key: expected string`, `OPENAI_API_KEY is not set`): they are left out whole, with
//! the description and the code, and [`WORDS_WITHHELD`] says where to read them. No list of the
//! harmless hints is kept, and no value is left to the masker for being too short for it: the
//! joint review T-1322 found `key: abc`, `password` then `hunter22`, and `Authentication failed`
//! then `hunter22` quoted by a policy that did, and a hint and a block that holds a secret
//! differ in where their words came from, which text does not show.
//! Nothing before the first mark can be a program's, so a forged mark among its words only
//! makes the block longer.

use std::sync::LazyLock;

use cf_harness::records::Quota;
use cf_proto::models::IT_SAID;
use regex::Regex;

use super::mask::mask;
use super::screen;

/// What stands in place of the words of a program that may hold a credential.
pub use cf_proto::models::WORDS_WITHHELD;

/// The most lines of the words of a program that are quoted: the last ones.
const QUOTE_LINES: usize = 4;
/// The most characters they take; the rest is cut and says so.
const QUOTE_CHARS: usize = 600;

/// A line of a stack, which is never quoted: Node's `at name (file:line:column)`, `at
/// file:line:column`, `at name (<anonymous>)`, `at name (native)` and `at async Promise.all (index 0)`. A line that begins with
/// `at` and is none of these is prose, and is looked at with the rest: it may be the label of
/// the value below it.
#[allow(clippy::expect_used)]
static FRAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^at\s.*(?::\d+:\d+\)?|\(<anonymous>\)|\(native\)|\(index \d+\))$")
        .expect("a pattern")
});

/// Whether `lines`, taken as one text, may hold a credential.
fn may_hold_a_credential(lines: &[&str]) -> bool {
    let rows: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
    screen::withheld(&rows, None).is_some() || screen::names_a_value(lines)
}

/// `reason` as it may be kept and told: masked, and where it carries the words of a program
/// after [`IT_SAID`], those screened whole and then cut (see the module). The lines of the words
/// that are quoted stand on lines of their own, joined with `\n`.
pub fn quotable(reason: &str) -> String {
    let Some((own, program)) = reason.split_once(IT_SAID) else {
        return mask(reason).text;
    };
    let own = mask(own).text;
    match said(program) {
        Some(said) => format!("{own}{IT_SAID}{said}"),
        None => own,
    }
}

/// The words of a program, screened whole before they are cut: none where it said nothing that
/// is quoted, [`WORDS_WITHHELD`] where anything it said may hold a credential, else its last
/// lines, masked.
fn said(program: &str) -> Option<String> {
    let lines: Vec<&str> = program
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    // A frame is never quoted, but it is looked at: `at password: correct (native)` is the
    // label of the lines below it as much as any line (T-1276, `frame-label-continuation`).
    let quoted: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| !FRAME.is_match(line))
        .collect();
    if quoted.is_empty() {
        return None;
    }
    // Looked at twice where there are frames: with them, for one may be the label of the lines
    // below it, and without them, so that a name or a token the wrap cut in two around a frame
    // is whole at the join (T-1276, `frame-wrapped-token`: `s` / `at load (native)` / `k-fake…`).
    let holds = may_hold_a_credential(&lines)
        || (quoted.len() < lines.len() && may_hold_a_credential(&quoted));
    if holds {
        return Some(WORDS_WITHHELD.to_owned());
    }
    let kept = &quoted[quoted.len().saturating_sub(QUOTE_LINES)..];
    let joined = mask(&kept.join("\n")).text;
    Some(match joined.char_indices().nth(QUOTE_CHARS) {
        Some((cut, _)) => format!("{}…", &joined[..cut]),
        None => joined,
    })
}

/// A hold's report with its words as [`held_words`] lets them be kept: the report every writer
/// of the hold reads (the ledger's event, the window's activity, the notes).
pub fn screened_hold(quota: &Quota) -> Quota {
    match quota {
        Quota::Unavailable { at, because } => Quota::Unavailable {
            at: at.clone(),
            because: held_words(because),
        },
        Quota::Moved { at, because } => Quota::Moved {
            at: at.clone(),
            because: held_words(because),
        },
        Quota::Busy { at, because } => Quota::Busy {
            at: at.clone(),
            because: bounded(&mask(because).text),
        },
        other => other.clone(),
    }
}

/// What the card says where the harness's words of a refusal were left out.
pub const LEFT_OUT: &str =
    "the harness's words were left out because they named a key or a login; read them in its window";

/// The most characters of a hold's words that are kept: the one bound, set here, after the
/// screen has seen the words whole. No harness cuts its words before they come here (T-2353: a
/// cut before the screen took the `@` of a gateway URL and left its password in what the screen
/// saw), and none of the writers of a hold sets a bound of its own.
const WORDS_CHARS: usize = 300;

/// `text` cut at [`WORDS_CHARS`] characters, and saying so.
fn bounded(text: &str) -> String {
    match text.char_indices().nth(WORDS_CHARS) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    }
}

/// The words of a hold (`Quota::Unavailable`'s or `Quota::Moved`'s `because`) as they may be
/// kept in the ledger, said on the board and told in a note (T-2010). A refusal's words
/// (`its billing was refused: …`, `its login was refused: …`) are screened whole by the rule of a
/// program's words ([`said`]): where anything in them may hold a credential none is kept, and
/// the reason says which refusal it was and that the words were left out; what passes is cut at
/// [`WORDS_CHARS`]. Any other reason (Muse's fixed sentences, a model's move) is masked whole and
/// cut at the same bound.
pub fn held_words(because: &str) -> String {
    for lead in ["its billing was refused", "its login was refused"] {
        let Some(rest) = because.strip_prefix(lead) else {
            continue;
        };
        let words = rest.strip_prefix(": ").unwrap_or(rest);
        return match said(words) {
            None => lead.to_owned(),
            Some(kept) if kept == WORDS_WITHHELD => format!("{lead}; {LEFT_OUT}"),
            Some(kept) => format!("{lead}: {}", bounded(&kept.replace('\n', " "))),
        };
    }
    bounded(&mask(because).text)
}

#[cfg(test)]
mod tests;
