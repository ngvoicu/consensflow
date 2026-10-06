//! The Rust `cf` held to what Node's `bin/cf.mjs` said and did, case by case:
//! tests/goldens/cli/ (`npm run goldens:cli`, one set a platform, the system's
//! own recorded on it) ran each case against Node, in a folder of its own,
//! with the clock fixed and the environment given in full, and wrote down the
//! files of the folder before and after, the output, the error output and the
//! exit code. Here each case is made again in a folder of its own, the binary run
//! as the case says with the switch on (`CONSENSFLOW_DAEMON=native`), and all of
//! it compared byte for byte. The format is in tests/goldens/cli/FORMAT.md.
//!
//! The binary cannot be given the clock Node was: what it stamps an agent with
//! is read as the instant the recorder fixed once it is known to be one instant,
//! the run's own. A difference Rust keeps from Node on purpose is recorded with
//! what Rust says (`kept`), and is held to that. `setup` and `doctor` are
//! recorded but not played: they wait for the launcher and the stale hooks
//! (`NOT_PORTED`), and until then `cf` hands them to Node's sources.

// The goldens' own reading and the process the tests start: a failure in
// either is the test's.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    clippy::too_many_lines
)]

use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};

use cf_base::time::{iso, parse, Clock, SystemClock};
use serde_json::Value;

/// The name of this system's goldens: Windows' or, as every other system the
/// tests run on, macOS's.
const PLATFORM: &str = if cfg!(windows) { "win32" } else { "darwin" };

/// The verbs recorded and not played yet.
const NOT_PORTED: [&str; 2] = ["setup", "doctor"];

/// How many cases there are at least: a file with fewer has lost some.
const AT_LEAST: usize = 350;

/// How far from the run's own time an instant may be, in milliseconds, and still be the one it stamped.
const MARGIN_MS: i64 = 5_000;

fn golden() -> Value {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/goldens")
        .join(format!("cli.{PLATFORM}.json"));
    let contents = fs::read_to_string(&file).unwrap_or_else(|error| {
        panic!(
            "{}: {error}: record it on this system with `npm run goldens:cli`",
            file.display()
        )
    });
    serde_json::from_str(&contents).unwrap()
}

/// A path as Node writes one: without the prefix Windows gives a resolved one.
fn plain(path: &str) -> String {
    if let Some(share) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{share}");
    }
    path.strip_prefix(r"\\?\").unwrap_or(path).to_owned()
}

/// A text as the recording writes it: the folder as `$ROOT`, the version as `$VERSION`.
fn normalize(text: &str, root: &str) -> String {
    text.replace(root, "$ROOT")
        .replace(env!("CARGO_PKG_VERSION"), "$VERSION")
}

/// The folder as the case's `before` says, made.
fn make(root: &Path, before: &Value) {
    let name = plain(&root.to_string_lossy());
    for entry in before.as_array().unwrap() {
        if let Some(dir) = entry["dir"].as_str() {
            fs::create_dir_all(root.join(dir)).unwrap();
            continue;
        }
        let path = root.join(entry["path"].as_str().unwrap());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            entry["text"].as_str().unwrap().replace("$ROOT", &name),
        )
        .unwrap();
        #[cfg(unix)]
        if entry["executable"].as_bool() == Some(true) {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
}

/// The folder's entries as the recorder lists them: each file with its text,
/// each folder with nothing in it, by path.
fn listing(root: &Path, at: &Path, found: &mut Vec<Value>) {
    let mut items: Vec<_> = fs::read_dir(at).unwrap().map(Result::unwrap).collect();
    items.sort_by_key(fs::DirEntry::file_name);
    let named = |path: &Path| {
        path.strip_prefix(root)
            .unwrap()
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    };
    if items.is_empty() && at != root {
        found.push(serde_json::json!({ "dir": named(at) }));
    }
    for item in items {
        let path = item.path();
        if path.is_dir() {
            listing(root, &path, found);
            continue;
        }
        let text = String::from_utf8_lossy(&fs::read(&path).unwrap()).into_owned();
        let mut entry = serde_json::json!({
            "path": named(&path),
            "text": normalize(&text, &plain(&root.to_string_lossy())),
        });
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if fs::metadata(&path).unwrap().permissions().mode() & 0o111 != 0 {
                entry["executable"] = Value::Bool(true);
            }
        }
        found.push(entry);
    }
}

