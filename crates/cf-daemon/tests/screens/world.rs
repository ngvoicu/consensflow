//! The folder a trace's test made (`«root»`): the files it put there, what is in it
//! before and after an exchange, and how it is spelled on this machine.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

/// A file as a trace has it: its text, and whether it is a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub text: String,
    pub executable: bool,
}

/// Every file of the folder by its path under it, `/` between the parts.
pub type Snapshot = BTreeMap<String, File>;

/// The folder, in a temporary place that goes with it.
pub struct Root {
    dir: tempfile::TempDir,
}

/// `«root»` and the path that follows it, as far as a path's own characters go.
static ROOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new("«root»([A-Za-z0-9._/-]*)").unwrap());

/// A time a writer stamped from its clock.
static STAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z").unwrap());

/// The time a player gives where a trace says `«now»`.
const GIVEN: &str = "2026-01-01T00:00:00.000Z";

impl Root {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// `text` with every `«root»` and the path after it written as this machine
    /// writes it: with its own separator, and for text that is JSON, escaped as
    /// JSON escapes it (a Windows path has backslashes).
    pub fn expand(&self, text: &str, json: bool) -> String {
        ROOTED
            .replace_all(text, |found: &regex::Captures<'_>| {
                let path = found[1]
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .fold(self.path().to_path_buf(), |path, part| path.join(part));
                let path = path.to_string_lossy().into_owned();
                if json {
                    path.replace('\\', "\\\\").replace('"', "\\\"")
                } else {
                    path
                }
            })
            .into_owned()
    }

    /// The file at `path` (under the folder, `/` between the parts).
    pub fn file(&self, path: &str) -> PathBuf {
        path.split('/')
            .fold(self.path().to_path_buf(), |file, part| file.join(part))
    }

    /// Puts the files a `world` step names in place: `null` takes one away. A
    /// program is the script the trace has, made executable; on Windows, where
    /// `harness_path` finds a `.cmd`, it is the shape of an npm shim.
    pub fn put(&self, files: &Map<String, Value>) {
        for (path, file) in files {
            let target = self.file(path);
            if file.is_null() {
                let _ = fs::remove_file(&target);
                continue;
            }
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            let executable = file["executable"].as_bool().unwrap_or(false);
            if executable && cfg!(windows) {
                cf_harness::testing::fake_window_executable(&target);
                continue;
            }
            let text = file["text"].as_str().unwrap();
            fs::write(&target, text.replace("«now»", GIVEN)).unwrap();
            #[cfg(unix)]
            if executable {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
    }

    /// What is in the folder now.
    pub fn snapshot(&self) -> Snapshot {
        let mut found = Snapshot::new();
        walk(self.path(), "", &mut found);
        found
    }
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
pub fn is(file: Option<&File>, recorded: &Value, root: &Root) -> bool {
    match (file, recorded) {
        (None, Value::Null) => true,
        (Some(file), Value::Object(recorded)) => {
            let text = root.expand(recorded["text"].as_str().unwrap(), false);
            let executable = recorded
                .get("executable")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            masked(&file.text) == masked(&text) && (cfg!(windows) || file.executable == executable)
        }
        _ => false,
    }
}
