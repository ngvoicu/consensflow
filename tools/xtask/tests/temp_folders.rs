//! A temporary folder is the last field of what owns it (T-1333). A struct drops its fields in
//! the order they are written, and a `TempDir` that goes before the ledger, the log or the trace
//! that has a file open in it asks the system to remove a folder with files open: Unix does,
//! Windows will not, and `TempDir` says nothing of it. The folder stays, with its `consensflow.db`
//! in it, for good: zeewin2 held 4,416 `.tmpXXXXXX` folders and 1,075 `cf-dispatch-XXXXXX` in the
//! eight days after it was cleaned, the greater part of them from the daemon's tests, whose
//! `Scene` and `Rig` had the folder first (2026-11-08, measured test by test).
//!
//! The rule reads the Rust of the workspace (its comments are not read): of the fields of a
//! struct with a body, the ones whose type is a `TempDir` come after every one that is not. The
//! contract rig of the daemon has said so in a comment since it was written.
#![allow(clippy::disallowed_methods, clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::checkout;

/// The folders of the sources.
const LOOKED_IN: [&str; 3] = ["crates", "tools", "app/src-tauri"];

/// The folders no source of the repository is in: what is fetched and what is built.
const NOT_THE_REPOSITORYS: [&str; 5] = [".git", "node_modules", "target", ".cache", "resources"];

/// This file names the shapes it refuses, as data.
const THIS: &str = "temp_folders.rs";

/// Every Rust source under `folder` (a link is not followed).
fn sources(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type().unwrap();
        if kind.is_dir() && !NOT_THE_REPOSITORYS.contains(&name.as_str()) {
            sources(&entry.path(), found);
        } else if kind.is_file() && name.ends_with(".rs") && name != THIS {
            found.push(entry.path());
        }
    }
}

/// A struct with a body, where its name is, and its fields' types in the order they are written.
struct Struct {
    line: usize,
    name: String,
    fields: Vec<String>,
}

/// `source` without its comments, line ends kept so that a line is still the line.
fn without_comments(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let (mut text, mut at) = (String::new(), 0);
    while at < chars.len() {
        match chars[at] {
            '/' if chars.get(at + 1) == Some(&'/') => {
                while at < chars.len() && chars[at] != '\n' {
                    at += 1;
                }
            }
            '/' if chars.get(at + 1) == Some(&'*') => {
                let mut depth = 0;
                while at < chars.len() {
                    if chars[at] == '/' && chars.get(at + 1) == Some(&'*') {
                        depth += 1;
                        at += 2;
                    } else if chars[at] == '*' && chars.get(at + 1) == Some(&'/') {
                        depth -= 1;
                        at += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        if chars[at] == '\n' {
                            text.push('\n');
                        }
                        at += 1;
                    }
                }
            }
            c => {
                text.push(c);
                at += 1;
            }
        }
    }
    text
}

/// The structs with a body in `source`, each with the types of its fields.
fn structs(source: &str) -> Vec<Struct> {
    let text = without_comments(source);
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let before = at.checked_sub(1).and_then(|b| chars.get(b));
        let starts = chars[at..].starts_with(&['s', 't', 'r', 'u', 'c', 't', ' '])
            && !before.is_some_and(|c| c.is_alphanumeric() || *c == '_');
        if !starts {
            at += 1;
            continue;
        }
        let line = chars[..at].iter().filter(|c| **c == '\n').count() + 1;
        let mut end = at + "struct ".len();
        let name: String = chars[end..]
            .iter()
            .take_while(|c| c.is_alphanumeric() || **c == '_')
            .collect();
        end += name.len();
        // Generics and a `where` clause come before the body; a `;` or a `(` is no body.
        let mut angle = 0usize;
        while end < chars.len() {
            match chars[end] {
                '<' => angle += 1,
                '>' if angle > 0 && chars.get(end.wrapping_sub(1)) != Some(&'-') => angle -= 1,
                '{' if angle == 0 => break,
                ';' | '(' if angle == 0 => {
                    end = chars.len();
                    break;
                }
                _ => {}
            }
            end += 1;
        }
        if name.is_empty() || end >= chars.len() {
            at += 1;
            continue;
        }
        let mut depth = 0usize;
        let mut body = String::new();
        let mut fields = Vec::new();
        let mut stop = end;
        for (i, c) in chars.iter().enumerate().skip(end) {
            stop = i;
            match c {
                '{' | '(' | '[' | '<' => {
                    depth += 1;
                    if depth > 1 {
                        body.push(*c);
                    }
                }
                '}' | ')' | ']' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    body.push(*c);
                }
                '>' if chars.get(i.wrapping_sub(1)) != Some(&'-') => {
                    depth = depth.saturating_sub(1);
                    body.push(*c);
                }
                ',' if depth == 1 => fields.push(std::mem::take(&mut body)),
                c => body.push(*c),
            }
        }
        fields.push(body);
        let fields = fields
            .iter()
            .filter_map(|field| field_type(field))
            .collect();
        found.push(Struct { line, name, fields });
        at = stop.max(at + 1);
    }
    found
}

