//! Nothing the ledger writes bypasses `Store::write` and its batches (CLN-65): a write
//! outside the guarded transaction does not see that the ledger is broken, and appears
//! to succeed on a connection that is left in a transaction nothing will commit.
//!
//! A source test, read from the text: a function that writes (a statement that
//! changes rows, or a call of `execute`) outside a closure given to `write` or
//! `write_batch`, or that calls such a function outside one, is *exposed*; every
//! public function of the crate must be none, and so must what `open_ledger` calls.
//! The few functions that make the transactions themselves are named below.

#![allow(clippy::expect_used)]

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// What makes the transaction, or opens the file, or is the tests' own.
const THE_GUARD_AND_ITS_OWN: [&str; 9] = [
    "write",
    "write_batch",
    "migrate",
    "open_ledger",
    "open_existing_ledger",
    "open_ledger_as",
    "write_raw",
    "roll_back",
    "break_with",
];

struct Function {
    name: String,
    public: bool,
    /// Code of the body, strings blanked.
    body: Vec<u8>,
    /// Strings of the body, each with its offset in the body.
    strings: Vec<(usize, String)>,
}

#[test]
fn every_write_of_the_ledger_goes_through_the_guarded_transaction() {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut functions: Vec<Function> = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).expect("a source");
        let text = text
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("the code before its tests")
            .to_owned();
        functions.extend(functions_of(&text));
    }
    assert!(functions.len() > 500, "the sources were read");

    // The fixed point: who is exposed.
    let guarded: HashSet<&str> = THE_GUARD_AND_ITS_OWN.into_iter().collect();
    let mut exposed: HashSet<String> = HashSet::new();
    loop {
        let before = exposed.len();
        for function in &functions {
            if guarded.contains(function.name.as_str()) || exposed.contains(&function.name) {
                continue;
            }
            if exposes(function, &exposed, true) {
                exposed.insert(function.name.clone());
            }
        }
        if exposed.len() == before {
            break;
        }
    }

    let mut loose: Vec<String> = Vec::new();
    for function in &functions {
        if function.public
            && !guarded.contains(function.name.as_str())
            && exposes(function, &exposed, true)
        {
            loose.push(format!("pub fn {}", function.name));
        }
        if function.name == "open_ledger_as" && exposes(function, &exposed, false) {
            loose.push(
                "open_ledger_as calls a function that writes outside a guarded transaction"
                    .to_owned(),
            );
        }
    }
    loose.sort();
    loose.dedup();
    assert!(
        loose.is_empty(),
        "these write outside Store::write: {loose:#?}"
    );
}

/// Whether `function` writes, or calls an exposed function, outside every closure given
/// to `write`; its own statements count only where `own` says.
fn exposes(function: &Function, exposed: &HashSet<String>, own: bool) -> bool {
    let regions = write_regions(&function.body);
    let outside = |at: usize| !regions.iter().any(|(from, to)| (*from..*to).contains(&at));
    if own {
        for (at, text) in &function.strings {
            if outside(*at) && writes_rows(text) {
                return true;
            }
        }
        for token in ["execute(", "execute_batch("] {
            if find_all(&function.body, token.as_bytes())
                .into_iter()
                .any(outside)
            {
                return true;
            }
        }
    }
    let mut at = 0;
    while at < function.body.len() {
        let c = function.body[at];
        if is_word(c) && (at == 0 || !is_word(function.body[at - 1])) {
            let end = (at..function.body.len())
                .find(|&i| !is_word(function.body[i]))
                .unwrap_or(function.body.len());
            let word = String::from_utf8_lossy(&function.body[at..end]).into_owned();
            // A free call, or a method of the store: another receiver's method of the same name is another function.
            let before = &function.body[..at];
            let method = before.ends_with(b".");
            let on_store = before.ends_with(b"store.");
            if function.body.get(end) == Some(&b'(')
                && (!method || on_store)
                && exposed.contains(&word)
                && word != function.name
                && outside(at)
            {
                return true;
            }
            at = end;
        } else {
            at += 1;
        }
    }
    false
}

