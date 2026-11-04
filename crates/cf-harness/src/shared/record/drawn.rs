//! What the history draws of an item besides its words (chief-harness CHF-14): what kind
//! of item it is; for a tool's call its name, a header, how it ended, the time it took and
//! the diff it made; for a compaction the tokens it compacted from. And the items only the
//! history shows (a model's thinking, a call still running, a compaction a harness wrote no
//! summary item of), which stand beside a reading's items ([`Record::beside`]), never among
//! them: no reader of the conversation's words (a result, a handoff's last words, "the turn
//! answered") ever comes to one.
//!
//! A reading Node wrote had none of this, and the recordings hold the Rust readers to those
//! readings: none of it is said where a reading is ([`Item`]'s serialization leaves the
//! detail out, and a record's serialization the items beside it).
//!
//! [`Record::beside`]: super::reading::Record::beside

use std::sync::Arc;

use serde_json::Value;

use super::reading::{Item, Record, Role};

/// What an item is, as the history draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// What the conversation was given: the human's words, or ConsensFlow's delivery.
    Prompt,
    /// The agent's words.
    Text,
    /// The model's thinking, where its harness keeps it: never the agent's words.
    Thinking,
    /// A tool's call, with what it returned.
    Tool,
    /// A compaction of the conversation, with its summary where the harness wrote one.
    Compaction,
    /// What the harness put in the conversation itself (a hook's context, an extension's
    /// message, a notification).
    Notice,
}

impl Kind {
    /// The word the ledger keeps it by.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Prompt => "prompt",
            Kind::Text => "text",
            Kind::Thinking => "thinking",
            Kind::Tool => "tool",
            Kind::Compaction => "compaction",
            Kind::Notice => "notice",
        }
    }
}

/// How a tool's call stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Called, with no result yet.
    Running,
    /// It returned, and its harness says it did not fail.
    Ok,
    /// Its harness says it failed (an error, a command's exit code that is not 0).
    Failed,
}

impl Status {
    /// The word the ledger keeps it by.
    pub fn word(self) -> &'static str {
        match self {
            Status::Running => "running",
            Status::Ok => "ok",
            Status::Failed => "failed",
        }
    }
}

/// What the history draws of an item besides its words: its kind, and what its harness's
/// record says of a call or a compaction. A field the record does not say is none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detail {
    pub kind: Kind,
    /// The tool's name, as its harness calls it.
    pub name: Option<Arc<str>>,
    /// The call in one line, as the history heads its block: `$ cmd`, `read path:range`,
    /// `edit path`, `write path`, or the tool's name and what it was given. At most
    /// [`HEADER_MAX`] characters.
    pub header: Option<Arc<str>>,
    pub status: Option<Status>,
    /// How long the call took, in milliseconds.
    pub took_ms: Option<u64>,
    /// The change an edit made, as unified diff lines (`-old`, `+new`, ` kept`, `@@ …`).
    pub diff: Option<Arc<str>>,
    /// How many tokens a compaction compacted the conversation from.
    pub tokens: Option<u64>,
}

impl Detail {
    /// A detail of `kind` that says nothing else.
    pub fn of(kind: Kind) -> Self {
        Self {
            kind,
            name: None,
            header: None,
            status: None,
            took_ms: None,
            diff: None,
            tokens: None,
        }
    }
}

/// The most characters a header holds.
pub const HEADER_MAX: usize = 256;