/// The type of a field written as `#[attribute] pub(crate) name: Type`, if it is one.
fn field_type(field: &str) -> Option<String> {
    let mut rest = field.trim();
    while rest.starts_with("#[") {
        rest = rest[rest.find(']')? + 1..].trim_start();
    }
    let (name, kind) = rest.split_once(':')?;
    let name = name.split_whitespace().last()?;
    (name.chars().all(|c| c.is_alphanumeric() || c == '_') && !kind.trim().is_empty())
        .then(|| kind.trim().to_owned())
}

/// The structs of `source` that have a `TempDir` before a field that is none.
fn out_of_order(source: &str) -> Vec<Struct> {
    structs(source)
        .into_iter()
        .filter(|one| {
            let is_dir = |kind: &String| kind.contains("TempDir");
            one.fields
                .iter()
                .position(is_dir)
                .is_some_and(|first| one.fields[first..].iter().any(|kind| !is_dir(kind)))
        })
        .collect()
}

#[test]
fn a_temporary_folder_before_a_field_is_found_and_one_after_the_rest_is_not() {
    let wrong = [
        "struct Scene { _home: tempfile::TempDir, context: Rc<Context>, }",
        "pub struct Rig {\n    pub(crate) home: TempDir,\n    #[allow(dead_code)]\n    spawn: Rc<Spawn>,\n}",
        "struct A<T: Into<Vec<u8>>> { dir: tempfile::TempDir, map: HashMap<String, Vec<u8>>, t: T }",
        "struct B { folder: Option<tempfile::TempDir>, ledger: RefCell<Ledger> }",
        "struct C { dir: TempDir, other: TempDir, file: File }",
    ];
    for source in wrong {
        assert_eq!(out_of_order(source).len(), 1, "{source}");
    }
    let right = [
        "struct Scene { context: Rc<Context>, _home: tempfile::TempDir }",
        "pub struct Rig {\n    spawn: Rc<Spawn>,\n    /// Last, so that the folder goes after what has files open in it.\n    pub home: tempfile::TempDir,\n}",
        "struct Only { dir: TempDir }",
        "struct Two { ledger: Ledger, one: TempDir, two: tempfile::TempDir }",
        "struct Unit;",
        "struct Pair(TempDir, Ledger);",
        "// struct Commented { dir: TempDir, ledger: Ledger }\nstruct Fine { ledger: Ledger, dir: TempDir }",
        "struct Fn { call: fn(i32) -> Vec<u8>, dir: TempDir }",
    ];
    for source in right {
        assert!(out_of_order(source).is_empty(), "{source}");
    }
    assert_eq!(structs("\n\nstruct Late { a: u8, b: u16 }")[0].line, 3);
}

#[test]
fn no_struct_of_the_workspace_has_a_temporary_folder_before_a_field_with_a_file_open_in_it() {
    let (root, mut found) = (checkout(), Vec::new());
    for folder in LOOKED_IN {
        sources(&root.join(folder), &mut found);
    }
    let mut refused = Vec::new();
    for file in found {
        let source = String::from_utf8_lossy(&fs::read(&file).unwrap()).into_owned();
        for one in out_of_order(&source) {
            let named = file.strip_prefix(&root).unwrap_or(&file).display();
            refused.push(format!("{named}:{}: struct {}", one.line, one.name));
        }
    }
    refused.sort();
    assert!(
        refused.is_empty(),
        "a temporary folder goes last in what owns it, or Windows will not remove it from under the files its fields have open (and `TempDir` does not say so); move it to the end of the struct:\n{}",
        refused.join("\n")
    );
}