/// A statement that changes rows or the schema: SQL that begins with the verb (words that
/// only mention one are none).
fn writes_rows(sql: &str) -> bool {
    let first = sql
        .trim_start()
        .split(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or_default();
    matches!(
        first,
        "INSERT" | "UPDATE" | "DELETE" | "REPLACE" | "CREATE" | "DROP" | "ALTER"
    )
}

/// The spans of the arguments of each `.write(` and `.write_batch(` call.
fn write_regions(body: &[u8]) -> Vec<(usize, usize)> {
    let mut regions = Vec::new();
    for token in [".write(", ".write_batch("] {
        for at in find_all(body, token.as_bytes()) {
            let open = at + token.len() - 1;
            if let Some(close) = matching(body, open, b'(', b')') {
                regions.push((open, close));
            }
        }
    }
    regions
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    haystack
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .map(|(at, _)| at)
        .collect()
}

fn matching(code: &[u8], open: usize, up: u8, down: u8) -> Option<usize> {
    let mut depth = 0usize;
    for (at, c) in code.iter().enumerate().skip(open) {
        if *c == up {
            depth += 1;
        } else if *c == down {
            depth -= 1;
            if depth == 0 {
                return Some(at);
            }
        }
    }
    None
}

fn sources(folder: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(folder).expect("a folder") {
        let path = entry.expect("an entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if name != "testing" && name != "tests" {
                sources(&path, found);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && name != "testing.rs"
            // The role tables are text in memory, and keep no SQLite connection.
            && name != "roles.rs"
            && name != "tests.rs"
        {
            found.push(path);
        }
    }
}

/// Code with comments and the insides of strings and characters blanked, and the strings.
fn blanked(text: &str) -> (Vec<u8>, Vec<(usize, String)>) {
    let bytes = text.as_bytes();
    let mut code = bytes.to_vec();
    let mut strings = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let c = bytes[at];
        if c == b'/' && bytes.get(at + 1) == Some(&b'/') {
            while at < bytes.len() && bytes[at] != b'\n' {
                code[at] = b' ';
                at += 1;
            }
        } else if c == b'/' && bytes.get(at + 1) == Some(&b'*') {
            let end = text[at..].find("*/").map_or(bytes.len(), |n| at + n + 2);
            for blank in &mut code[at..end] {
                if *blank != b'\n' {
                    *blank = b' ';
                }
            }
            at = end;
        } else if c == b'"'
            || (c == b'r'
                && matches!(bytes.get(at + 1), Some(b'"' | b'#'))
                && at > 0
                && !is_word(bytes[at - 1]))
        {
            let (start, close): (usize, String) = if c == b'"' {
                (at + 1, "\"".to_owned())
            } else {
                let hashes = bytes[at + 1..].iter().take_while(|b| **b == b'#').count();
                if bytes.get(at + 1 + hashes) != Some(&b'"') {
                    at += 1;
                    continue;
                }
                (at + 2 + hashes, format!("\"{}", "#".repeat(hashes)))
            };
            let mut end = start;
            loop {
                if end >= bytes.len() {
                    break;
                }
                if c == b'"' && bytes[end] == b'\\' {
                    end += 2;
                    continue;
                }
                if bytes[end..].starts_with(close.as_bytes()) {
                    break;
                }
                end += 1;
            }
            let end = end.min(bytes.len());
            strings.push((
                start,
                String::from_utf8_lossy(&bytes[start..end]).into_owned(),
            ));
            for blank in &mut code[start..end] {
                if *blank != b'\n' {
                    *blank = b' ';
                }
            }
            at = (end + close.len()).min(bytes.len());
        } else if c == b'\'' {
            // A character, or a lifetime.
            let rest = &text[at + 1..];
            let closes = if let Some(escaped) = rest.strip_prefix('\\') {
                escaped.find('\'').map(|n| n + 2)
            } else {
                rest.chars().next().and_then(|first| {
                    (rest[first.len_utf8()..].starts_with('\'')).then_some(first.len_utf8() + 1)
                })
            };
            if let Some(n) = closes {
                for blank in &mut code[at + 1..at + n] {
                    *blank = b' ';
                }
                at += n + 1;
            } else {
                at += 1;
            }
        } else {
            at += 1;
        }
    }
    (code, strings)
}

fn functions_of(text: &str) -> Vec<Function> {
    let (code, strings) = blanked(text);
    let mut found = Vec::new();
    for at in find_all(&code, b"fn ") {
        if at > 0 && is_word(code[at - 1]) {
            continue;
        }
        let name_from = at + 3;
        let name_to = (name_from..code.len())
            .find(|&i| !is_word(code[i]))
            .unwrap_or(code.len());
        let name = String::from_utf8_lossy(&code[name_from..name_to]).into_owned();
        if name.is_empty() {
            continue;
        }
        // The body: the first `{` at bracket depth 0 after the parameters, unless a `;` comes first.
        let mut depth = 0i32;
        let mut open = None;
        for (i, c) in code.iter().enumerate().skip(name_to) {
            match c {
                b'(' | b'[' | b'<' => depth += 1,
                b')' | b']' | b'>' => {
                    if !(*c == b'>' && i > 0 && code[i - 1] == b'-') {
                        depth -= 1;
                    }
                }
                b';' if depth <= 0 => break,
                b'{' if depth <= 0 => {
                    open = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let Some(open) = open else { continue };
        let Some(close) = matching(&code, open, b'{', b'}') else {
            continue;
        };
        let line_start = code[..at]
            .iter()
            .rposition(|c| *c == b'\n')
            .map_or(0, |n| n + 1);
        let head = String::from_utf8_lossy(&code[line_start..at]).into_owned();
        let public = head.trim_start().starts_with("pub ") && !head.contains("pub(");
        found.push(Function {
            name,
            public,
            body: code[open..close].to_vec(),
            strings: strings
                .iter()
                .filter(|(from, _)| (open..close).contains(from))
                .map(|(from, text)| (from - open, text.clone()))
                .collect(),
        });
    }
    found
}
