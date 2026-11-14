//! Every command of the board that takes a text takes it from standard input when the text is `-`,
//! and the commands that give a window work take it from a file as well (`--file`), so that a
//! chief's brief reaches the board as written where a double-quoted one would lose its backticks
//! and `$( )` to the shell (the field report of 2026-09-26, issue `brief-backticks-run-as-commands`).
//! The fix put the reading in `text_of` (`src/board/mod.rs`), and its tests covered `task add -`
//! and `task edit -` alone: a command that reads its text some other way, or forgets `-`, passed.
//!
//! So the commands are not listed here. They are the lines of the table `cf help` prints
//! (`src/board/usage.txt`) whose command ends in a text, `"…"`, outside any `[ ]`: `cf history`'s
//! `[--find "…"]` is a flag's value, not a text. Each is run as the table writes it (`<tier>` and
//! the other words filled in), with its text given four ways, and the body the board received is
//! compared with the text byte for byte:
//!
//! - on standard input at `-`, with the heredoc's final line break, which is not the text's;
//! - as one argument, many lines and every character a shell or cmd.exe has a meaning for;
//! - in a file, for the commands the table's footer names as taking one (`--file`);
//! - on Unix, through a real `sh` and the quoted heredoc the role cards show
//!   (`cf task add --tier standard - <<'BRIEF'`);
//! - in the encodings Windows PowerShell 5.1 writes (UTF-16 with a byte order mark, both ways in),
//!   which are decoded, and in those that are no text (UTF-16 without a mark, a NUL byte, a Windows
//!   code page), which are refused with nothing posted (the review T-1447, finding 1).
//!
//! A command that takes a text and is not in the table is found from the sources: every function
//! of `src/board` that reads one (`text_of`, `brief_of`) is a command of the table, or `moved`,
//! which serves `cf task <verb>` for the verbs of `MOVES`. `cf task done` is in the table, so
//! `done` is covered; `accept`, `cancel`, `pause` and `finish` take no text, and the table says
//! none, so they are not run here.

// The tests' own helpers: a failure in one is the test's.
#![allow(clippy::expect_used)]

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use cf_board::scripted::{reply, scripted};
use common::{cf, cf_bytes};
use serde_json::{json, Value};

/// What a double-quoted brief loses to the shell, and what cmd.exe and the other shells have a
/// meaning for: command substitution, `$( )`, `${ }`, `$(( ))`, backslashes, both kinds of quote,
/// `%PATH%`, `!x!`, `^`, `&`, `|`, redirections, globs, a line break in every spelling, a blank
/// line, trailing spaces, a tab, diacritics, an emoji, a bullet list and a flag among the words.
const TEXTS: [&str; 2] = [
    "Run `cf ask` first, then `cf ask --human \"…\"`: do not run them.\n\
     $(echo gone) ${HOME} $HOME $((1+1)) \\$ \\` \\\\ `echo gone`\n\
     He said \"only the Romanian one\" and it's 'fine'.\n\
     100% sure | %PATH% ^ !x! & > < ; # * ? [a] {b} ~\r\n\
     Line after a CRLF.\n\
     \n\
     \tindented, trailing spaces   \n\
     Întrebarea 1: 漢字 🙂\n\
     --force and -h inside the text",
    "- first\n- second `third`\n\n$(second) \"third\"",
];

/// The board's answer to any one command: a task as `cf task add` and the others read it, and a
/// message as `cf ask`, `cf note`, `cf answer` and `cf tell` do. A command takes what it reads
/// and ignores the rest.
fn the_answer() -> Value {
    json!({
        "task": {
            "number": 3, "state": "open", "assignee": null, "requester": "chief",
            "title": "Parser", "blockedBy": [], "pool": "worker", "tier": "standard",
        },
        "message": {
            "id": 5, "kind": "note", "state": "queued", "sender": "chief",
            "recipient": "zeus", "task": 3, "body": "x",
        },
        "gated": false,
    })
}

/// A command of the table: the words it is named by, and the words it is run with up to its text.
#[derive(Debug, Clone)]
struct Command {
    /// `["task", "add"]`, `["tell"]`: the lowercase words after `cf`.
    name: Vec<String>,
    /// Every word before the text, the placeholders filled: `["task", "add", "--tier", "standard"]`.
    words: Vec<String>,
}