/// `text` with each `createdAt` and `updatedAt` that holds an instant of the
/// window, as `toISOString` writes one, written as `clock`: the time the run
/// stamped, which the recorder fixed for Node. The instants are put in `seen`.
fn unstamp(text: &str, window: (i64, i64), clock: &str, seen: &mut BTreeSet<i64>) -> String {
    const KEYS: [&str; 2] = ["\"createdAt\": \"", "\"updatedAt\": \""];
    let mut written = String::new();
    let mut rest = text;
    while let Some((at, len)) = KEYS
        .iter()
        .filter_map(|key| rest.find(key).map(|at| (at, key.len())))
        .min()
    {
        written.push_str(&rest[..at + len]);
        rest = &rest[at + len..];
        let end = rest.find('"').unwrap_or(rest.len());
        let value = &rest[..end];
        match parse(value).filter(|ms| iso(*ms) == value && (window.0..=window.1).contains(ms)) {
            Some(ms) => {
                seen.insert(ms);
                written.push_str(clock);
            }
            None => written.push_str(value),
        }
        rest = &rest[end..];
    }
    written.push_str(rest);
    written
}

/// What the binary did with one case.
struct Ran {
    stdout: Option<String>,
    stderr: String,
    code: Option<i32>,
    after: Vec<Value>,
    /// The instants the run stamped an agent with, which were more than one.
    stamped: BTreeSet<i64>,
}

/// The binary run as the case says, in `root`: with exactly the environment the
/// case gives, and the switch on.
fn spawn(case: &Value, root: &Path) -> (Output, Option<String>) {
    let name = plain(&root.to_string_lossy());
    let mut command = Command::new(env!("CARGO_BIN_EXE_cf"));
    command.env_clear();
    for (variable, value) in case["env"].as_object().unwrap() {
        command.env(variable, value.as_str().unwrap().replace("$ROOT", &name));
    }
    command.env("CONSENSFLOW_DAEMON", "native");
    if cfg!(windows) {
        if let Some(system) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system);
        }
    }
    command.args(
        case["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|word| word.as_str().unwrap()),
    );
    match case["pipe"].as_str() {
        // Nobody reads the output: the pipe's reading end is closed before the
        // binary starts to write.
        Some("closed") => {
            let (reader, writer) = std::io::pipe().unwrap();
            drop(reader);
            let ran = command
                .stdin(Stdio::null())
                .stdout(writer)
                .stderr(Stdio::piped())
                .output()
                .unwrap();
            (ran, None)
        }
        // `cf … | head -1`: the output is read up to its first line, and
        // closed.
        Some("first-line") => {
            let mut child = command
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut first = String::new();
            let mut reader = BufReader::new(child.stdout.take().unwrap());
            reader.read_line(&mut first).unwrap();
            drop(reader);
            (child.wait_with_output().unwrap(), Some(first))
        }
        _ => {
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut stdin = child.stdin.take().unwrap();
            // A command that reads no input may have ended before it is written.
            let _ = stdin.write_all(case["stdin"].as_str().unwrap_or_default().as_bytes());
            drop(stdin);
            (child.wait_with_output().unwrap(), None)
        }
    }
}

fn run(case: &Value, clock: &str) -> Ran {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    make(&root, &case["before"]);
    let name = plain(&root.to_string_lossy());
    let started = SystemClock.now_ms();
    let (output, first) = spawn(case, &root);
    let window = (started - MARGIN_MS, SystemClock.now_ms() + MARGIN_MS);
    let mut stamped = BTreeSet::new();
    let mut after = Vec::new();
    listing(&root, &root, &mut after);
    // By path as a whole, as the recorder sorts them, and not folder by folder.
    after.sort_by_key(|entry| {
        entry["path"]
            .as_str()
            .or_else(|| entry["dir"].as_str())
            .unwrap_or_default()
            .to_owned()
    });
    for entry in &mut after {
        let is_roster = entry["path"]
            .as_str()
            .is_some_and(|path| path.ends_with("agents.json"));
        if is_roster {
            let text = entry["text"].as_str().unwrap();
            entry["text"] = Value::String(unstamp(text, window, clock, &mut stamped));
        }
    }
    let stdout = if case["pipe"].as_str() == Some("closed") {
        None
    } else if let Some(first) = first {
        Some(first)
    } else {
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    };
    Ran {
        stdout: stdout.map(|text| normalize(&text, &name)),
        stderr: normalize(&String::from_utf8_lossy(&output.stderr), &name),
        code: output.status.code(),
        after,
        stamped,
    }
}