/// What a call does, as its header says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action<'a> {
    /// A command a shell runs: `$ cmd`.
    Shell(&'a str),
    /// A file read, from a line and for some lines where the call says: `read path:from-to`.
    Read {
        path: &'a str,
        from: Option<u64>,
        lines: Option<u64>,
    },
    /// A file changed in place: `edit path`.
    Edit(&'a str),
    /// A file written whole: `write path`.
    Write(&'a str),
    /// Any other call: its tool's name, and what it was given where one thing says it.
    Other {
        name: &'a str,
        about: Option<&'a str>,
    },
}

/// The header of a call that does `action`: its first line, and "…" where it says more
/// than a line or more than [`HEADER_MAX`] characters.
pub fn header(action: Action<'_>) -> Arc<str> {
    let said = match action {
        Action::Shell(command) => format!("$ {command}"),
        Action::Read { path, from, lines } => match (from, lines) {
            (Some(from), Some(lines)) => format!(
                "read {path}:{from}-{}",
                from.saturating_add(lines.saturating_sub(1))
            ),
            (Some(from), None) => format!("read {path}:{from}"),
            (None, Some(lines)) => format!("read {path}:1-{lines}"),
            (None, None) => format!("read {path}"),
        },
        Action::Edit(path) => format!("edit {path}"),
        Action::Write(path) => format!("write {path}"),
        Action::Other { name, about } => {
            match about.map(str::trim).filter(|about| !about.is_empty()) {
                Some(about) => format!("{name} {about}"),
                None => name.to_owned(),
            }
        }
    };
    Arc::from(one_line(&said, HEADER_MAX))
}

/// `text`'s first line, cut to `max` characters with "…" where it says more.
fn one_line(text: &str, max: usize) -> String {
    let text = text.trim();
    let first = text.lines().next().unwrap_or_default();
    let more = first.len() < text.len();
    let mut shown: String = first.chars().take(max).collect();
    if more || shown.len() < first.len() {
        // The mark takes the place of the last character kept, so the whole is `max` at most.
        if shown.chars().count() == max {
            shown.pop();
        }
        shown.push('…');
    }
    shown
}

/// The header of a call to the tool `name` with `arguments` (an object, or JSON text of one,
/// as some harnesses write it), for a harness whose tools are not this module's to know by
/// name: a call that gives a command is a shell's; one named for reading, editing or
/// writing a file and that names one is that; any other is its name and what it was given.
pub fn guess(name: &str, arguments: Option<&Value>) -> Arc<str> {
    let parsed;
    let arguments = match arguments {
        Some(Value::String(text)) => {
            parsed = serde_json::from_str::<Value>(text).ok();
            parsed.as_ref()
        }
        other => other,
    };
    let says = |member: &str| arguments.and_then(|arguments| arguments.get(member));
    let text = |member: &str| says(member).and_then(Value::as_str);
    let command = ["command", "cmd"]
        .into_iter()
        .find_map(|member| match says(member) {
            Some(Value::String(command)) => Some(command.clone()),
            Some(Value::Array(words)) => Some(shell_words(words)),
            _ => None,
        });
    let path = [
        "path",
        "file_path",
        "filePath",
        "file",
        "target_file",
        "notebook_path",
    ]
    .into_iter()
    .find_map(text);
    let named = name.to_ascii_lowercase();
    let is = |words: &[&str]| words.iter().any(|word| named.contains(word));
    let number = |members: &[&str]| {
        members
            .iter()
            .find_map(|member| says(member).and_then(Value::as_u64))
    };
    let action = match (&command, path) {
        (Some(command), _) => Action::Shell(command),
        (None, Some(path)) if is(&["edit", "patch", "replace"]) => Action::Edit(path),
        (None, Some(path)) if is(&["write", "create"]) => Action::Write(path),
        (None, Some(path)) if is(&["read", "view", "open", "cat"]) => {
            let from = number(&["offset", "start_line", "line"]);
            let lines = match (from, number(&["end_line"])) {
                (Some(from), Some(end)) if end >= from => Some(end - from + 1),
                _ => number(&["limit", "lines"]),
            };
            Action::Read { path, from, lines }
        }
        (None, path) => Action::Other {
            name,
            about: ["pattern", "query", "url"]
                .into_iter()
                .find_map(text)
                .or(path)
                .or_else(|| text("description")),
        },
    };
    header(action)
}

/// A command given as its words: the script a shell is given to run (`sh -c`, `bash -lc`,
/// `/bin/zsh -c` on the Mac; `cmd.exe /c`, `powershell.exe -NoProfile -Command`, `pwsh -c`
/// on Windows: its last word, after a flag that runs it), else the words with a space
/// between.
fn shell_words(words: &[Value]) -> String {
    const SHELLS: [&str; 8] = [
        "sh",
        "bash",
        "zsh",
        "dash",
        "fish",
        "cmd",
        "powershell",
        "pwsh",
    ];
    const RUNS: [&str; 6] = ["-c", "-lc", "-ic", "/c", "-command", "-encodedcommand"];
    let words: Vec<&str> = words.iter().filter_map(Value::as_str).collect();
    let shell = words.first().map(|first| {
        let name = first
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(first)
            .to_ascii_lowercase();
        name.strip_suffix(".exe").map(str::to_owned).unwrap_or(name)
    });
    match words.as_slice() {
        [.., flag, script]
            if words.len() >= 3
                && shell
                    .as_deref()
                    .is_some_and(|shell| SHELLS.contains(&shell))
                && RUNS.contains(&flag.to_ascii_lowercase().as_str()) =>
        {
            (*script).to_owned()
        }
        _ => words.join(" "),
    }
}

/// A tool's detail: its name and header, how it stands, and what else its record says.
pub fn tool(name: &str, header: Arc<str>, status: Option<Status>) -> Detail {
    Detail {
        name: Some(Arc::from(name)),
        header: Some(header),
        status,
        ..Detail::of(Kind::Tool)
    }
}

/// The time between two of a record's times, in milliseconds since the epoch: none where
/// either is none or the second comes first.
pub fn took(called: Option<i64>, returned: Option<i64>) -> Option<u64> {
    let (called, returned) = (called?, returned?);
    u64::try_from(returned.checked_sub(called)?).ok()
}

/// An item only the history shows: of `role` as no reader of the conversation's words takes
/// one, with `detail`.
pub fn beside_item(
    id: Arc<str>,
    role: Role,
    text: &str,
    at: Option<serde_json::Value>,
    detail: Detail,
) -> Item {
    Item {
        id,
        role,
        text: Arc::from(text),
        complete: true,
        at,
        commentary: false,
        synthetic: false,
        compaction: detail.kind == Kind::Compaction,
        detail: Some(Arc::new(detail)),
    }
}

/// A record's items and those only the history shows, in the history's order: the order
/// ConsensFlow's copy keeps them in.
#[derive(Debug, Clone, Copy)]
pub struct Timeline<'a> {
    items: &'a [Item],
    beside: &'a [(usize, Item)],
}

impl<'a> Timeline<'a> {
    /// The timeline of `record`.
    pub fn of(record: &'a Record) -> Self {
        Self {
            items: &record.items,
            beside: &record.beside,
        }
    }

    /// The timeline of a record that could not be read: none.
    pub fn none() -> Self {
        Self {
            items: &[],
            beside: &[],
        }
    }

    pub fn len(&self) -> usize {
        self.items.len() + self.beside.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Its items from the one at `first` on, in order.
    pub fn from(&self, first: usize) -> impl Iterator<Item = (&'a Item, bool)> + 'a {
        let items = self.items;
        let beside = self.beside;
        // The items beside that stand before `first`: one at place `p` among the items, the
        // `j`th of them, stands at `p + j` in the timeline.
        let skipped = beside
            .iter()
            .enumerate()
            .take_while(|(j, (place, _))| place + j < first)
            .count();
        let mut next_item = first - skipped.min(first);
        let mut next_beside = skipped;
        std::iter::from_fn(move || {
            let item_at = next_item;
            match beside.get(next_beside) {
                Some((place, item)) if *place <= item_at => {
                    next_beside += 1;
                    Some((item, true))
                }
                _ => {
                    let item = items.get(item_at)?;
                    next_item += 1;
                    Some((item, false))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests;