impl Command {
    fn shown(&self) -> String {
        format!("cf {} \"…\"", self.words.join(" "))
    }

    fn last(&self) -> &str {
        self.name.last().map_or("", String::as_str)
    }
}

/// `cf help`, as a window gets it: all the board's usage.
fn usage() -> String {
    let ran = cf(
        &["help"],
        &[
            ("CONSENSFLOW_URL", "http://127.0.0.1:9"),
            ("CONSENSFLOW_TOKEN", "tok"),
        ],
        "",
    );
    assert_eq!(ran.status.code(), Some(0));
    String::from_utf8_lossy(&ran.stdout).into_owned()
}

/// How a placeholder of the table is filled: `<critical|complex|standard|light>` by its first
/// word, and `<tier>` (what `--tier` takes) by one that is a tier. Any other placeholder stops the
/// test, which then has to be taught it.
fn filled(word: &str, before: Option<&str>) -> String {
    let Some(inner) = word
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
    else {
        return word.to_owned();
    };
    match (inner.split('|').next(), before) {
        (Some(first), _) if inner.contains('|') => first.to_owned(),
        (_, Some("--tier")) => "standard".to_owned(),
        _ => panic!("the table has a placeholder this test cannot fill: {word}"),
    }
}

/// The commands of the table that end in a text: a line of the list that begins `cf `, with the
/// text `"…"` standing outside any `[ ]`. What follows the text is the command's description.
fn commands() -> Vec<Command> {
    usage()
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("cf "))
        .filter_map(|line| {
            let mut depth = 0_i32;
            let mut words: Vec<String> = Vec::new();
            for word in line.split_whitespace() {
                depth += i32::try_from(word.matches('[').count()).expect("a count");
                depth -= i32::try_from(word.matches(']').count()).expect("a count");
                if word == "\"…\"" && depth == 0 {
                    let name: Vec<String> = words
                        .iter()
                        .take_while(|part| {
                            part.chars()
                                .all(|each| each.is_ascii_lowercase() || each == '|')
                        })
                        .cloned()
                        .collect();
                    // A line that names several verbs at once (`task edit|reopen|resume T-3 …
                    // --title "…"`) says what each takes, and each has a line of its own.
                    if name.iter().any(|part| part.contains('|')) {
                        return None;
                    }
                    return Some(Command { name, words });
                }
                let before = words.last().cloned();
                words.push(filled(word, before.as_deref()));
            }
            None
        })
        .collect()
}

/// The words of the table's footer that name the commands taking a file: `(task add, reopen,
/// resume, edit and tell)`, each by the last of its words; or `(every command that takes
/// words)`, each command of the table that takes a text.
fn file_commands() -> BTreeSet<String> {
    let usage = usage();
    let line = usage
        .lines()
        .find(|line| line.contains("which no shell cuts ("))
        .expect("the table's footer names the commands that take a file");
    let named = line
        .split_once("which no shell cuts (")
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(named, _)| named)
        .expect("a list in brackets");
    // Every command of the table that takes a text takes a file too (T-1110).
    if named == "every command that takes words" {
        return commands()
            .iter()
            .map(|command| command.last().to_owned())
            .collect();
    }
    named
        .replace(" and ", ", ")
        .split(", ")
        .filter_map(|each| each.split_whitespace().last())
        .map(str::to_owned)
        .collect()
}

/// What the board received of `args` run with `input` on standard input: the body, and the exit
/// code and the error output for a message when it is not what it should be.
fn posted(args: &[String], input: &str) -> (Option<i32>, Option<Value>, String) {
    let api = scripted(vec![reply(201, the_answer())]);
    let ran = cf(
        args,
        &[("CONSENSFLOW_URL", &api.url), ("CONSENSFLOW_TOKEN", "tok")],
        input,
    );
    let body = api
        .received()
        .into_iter()
        .next()
        .and_then(|got| got.json())
        .and_then(|sent| sent.get("body").cloned());
    (
        ran.status.code(),
        body,
        String::from_utf8_lossy(&ran.stderr).into_owned(),
    )
}

/// Asserts that `args` with `input` posted exactly `text`.
fn assert_posts(command: &Command, route: &str, args: &[String], input: &str, text: &str) {
    let (code, body, err) = posted(args, input);
    assert_eq!(
        (code, body.as_ref().and_then(Value::as_str)),
        (Some(0), Some(text)),
        "{} {route}: the board should have received the text as written; stderr: {err}",
        command.shown()
    );
}

