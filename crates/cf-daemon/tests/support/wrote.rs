//! The files an exchange or an operation writes (`wrote` in a trace): what is in
//! the world's folder before and after it, and how each file that changed is
//! held to what Node recorded. A step with no `wrote` changed nothing: a file
//! that appears, goes or is rewritten without the trace saying so fails it.
//!
//! A file is read as the recorder reads one (`tests/goldens/daemon/world.mjs`):
//! its text, with a roster's stamps named `«now»` and nothing else of it or of
//! any other file masked, and whether it is a program. A roster that is
//! rewritten with no change but its stamps is not changed, for the recorder
//! and so for the player.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::support::compare::differs;
use crate::world::World;

/// A file as a trace has it: its text, a roster's stamps masked, and whether it
/// is a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    text: String,
    executable: bool,
}

/// Every file of the folder by its path under it, `/` between the parts.
pub type Snapshot = BTreeMap<String, File>;

/// What the recorder masks in a roster file: the time of a `createdAt` or of an
/// `updatedAt`, which the writer took from its own clock (`maskStamps`,
/// `tests/goldens/daemon/mask.mjs`, which takes whatever the field holds: here
/// only a time is the clock's, so a field that holds another text stays as it
/// is, and is held to the `«now»` Node recorded).
static STAMPED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"("(?:createdAt|updatedAt)": )"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z""#)
        .unwrap()
});

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
        let text = String::from_utf8(fs::read(entry.path()).unwrap())
            .unwrap_or_else(|_| panic!("{path} is no UTF-8"));
        let file = File {
            text: masked(&path, &text),
            executable: is_executable(&entry.path()),
        };
        found.insert(path, file);
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

/// The text of the file at `path` as the recorder writes it down: a roster
/// (`agents.json`, wherever it lies) has the time of each `createdAt` and
/// `updatedAt` read as `«now»`, and no other text of it, and no other file,
/// has anything masked.
pub fn masked(path: &str, text: &str) -> String {
    if path.rsplit('/').next() == Some("agents.json") {
        STAMPED.replace_all(text, r#"${1}"«now»""#).into_owned()
    } else {
        text.to_owned()
    }
}

/// Why the files that changed between `before` and `after` are not the ones
/// `step` says it `wrote`, each as it was and as it became: none when they are.
pub fn held(step: &Value, before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let changed: BTreeMap<&str, (Option<&File>, Option<&File>)> = before
        .keys()
        .chain(after.keys())
        .filter(|path| before.get(*path) != after.get(*path))
        .map(|path| (path.as_str(), (before.get(path), after.get(path))))
        .collect();
    let recorded = step.get("wrote").and_then(Value::as_object);
    let said: BTreeSet<&str> = recorded
        .into_iter()
        .flatten()
        .map(|(path, _)| path.as_str())
        .collect();
    let made: BTreeSet<&str> = changed.keys().copied().collect();
    if made != said {
        return vec![format!("the files it changed: {made:?}, Node {said:?}")];
    }
    let mut problems = Vec::new();
    for (path, (was, now)) in changed {
        let sides = &step["wrote"][path];
        for (side, file) in [("before", was), ("after", now)] {
            problems.extend(not_as_recorded(path, side, file, &sides[side]));
        }
    }
    problems
}

/// Why the file at `path` is not what a trace says it was on `side` of the
/// step: `null` for none, else its text, and its program bit, which is a POSIX
/// matter.
fn not_as_recorded(
    path: &str,
    side: &str,
    file: Option<&File>,
    recorded: &Value,
) -> Option<String> {
    let what = format!("{path} {side}");
    match (file, recorded) {
        (None, Value::Null) => None,
        (None, _) => Some(format!("{what}: not there, Node has it")),
        (Some(_), Value::Null) => Some(format!("{what}: there, Node has none")),
        (Some(file), recorded) => {
            let text = recorded["text"]
                .as_str()
                .expect("a file of text: no trace holds another");
            let executable = recorded["executable"].as_bool().unwrap_or(false);
            differs(&what, &file.text, text).or_else(|| {
                (!cfg!(windows) && file.executable != executable)
                    .then(|| format!("{what}: a program: {}, Node {executable}", file.executable))
            })
        }
    }
}
