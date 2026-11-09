//! A temporary folder is the last field of what owns it (T-1333). A struct drops its fields in
//! the order they are written, and a `TempDir` that goes before the ledger, the log or the trace
//! that has a file open in it asks the system to remove a folder with files open: Unix does,
//! Windows will not, and `TempDir` says nothing of it. The folder stays, with its `consensflow.db`
//! in it, for good: zeewin2 held 4,416 `.tmpXXXXXX` folders and 1,075 `cf-dispatch-XXXXXX` in the
//! eight days after it was cleaned, the greater part of them from the daemon's tests, whose
//! `Scene` and `Rig` had the folder first (2026-11-08, measured test by test).
//!
//! The rule reads the Rust of the workspace (its comments are not read): of the fields of a
//! struct, the ones that own a temporary folder come after every one that does not. A field owns
//! one where its type is a `TempDir`, or names a newtype of one in its crate (`struct Folder(
//! TempDir)`, as the evals have), however many newtypes deep. The contract rig of the daemon has
//! said so in a comment since it was written.
#![allow(clippy::disallowed_methods, clippy::unwrap_used)]

mod common;

use std::collections::BTreeSet;
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

/// A struct with a body or a tuple, where its name is, and its fields' types in the order they are
/// written.
struct Struct {
    line: usize,
    name: String,
    fields: Vec<String>,
    /// The crate it is in: newtypes are told apart by it, since many crates have a `Folder`.
    krate: String,
    /// Whether it is a tuple struct.
    tuple: bool,
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
                ';' if angle == 0 => {
                    end = chars.len();
                    break;
                }
                '(' if angle == 0 => break,
                _ => {}
            }
            end += 1;
        }
        if name.is_empty() || end >= chars.len() {
            at += 1;
            continue;
        }
        let tuple = chars[end] == '(';
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
            .filter_map(|field| field_type(field, tuple))
            .collect();
        found.push(Struct {
            line,
            name,
            fields,
            krate: String::new(),
            tuple,
        });
        at = stop.max(at + 1);
    }
    found
}

/// The type of a field written as `#[attribute] pub(crate) name: Type`, or as `pub Type` where
/// the struct is a tuple's, if it is one.
fn field_type(field: &str, tuple: bool) -> Option<String> {
    let mut rest = field.trim();
    while rest.starts_with("#[") {
        rest = rest[rest.find(']')? + 1..].trim_start();
    }
    if tuple {
        let rest = rest.strip_prefix("pub").map_or(rest, |after| {
            let after = after.trim_start();
            if after.starts_with('(') {
                after[after.find(')').map_or(0, |close| close + 1)..].trim_start()
            } else {
                after
            }
        });
        return (!rest.is_empty()).then(|| rest.to_owned());
    }
    let (name, kind) = rest.split_once(':')?;
    let name = name.split_whitespace().last()?;
    (name.chars().all(|c| c.is_alphanumeric() || c == '_') && !kind.trim().is_empty())
        .then(|| kind.trim().to_owned())
}

/// The names `kind` is written with.
fn names(kind: &str) -> impl Iterator<Item = &str> {
    kind.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
}

/// Whether a field of type `kind` in crate `krate` owns a temporary folder: it is one, or is
/// (or is held by an `Rc`, a `Box`, an `Option`) a newtype of one in that crate.
fn owns(kind: &str, krate: &str, owners: &BTreeSet<(String, String)>) -> bool {
    let words: Vec<&str> = names(kind).collect();
    if words.contains(&"TempDir") {
        return true;
    }
    let held = ["Rc", "Arc", "Box", "Option", "RefCell", "Cell"];
    let mut first = words
        .iter()
        .copied()
        .skip_while(|word| matches!(*word, "mut" | "pub" | "crate"));
    let outer = first.next();
    let named = if outer.is_some_and(|word| held.contains(&word)) {
        first.next()
    } else {
        outer
    };
    named.is_some_and(|word| owners.contains(&(krate.to_owned(), word.to_owned())))
}

