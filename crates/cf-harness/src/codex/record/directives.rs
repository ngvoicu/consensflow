//! What Codex's own screen draws and nobody else should be handed: the
//! directives the model writes into its answer for Codex's UI,
//! `:codex-<name>[label]{key="value" …}`: a file citation
//! (`:codex-file-citation{path="…" purpose="source"}`), a suggested follow-up
//! (`:codex-followup[Short action]{prompt="…"}`). Whoever reads the answer in
//! ConsensFlow (the chief a worker's result goes to, the board) reads it as
//! text, where the markup says nothing. A file citation reads as its path;
//! any other directive as its label, or as nothing where it has none.
//!
//! A directive is read as the markdown that Codex's screen reads it by, so
//! text that quotes one is left as written: inside a fenced block, or between
//! backticks, it is code. A directive that is not whole (its braces are never
//! closed) is text as well.

use std::borrow::Cow;

/// What every directive of Codex's begins with.
const OPENING: &str = ":codex-";

/// The directive that cites a file: it reads as its `path`.
const FILE_CITATION: &str = "file-citation";

/// `text` as it reads without the directives of Codex's screen: the text
/// itself where it holds none.
pub(super) fn readable(text: &str) -> Cow<'_, str> {
    if !text.contains(OPENING) {
        return Cow::Borrowed(text);
    }
    let mut read = String::with_capacity(text.len());
    // The part of the text not yet put in `read`, and the fence it is inside of.
    let mut from = 0;
    let mut fence: Option<Fence> = None;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let end = at + line.len();
        match fence {
            None => {
                if let Some(opening) = Fence::opened_by(line) {
                    read.push_str(&prose(&text[from..at]));
                    from = at;
                    fence = Some(opening);
                }
            }
            Some(opening) => {
                if opening.closed_by(line) {
                    read.push_str(&text[from..end]);
                    from = end;
                    fence = None;
                }
            }
        }
        at = end;
    }
    match fence {
        Some(_) => read.push_str(&text[from..]),
        None => read.push_str(&prose(&text[from..])),
    }
    if read == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(read)
    }
}

/// A fenced block of code: the character it is fenced with, and how many of
/// them opened it.
#[derive(Clone, Copy)]
struct Fence {
    mark: char,
    length: usize,
}

impl Fence {
    /// The fence the line opens, if it does: three or more backticks or
    /// tildes, after at most three spaces.
    fn opened_by(line: &str) -> Option<Self> {
        let (mark, length) = Self::run(line)?;
        // A backtick fence's own line holds no other backtick.
        if mark == '`' && line.trim_start_matches([' ', '`']).contains('`') {
            return None;
        }
        Some(Self { mark, length })
    }

    /// Whether the line ends this fence: as many of its character, or more, and
    /// nothing else.
    fn closed_by(self, line: &str) -> bool {
        match Self::run(line) {
            Some((mark, length)) => {
                mark == self.mark
                    && length >= self.length
                    && line
                        .trim_start_matches(' ')
                        .trim_start_matches(self.mark)
                        .trim()
                        .is_empty()
            }
            None => false,
        }
    }

    /// The character a line begins with three or more times, after at most
    /// three spaces, and how many times.
    fn run(line: &str) -> Option<(char, usize)> {
        let indent = line.chars().take_while(|char| *char == ' ').count();
        if indent > 3 {
            return None;
        }
        let rest = &line[indent..];
        let mark = rest
            .chars()
            .next()
            .filter(|mark| matches!(mark, '`' | '~'))?;
        let length = rest.chars().take_while(|char| *char == mark).count();
        (length >= 3).then_some((mark, length))
    }
}

