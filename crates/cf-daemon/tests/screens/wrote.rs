//! The files the screens write (`wrote` in a trace): what is in the world's
//! folder before and after an exchange, and how each file that changed is held
//! to what Node's recorded, its stamps being the clock's and so masked.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::world::World;

/// A file as a trace has it: its text, and whether it is a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub text: String,
    pub executable: bool,
}

/// Every file of the folder by its path under it, `/` between the parts.
pub type Snapshot = BTreeMap<String, File>;

/// A time a writer stamped from its clock.
static STAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z").unwrap());

/// What is in the world's folder now.
pub fn snapshot(world: &World) -> Snapshot {
    let mut found = Snapshot::new();
    walk(world.path(), "", &mut found);
    found
}

fn walk(folder: &Path, under: &str, found: &mut Snapshot) {
    for entry in fs::read_dir(folder).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = if under.is_empty() {
            name
        } else {
            format!("{under}/{name}")
        };
        if entry.file_type().unwrap().is_dir() {
            walk(&entry.path(), &path, found);
            continue;
        }
        let text = fs::read(entry.path())
            .map(|bytes| String::from_utf8(bytes).unwrap_or_else(|_| panic!("{path} is no UTF-8")))
            .unwrap();
        found.insert(
            path,
            File {
                text,
                executable: is_executable(&entry.path()),
            },
        );
    }
}

#[cfg(unix)]
fn is_executable(file: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(file).unwrap().permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_file: &Path) -> bool {
    false
}

/// What each file that is not as it was is, as it was and as it is: none for
/// one that was not there, or is not now.
pub type Changes = BTreeMap<String, (Option<File>, Option<File>)>;

/// The files `after` has that `before` did not have or had otherwise, and those it lost.
pub fn changes(before: &Snapshot, after: &Snapshot) -> Changes {
    before
        .keys()
        .chain(after.keys())
        .filter(|path| before.get(*path) != after.get(*path))
        .map(|path| {
            (
                path.clone(),
                (before.get(path).cloned(), after.get(path).cloned()),
            )
        })
        .collect()
}

/// `text` as a comparison reads it: every time the writer stamped is `«now»`.
pub fn masked(text: &str) -> String {
    STAMP.replace_all(text, "«now»").into_owned()
}

/// Whether `file` is what a trace says one is: `null` for none, else its
/// text, stamps masked (a file's `executable` is a POSIX matter).
pub fn is(file: Option<&File>, recorded: &Value, world: &World) -> bool {
    match (file, recorded) {
        (None, Value::Null) => true,
        (Some(file), Value::Object(recorded)) => {
            let text = world.expand(recorded["text"].as_str().unwrap(), false);
            let executable = recorded
                .get("executable")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            masked(&file.text) == masked(&text) && (cfg!(windows) || file.executable == executable)
        }
        _ => false,
    }
}