/// The newtypes of a temporary folder: tuple structs of one field that is one, or another
/// newtype, by crate and name. (A named struct of one field is a rig as often as a wrapper, and
/// its name is the name of other rigs in the crate.)
fn owners(all: &[Struct]) -> BTreeSet<(String, String)> {
    let mut owners = BTreeSet::new();
    loop {
        let before = owners.len();
        for one in all {
            if one.tuple && one.fields.len() == 1 && owns(&one.fields[0], &one.krate, &owners) {
                owners.insert((one.krate.clone(), one.name.clone()));
            }
        }
        if owners.len() == before {
            return owners;
        }
    }
}

/// The structs of `all` that have a field that owns a temporary folder before one that does not.
fn out_of_order<'a>(all: &'a [Struct], owners: &BTreeSet<(String, String)>) -> Vec<&'a Struct> {
    all.iter()
        .filter(|one| {
            let is_dir = |kind: &String| owns(kind, &one.krate, owners);
            one.fields
                .iter()
                .position(is_dir)
                .is_some_and(|first| one.fields[first..].iter().any(|kind| !is_dir(kind)))
        })
        .collect()
}

/// [`out_of_order`] for the structs of one `source` alone.
fn alone(source: &str) -> Vec<String> {
    let all = structs(source);
    let owners = owners(&all);
    out_of_order(&all, &owners)
        .iter()
        .map(|one| one.name.clone())
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
        // A wrapper of a folder is one, and so is what holds the wrapper.
        "struct Folder(tempfile::TempDir);\nstruct Scene { _folder: Folder, db: Connection }",
        "struct Folder(pub tempfile::TempDir);\nstruct Scene { folder: Folder, db: Connection }",
        "struct Home(TempDir);\nstruct Folder(Home);\nstruct Rig { home: Folder, bridge: Bridge }",
        "struct Pair(TempDir, Ledger);",
    ];
    for source in wrong {
        assert_eq!(alone(source).len(), 1, "{source}");
    }
    let right = [
        "struct Scene { context: Rc<Context>, _home: tempfile::TempDir }",
        "pub struct Rig {\n    spawn: Rc<Spawn>,\n    /// Last, so that the folder goes after what has files open in it.\n    pub home: tempfile::TempDir,\n}",
        "struct Only { dir: TempDir }",
        "struct Two { ledger: Ledger, one: TempDir, two: tempfile::TempDir }",
        "struct Unit;",
        "struct Folder(tempfile::TempDir);\nstruct Scene { db: Connection, folder: Folder }",
        "struct Pair(Ledger, TempDir);",
        "// struct Commented { dir: TempDir, ledger: Ledger }\nstruct Fine { ledger: Ledger, dir: TempDir }",
        "struct Fn { call: fn(i32) -> Vec<u8>, dir: TempDir }",
    ];
    for source in right {
        assert!(alone(source).is_empty(), "{source}");
    }
    assert_eq!(structs("\n\nstruct Late { a: u8, b: u16 }")[0].line, 3);
}

#[test]
fn no_struct_of_the_workspace_has_a_temporary_folder_before_a_field_with_a_file_open_in_it() {
    let (root, mut found) = (checkout(), Vec::new());
    for folder in LOOKED_IN {
        sources(&root.join(folder), &mut found);
    }
    found.sort();
    let mut all = Vec::new();
    let mut at = Vec::new();
    for file in &found {
        let source = String::from_utf8_lossy(&fs::read(file).unwrap()).into_owned();
        let named = file.strip_prefix(&root).unwrap_or(file);
        let krate = named
            .components()
            .take(2)
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        for mut one in structs(&source) {
            one.krate.clone_from(&krate);
            at.push(named.display().to_string());
            all.push(one);
        }
    }
    let owners = owners(&all);
    let mut refused: Vec<String> = out_of_order(&all, &owners)
        .iter()
        .map(|one| {
            let index = all
                .iter()
                .position(|other| std::ptr::eq(other, *one))
                .unwrap();
            format!("{}:{}: struct {}", at[index], one.line, one.name)
        })
        .collect();
    refused.sort();
    assert!(
        refused.is_empty(),
        "a temporary folder (or what owns one) goes last in what owns it, or Windows will not remove it from under the files its fields have open (and `TempDir` does not say so); move it to the end of the struct:\n{}",
        refused.join("\n")
    );
}