fn with(command: &Command, rest: &[&str]) -> Vec<String> {
    command
        .words
        .iter()
        .cloned()
        .chain(rest.iter().map(|word| (*word).to_owned()))
        .collect()
}

#[test]
fn the_table_has_the_commands_that_take_a_text() {
    // The parser finds them: were it to find none, every test below would pass over nothing.
    let found = commands();
    let names: BTreeSet<String> = found.iter().map(|command| command.name.join(" ")).collect();
    assert!(!names.is_empty(), "the table's text commands: {found:?}");
    // `cf history --find "…"` is a flag's value, and no text.
    assert!(!names.contains("history"), "{names:?}");
    // What the table's footer says takes a file is among them, each.
    for word in file_commands() {
        assert!(
            found.iter().any(|command| command.last() == word),
            "the footer names {word}, which the table has no text command of"
        );
    }
}

#[test]
fn every_command_that_takes_a_text_reads_it_from_standard_input_at_a_dash() {
    for command in commands() {
        for text in TEXTS {
            // What a quoted heredoc gives: the text, and the line break its last line ends in;
            // and what PowerShell and `type` pipe, which ends in a Windows one.
            for ending in ["\n", "\r\n"] {
                let args = with(&command, &["-"]);
                assert_posts(&command, "`-`", &args, &format!("{text}{ending}"), text);
            }
        }
    }
}

#[test]
fn every_command_that_takes_a_text_takes_it_as_one_argument_whole() {
    for command in commands() {
        for text in TEXTS {
            let args = with(&command, &[text]);
            assert_posts(&command, "as one argument", &args, "", text);
        }
    }
}

#[test]
fn every_command_that_the_table_says_takes_a_file_reads_its_text_from_one() {
    let takers = file_commands();
    let mut ran = 0;
    for command in commands()
        .into_iter()
        .filter(|command| takers.contains(command.last()))
    {
        for (text, ending) in TEXTS.into_iter().zip(["\n", "\r\n"]) {
            let folder = tempfile::tempdir().expect("a folder");
            let file = folder.path().join("brief.md");
            // An editor ends the file with a line break, which is not the text's.
            fs::write(&file, format!("{text}{ending}")).expect("the file");
            let args = with(&command, &["--file", &file.to_string_lossy()]);
            assert_posts(&command, "`--file`", &args, "", text);
            ran += 1;
        }
    }
    assert!(
        ran >= takers.len() * TEXTS.len(),
        "{ran} runs for {takers:?}"
    );
}

/// The role cards show the quoted heredoc, `cf task add --tier standard - <<'BRIEF'`: a real
/// shell reads it, and the board gets what the shell was given between the lines.
#[cfg(unix)]
#[test]
fn every_command_that_takes_a_text_gets_it_whole_through_a_shells_quoted_heredoc() {
    let quote = |word: &str| format!("'{}'", word.replace('\'', "'\\''"));
    let bin = common::own_cf().to_string_lossy().into_owned();
    for command in commands() {
        for text in TEXTS {
            let words: Vec<String> = with(&command, &["-"])
                .iter()
                .map(|word| quote(word))
                .collect();
            let script = format!(
                "exec \"$BIN_UNDER_TEST\" {} <<'BRIEF'\n{text}\nBRIEF\n",
                words.join(" ")
            );
            let api = scripted(vec![reply(201, the_answer())]);
            let ran = common::cf_at(
                Path::new("/bin/sh"),
                &["-c", &script],
                &[
                    ("CONSENSFLOW_URL", &api.url),
                    ("CONSENSFLOW_TOKEN", "tok"),
                    ("BIN_UNDER_TEST", &bin),
                ],
                "",
            );
            let body = api
                .received()
                .into_iter()
                .next()
                .and_then(|got| got.json())
                .and_then(|sent| sent.get("body").cloned());
            assert_eq!(
                (ran.status.code(), body.as_ref().and_then(Value::as_str)),
                (Some(0), Some(text)),
                "{} through sh: {}",
                command.shown(),
                String::from_utf8_lossy(&ran.stderr)
            );
        }
    }
}