/// What Node said, or what Rust is recorded to say where it keeps a difference.
struct Expected<'a> {
    stdout: Option<&'a str>,
    stderr: &'a str,
    code: i64,
    after: &'a Value,
}

fn expected(case: &Value) -> Expected<'_> {
    match case.get("kept") {
        Some(kept) => {
            let rust = &kept["rust"];
            Expected {
                stdout: rust["stdout"].as_str(),
                stderr: rust["stderr"].as_str().unwrap(),
                code: rust["code"].as_i64().unwrap(),
                after: if rust["after"].as_str() == Some("before") {
                    &case["before"]
                } else {
                    &case["after"]
                },
            }
        }
        None => Expected {
            stdout: case["stdout"].as_str(),
            stderr: case["stderr"].as_str().unwrap(),
            code: case["code"].as_i64().unwrap(),
            after: &case["after"],
        },
    }
}

/// Where two texts first differ, a line of each around it: an output of a
/// catalog is tens of kilobytes, which an assertion's own message would cut.
fn first_difference(ran: &str, node: &str) -> String {
    let lines: Vec<&str> = ran.split('\n').collect();
    let wanted: Vec<&str> = node.split('\n').collect();
    let at = (0..lines.len().max(wanted.len()))
        .find(|at| lines.get(*at) != wanted.get(*at))
        .unwrap_or(0);
    format!(
        "line {}:\n      rust: {:?}\n      node: {:?}",
        at + 1,
        lines.get(at),
        wanted.get(at)
    )
}

/// Each way the binary's run differs from what was recorded.
fn differences(case: &Value, ran: &Ran) -> Vec<String> {
    let want = expected(case);
    let mut found = Vec::new();
    if let Some(stdout) = want.stdout {
        if ran.stdout.as_deref() != Some(stdout) {
            found.push(format!(
                "output: {}",
                first_difference(ran.stdout.as_deref().unwrap_or(""), stdout)
            ));
        }
    }
    if ran.stderr != want.stderr {
        found.push(format!(
            "error output: {}",
            first_difference(&ran.stderr, want.stderr)
        ));
    }
    if ran.code.map(i64::from) != Some(want.code) {
        found.push(format!("exit code: {:?}, recorded {}", ran.code, want.code));
    }
    if ran.stamped.len() > 1 {
        found.push(format!(
            "stamped more than one instant: {:?}",
            ran.stamped.iter().map(|ms| iso(*ms)).collect::<Vec<_>>()
        ));
    }
    let after = Value::Array(ran.after.clone());
    if &after != want.after {
        found.push(format!(
            "files: rust {}, node {}",
            serde_json::to_string(&after).unwrap(),
            serde_json::to_string(want.after).unwrap()
        ));
    }
    found
}

#[test]
fn every_case_says_writes_and_exits_as_node_did() {
    let golden = golden();
    assert_eq!(golden["format"], 1);
    assert_eq!(golden["platform"], PLATFORM);
    let clock = golden["clock"].as_str().unwrap();
    let cases = golden["cases"].as_array().unwrap();
    let names: BTreeSet<&str> = cases
        .iter()
        .map(|case| case["name"].as_str().unwrap())
        .collect();
    assert_eq!(cases.len(), names.len(), "each case once");
    assert!(cases.len() >= AT_LEAST, "{} cases", cases.len());
    let mut played = 0;
    let mut waiting = [0; NOT_PORTED.len()];
    let mut differ = Vec::new();
    for case in cases {
        let verb = case["args"][0].as_str().unwrap_or_default();
        if let Some(at) = NOT_PORTED.iter().position(|ported| *ported == verb) {
            waiting[at] += 1;
            continue;
        }
        played += 1;
        let found = differences(case, &run(case, clock));
        if !found.is_empty() {
            differ.push(format!(
                "{}:\n  {}",
                case["name"].as_str().unwrap(),
                found.join("\n  ")
            ));
        }
    }
    // What is recorded for the wiring that comes after is all there.
    assert!(waiting.iter().all(|count| *count >= 10), "{waiting:?}");
    assert!(played >= 300, "{played} played");
    assert!(
        differ.is_empty(),
        "{} of {played} cases differ:\n\n{}",
        differ.len(),
        differ.join("\n\n")
    );
}