/// A run of text outside any fenced block, its directives read. Text between
/// backticks is code and stays as it is.
fn prose(text: &str) -> Cow<'_, str> {
    let mut read = String::new();
    let (mut from, mut at) = (0, 0);
    while at < text.len() {
        let rest = &text[at..];
        if rest.starts_with('`') {
            at += code_span(rest);
        } else if rest.starts_with(OPENING) && !text[..at].ends_with(':') {
            let Some(found) = Directive::read(rest) else {
                at += OPENING.len();
                continue;
            };
            read.push_str(&text[from..at]);
            let after = at + found.length;
            let shown = found.shown();
            // A directive that reads as nothing leaves no gap behind it.
            if shown.is_empty()
                && read.ends_with([' ', '\t'])
                && text[after..].chars().next().is_none_or(char::is_whitespace)
            {
                read.pop();
            }
            read.push_str(&shown);
            (from, at) = (after, after);
        } else {
            // A character may be more than a byte: the next place to look is its end.
            at += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    if from == 0 {
        return Cow::Borrowed(text);
    }
    read.push_str(&text[from..]);
    Cow::Owned(read)
}

/// How much of `rest`, which begins with a backtick, is a code span or the
/// backticks of a run that opens none: the run of backticks and what is
/// between it and the next run of as many.
fn code_span(rest: &str) -> usize {
    let ticks = rest.bytes().take_while(|byte| *byte == b'`').count();
    let mut at = ticks;
    while let Some(next) = rest[at..].find('`') {
        let start = at + next;
        let run = rest[start..]
            .bytes()
            .take_while(|byte| *byte == b'`')
            .count();
        if run == ticks {
            return start + run;
        }
        at = start + run;
    }
    ticks
}

/// A directive as the text wrote it.
struct Directive {
    /// What it is called after `codex-`: `file-citation`, `followup`.
    name: String,
    /// What was between its square brackets.
    label: Option<String>,
    /// Its attributes, in the order written.
    attributes: Vec<(String, String)>,
    /// How many bytes of the text it takes up.
    length: usize,
}

impl Directive {
    /// The directive `text` begins with, if what it begins with is whole.
    fn read(text: &str) -> Option<Self> {
        let named = &text[OPENING.len()..];
        let name_length = named
            .find(|char: char| !(char.is_ascii_alphanumeric() || matches!(char, '-' | '_')))
            .unwrap_or(named.len());
        if !named.chars().next()?.is_ascii_alphabetic() {
            return None;
        }
        let mut length = OPENING.len() + name_length;
        let name = named[..name_length].to_owned();
        let mut label = None;
        if text[length..].starts_with('[') {
            let inside = bracketed(&text[length..])?;
            label = Some(text[length + 1..length + inside - 1].to_owned());
            length += inside;
        }
        let mut attributes = Vec::new();
        if text[length..].starts_with('{') {
            let (found, inside) = attributes_of(&text[length..])?;
            attributes = found;
            length += inside;
        }
        Some(Self {
            name,
            label,
            attributes,
            length,
        })
    }

    /// What the directive reads as: a file citation its path, anything else
    /// its label, and nothing where there is neither.
    fn shown(&self) -> String {
        let attribute = |wanted: &str| {
            self.attributes
                .iter()
                .find(|(name, _)| name == wanted)
                .map(|(_, value)| value.as_str())
        };
        let label = self.label.as_deref().filter(|label| !label.is_empty());
        if self.name == FILE_CITATION {
            if let Some(path) = attribute("path").filter(|path| !path.is_empty()) {
                return path.to_owned();
            }
        }
        label.unwrap_or_default().to_owned()
    }
}

/// How many bytes `text`, which begins with `[`, takes up to the `]` that
/// closes it: none where it is never closed. Brackets inside it that match
/// each other, and a character after a backslash, do not close it.
fn bracketed(text: &str) -> Option<usize> {
    let mut depth = 0_usize;
    let mut escaped = false;
    for (at, char) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match char {
            '\\' => escaped = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// The attributes of `text`, which begins with `{`, and how many bytes it
/// takes up to the `}` that ends them: none where they are never ended. A
/// value is quoted (with `"` or `'`, which a backslash can escape) or runs to
/// the next space; a name with no value has none.
fn attributes_of(text: &str) -> Option<(Vec<(String, String)>, usize)> {
    let mut attributes = Vec::new();
    let mut at = 1;
    loop {
        at += text[at..].len() - text[at..].trim_start().len();
        match text[at..].chars().next()? {
            '}' => return Some((attributes, at + 1)),
            _ => {
                let name_end = text[at..]
                    .find(|char: char| char.is_whitespace() || matches!(char, '=' | '}'))
                    .map_or(text.len(), |end| at + end);
                let name = text[at..name_end].to_owned();
                at = name_end;
                if name.is_empty() {
                    return None;
                }
                if !text[at..].starts_with('=') {
                    attributes.push((name, String::new()));
                    continue;
                }
                at += 1;
                let (value, taken) = value_of(&text[at..])?;
                attributes.push((name, value));
                at += taken;
            }
        }
    }
}

/// The value `text` begins with, and how many bytes it takes up: between
/// quotes, or up to a space or the end of the attributes.
fn value_of(text: &str) -> Option<(String, usize)> {
    let quote = text
        .chars()
        .next()
        .filter(|char| matches!(char, '"' | '\''));
    let Some(quote) = quote else {
        let end = text
            .find(|char: char| char.is_whitespace() || char == '}')
            .unwrap_or(text.len());
        return Some((text[..end].to_owned(), end));
    };
    let mut value = String::new();
    let mut escaped = false;
    for (at, char) in text.char_indices().skip(1) {
        if escaped {
            if char != quote {
                value.push('\\');
            }
            value.push(char);
            escaped = false;
        } else if char == '\\' {
            escaped = true;
        } else if char == quote {
            return Some((value, at + 1));
        } else {
            value.push(char);
        }
    }
    None
}

#[cfg(test)]
mod tests;