/// `text` as PowerShell 5.1's `>` writes it: a mark, UTF-16 little-endian, a Windows line break.
fn utf16_with_a_mark(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xfe];
    for unit in format!("{text}\r\n").encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

/// What the board received of `args` run with `input` as bytes on standard input.
fn posted_bytes(args: &[String], input: &[u8]) -> (Option<i32>, Option<Value>, String) {
    let api = scripted(vec![reply(201, the_answer())]);
    let ran = cf_bytes(
        args,
        &[("CONSENSFLOW_URL", &api.url), ("CONSENSFLOW_TOKEN", "tok")],
        input,
    );
    let body = api
        .received()
        .into_iter()
        .next()
        .and_then(|got| got.json())
        .and_then(|sent| sent.get("body").cloned());
    (
        ran.status.code(),
        body,
        String::from_utf8_lossy(&ran.stderr).into_owned(),
    )
}

#[test]
fn every_command_that_takes_a_text_decodes_utf16_with_a_mark_from_a_file_and_from_input() {
    let takers = file_commands();
    for command in commands() {
        for text in TEXTS {
            let bytes = utf16_with_a_mark(text);
            let (code, body, err) = posted_bytes(&with(&command, &["-"]), &bytes);
            assert_eq!(
                (code, body.as_ref().and_then(Value::as_str)),
                (Some(0), Some(text)),
                "{} on standard input: {err}",
                command.shown()
            );
            if !takers.contains(command.last()) {
                continue;
            }
            let folder = tempfile::tempdir().expect("a folder");
            let file = folder.path().join("brief.md");
            fs::write(&file, &bytes).expect("the file");
            let args = with(&command, &["--file", &file.to_string_lossy()]);
            let (code, body, err) = posted_bytes(&args, b"");
            assert_eq!(
                (code, body.as_ref().and_then(Value::as_str)),
                (Some(0), Some(text)),
                "{} --file: {err}",
                command.shown()
            );
        }
    }
}

#[test]
fn every_command_that_takes_a_text_refuses_what_is_no_utf8_and_posts_nothing() {
    let takers = file_commands();
    let mut without_a_mark = Vec::new();
    for unit in "Write the parser.".encode_utf16() {
        without_a_mark.extend_from_slice(&unit.to_le_bytes());
    }
    let refused: [(&str, Vec<u8>); 3] = [
        ("UTF-16 without a mark", without_a_mark),
        ("a Windows code page", b"Intrebarea: \xe9".to_vec()),
        ("a NUL byte", b"Write\0 the parser.".to_vec()),
    ];
    for command in commands() {
        for (what, bytes) in &refused {
            let (code, body, err) = posted_bytes(&with(&command, &["-"]), bytes);
            assert_eq!(
                (code, &body),
                (Some(1), &None),
                "{} on standard input, {what}: {err}",
                command.shown()
            );
            assert!(
                err.starts_with("cf: cannot read standard input: ")
                    && err.contains("Write it as UTF-8")
                    && err.ends_with("Nothing was sent.\n"),
                "{} on standard input, {what}: {err}",
                command.shown()
            );
            if !takers.contains(command.last()) {
                continue;
            }
            let folder = tempfile::tempdir().expect("a folder");
            let file = folder.path().join("brief.md");
            fs::write(&file, bytes).expect("the file");
            let args = with(&command, &["--file", &file.to_string_lossy()]);
            let (code, body, err) = posted_bytes(&args, b"");
            assert_eq!(
                (code, &body),
                (Some(1), &None),
                "{} --file, {what}: {err}",
                command.shown()
            );
            assert!(
                err.starts_with("cf: cannot read ") && err.ends_with("Nothing was sent.\n"),
                "{} --file, {what}: {err}",
                command.shown()
            );
        }
    }
}

/// No function of the board decodes the bytes of a text its own way: `words_of` (`src/board/
/// brief.rs`) is the one that does, for a file and for standard input. A `from_utf8_lossy` in the
/// board's code is a reader that would post what is no text.
#[test]
fn only_words_of_decodes_a_text() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    let mut stack = vec![folder];
    while let Some(folder) = stack.pop() {
        for entry in fs::read_dir(&folder).expect("the sources") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
            if path.extension().is_none_or(|extension| extension != "rs")
                || name.is_some_and(|name| name == "tests.rs" || name.ends_with("_tests.rs"))
            {
                continue;
            }
            for (number, line) in fs::read_to_string(&path)
                .expect("a source")
                .lines()
                .enumerate()
            {
                if line.trim_start().starts_with("#[cfg(test)]") {
                    break;
                }
                if !line.trim_start().starts_with("//") && line.contains("from_utf8_lossy") {
                    found.push(format!("{}:{}", path.display(), number + 1));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "a text is decoded by words_of alone, never lossily: {found:?}"
    );
}

/// The names of the functions of `src/board` that read a text: a call of `text_of` or `brief_of`
/// that is not their own definitions, their uses or a comment, and not in the tests at a file's end.
fn readers() -> BTreeSet<String> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/board");
    let mut found = BTreeSet::new();
    for entry in fs::read_dir(&folder).expect("the board's sources") {
        let path = entry.expect("an entry").path();
        if path.extension().is_none_or(|extension| extension != "rs")
            || path.file_name().is_some_and(|name| name == "tests.rs")
        {
            continue;
        }
        let mut current = String::new();
        for line in fs::read_to_string(&path).expect("a source").lines() {
            let line = line.trim_start();
            if line.starts_with("#[cfg(test)]") {
                break;
            }
            if line.starts_with("//") || line.starts_with("use ") {
                continue;
            }
            if let Some(name) = line
                .strip_prefix("pub fn ")
                .or_else(|| line.strip_prefix("fn "))
                .or_else(|| line.strip_prefix("pub(super) fn "))
            {
                current = name
                    .split(|each: char| !(each.is_alphanumeric() || each == '_'))
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                continue;
            }
            // The two that read, and what calls them from their own bodies.
            if (line.contains("text_of(") || line.contains("brief_of("))
                && !["text_of", "brief_of"].contains(&current.as_str())
            {
                found.insert(current.clone());
            }
        }
    }
    found
}

/// The verbs of `cf task` that move a task, from the source's `MOVES`.
fn moves() -> Vec<String> {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/board/task.rs"))
            .expect("task.rs");
    let list = source
        .split_once("const MOVES: [&str; ")
        .and_then(|(_, rest)| rest.split_once("= ["))
        .and_then(|(_, rest)| rest.split_once("];"))
        .map(|(list, _)| list)
        .expect("the list of the verbs that move a task");
    list.split(',')
        .map(|word| word.trim().trim_matches('"').to_owned())
        .filter(|word| !word.is_empty())
        .collect()
}

#[test]
fn a_function_that_reads_a_text_is_a_command_of_the_table() {
    let commands = commands();
    let moves = moves();
    let readers = readers();
    assert!(!readers.is_empty(), "the sources show no reader of a text");
    for reader in readers {
        let in_the_table = commands.iter().any(|command| command.last() == reader)
            || (reader == "decide_many"
                && commands
                    .iter()
                    .any(|command| command.name == ["task", "accept"]))
            || (reader == "moved"
                && commands.iter().any(|command| {
                    command.name.first().is_some_and(|first| first == "task")
                        && moves.iter().any(|verb| command.last() == verb)
                }));
        assert!(
            in_the_table,
            "`{reader}` in src/board reads a text, and no command of the table (cf help) ends in \
             a text for it: add its line to usage.txt, so that it is run here"
        );
    }
}

#[test]
fn several_decisions_take_words_from_input_and_a_file() {
    let folder = tempfile::tempdir().expect("a folder");
    let path = folder.path().join("checked words.md");
    fs::write(&path, "Checked `both`.\r\n").expect("the file");
    let path = path.to_string_lossy();
    for (args, input) in [
        (
            vec!["task", "accept", "T-3", "T-4", "-"],
            "Checked `both`.\n",
        ),
        (vec!["task", "accept", "T-3", "T-4", "--file", &path], ""),
    ] {
        let api = scripted(vec![reply(
            200,
            json!({"tasks":[{"task":{"number":3,"state":"accepted"}}, {"task":{"number":4,"state":"accepted"}}]}),
        )]);
        let ran = cf(
            &args,
            &[("CONSENSFLOW_URL", &api.url), ("CONSENSFLOW_TOKEN", "tok")],
            input,
        );
        assert_eq!(
            ran.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&ran.stderr)
        );
        assert_eq!(
            api.received()[0].json(),
            Some(json!({"action":"accept", "tasks":[3,4], "body":"Checked `both`."}))
        );
    }
}
