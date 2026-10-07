//! The Rust `cf` held to what Node's `bin/cf.mjs` said and did, case by case:
//! tests/goldens/cli/ (`npm run goldens:cli`, one set a platform, the system's
//! own recorded on it) ran each case against Node, in a folder of its own,
//! with the clock fixed and the environment given in full, and wrote down the
//! files of the folder before and after, the output, the error output and the
//! exit code. Here each case is made again in a folder of its own, the binary run
//! as the case says (no home of a recording has the way back's `use-node` file
//! in it, so the verbs are Rust's), and all of it compared byte for byte. The
//! format is in tests/goldens/cli/FORMAT.md.
//!
//! The binary cannot be given the clock Node was: what it stamps an agent with
//! is read as the instant the recorder fixed once it is known to be one instant,
//! the run's own. A difference Rust keeps from Node on purpose is recorded with
//! what Rust says (`kept`), and is held to that.
//!
//! `setup` and `doctor` are played as every verb is. What a recording names of
//! the machine that made it (`$NODE`, `$REPO`, `$HASH`, `$PAYLOAD`) is put as
//! this run's (`names.rs`), and the two differences the launcher's new shape
//! makes are stated once, each with its reason (`paired.rs`).

// The goldens' own reading and the process the tests start: a failure in
// either is the test's.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    clippy::too_many_lines
)]

mod names;
mod paired;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};

use cf_base::time::{iso, parse, Clock, SystemClock};
use names::Names;
use serde_json::Value;

/// The name of this system's goldens: Windows' or, as every other system the
/// tests run on, macOS's.
const PLATFORM: &str = if cfg!(windows) { "win32" } else { "darwin" };

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

/// The folder as the case's `before` says, made.
fn make(root: &Path, before: &Value, names: &Names) {
    let name = plain(&root.to_string_lossy());
    for entry in before.as_array().unwrap() {
        if let Some(dir) = entry["dir"].as_str() {
            fs::create_dir_all(root.join(dir)).unwrap();
            continue;
        }
        let path = root.join(entry["path"].as_str().unwrap());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, names.made(entry["text"].as_str().unwrap(), &name)).unwrap();
        #[cfg(unix)]
        if entry["executable"].as_bool() == Some(true) {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
}

/// The folder's entries as the recorder lists them: each file with its text,
/// each folder with nothing in it, by path.
fn listing(root: &Path, at: &Path, names: &Names, found: &mut Vec<Value>) {
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
            listing(root, &path, names, found);
            continue;
        }
        let bytes = fs::read(&path).unwrap();
        // A file of an extension's bundle is named by a hash and holds the
        // repository's own bytes: the recording has neither, but their names.
        let (place, text) = match names::bundle_entry(&named(&path)) {
            Some((masked, own)) => (masked, names::payload(&own, &bytes)),
            None => (
                named(&path),
                names.recorded(
                    &String::from_utf8_lossy(&bytes),
                    &plain(&root.to_string_lossy()),
                ),
            ),
        };
        let mut entry = serde_json::json!({ "path": place, "text": text });
        if is_executable(&path) {
            entry["executable"] = Value::Bool(true);
        }
        found.push(entry);
    }
}

/// Whether the file at `path` is a program: a POSIX matter, which Windows
/// has no bit for.
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    false
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
/// case gives.
fn spawn(case: &Value, root: &Path, names: &Names) -> (Output, Option<String>) {
    let name = plain(&root.to_string_lossy());
    let mut command = Command::new(&names.cf);
    command.env_clear();
    for (variable, value) in case["env"].as_object().unwrap() {
        command.env(variable, value.as_str().unwrap().replace("$ROOT", &name));
    }
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

fn run(case: &Value, clock: &str, names: &Names) -> Ran {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    make(&root, &case["before"], names);
    let name = plain(&root.to_string_lossy());
    let started = SystemClock.now_ms();
    let (output, first) = spawn(case, &root, names);
    let window = (started - MARGIN_MS, SystemClock.now_ms() + MARGIN_MS);
    let mut stamped = BTreeSet::new();
    let mut after = Vec::new();
    listing(&root, &root, names, &mut after);
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
        stdout: stdout.map(|text| names.recorded(&text, &name)),
        stderr: names.recorded(&String::from_utf8_lossy(&output.stderr), &name),
        code: output.status.code(),
        after,
        stamped,
    }
}

/// What Node said, or what Rust is recorded to say where it keeps a difference.
struct Expected {
    stdout: Option<String>,
    stderr: String,
    code: i64,
    after: Value,
}

fn expected(case: &Value) -> Expected {
    match case.get("kept") {
        Some(kept) => {
            let rust = &kept["rust"];
            Expected {
                stdout: rust["stdout"].as_str().map(str::to_owned),
                stderr: rust["stderr"].as_str().unwrap().to_owned(),
                code: rust["code"].as_i64().unwrap(),
                after: if rust["after"].as_str() == Some("before") {
                    case["before"].clone()
                } else {
                    case["after"].clone()
                },
            }
        }
        None => Expected {
            stdout: case["stdout"].as_str().map(str::to_owned),
            stderr: case["stderr"].as_str().unwrap().to_owned(),
            code: case["code"].as_i64().unwrap(),
            after: case["after"].clone(),
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

/// Where two listings of files first differ: the first path that one has and
/// the other has not, or holds otherwise.
fn first_file_difference(ran: &[Value], node: &[Value]) -> String {
    let at = (0..ran.len().max(node.len()))
        .find(|at| ran.get(*at) != node.get(*at))
        .unwrap_or(0);
    let show = |entry: Option<&Value>| {
        entry.map_or_else(
            || "none".to_owned(),
            |entry| serde_json::to_string(entry).unwrap(),
        )
    };
    format!(
        "entry {}:\n      rust: {}\n      node: {}",
        at + 1,
        show(ran.get(at)),
        show(node.get(at))
    )
}

/// Each way the binary's run differs from what was recorded, and the reason
/// of the difference that was meant, if the case held one.
fn differences(case: &Value, ran: &Ran) -> (Vec<String>, Option<&'static str>) {
    let mut want = expected(case);
    let meant = paired::pair(case, &mut want);
    let mut found = Vec::new();
    if let Some(stdout) = &want.stdout {
        if ran.stdout.as_deref() != Some(stdout.as_str()) {
            found.push(format!(
                "output: {}",
                first_difference(ran.stdout.as_deref().unwrap_or(""), stdout)
            ));
        }
    }
    if ran.stderr != want.stderr {
        found.push(format!(
            "error output: {}",
            first_difference(&ran.stderr, &want.stderr)
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
    if Value::Array(ran.after.clone()) != want.after {
        found.push(format!(
            "files: {}",
            first_file_difference(&ran.after, want.after.as_array().unwrap())
        ));
    }
    (found, meant)
}

#[test]
fn every_case_says_writes_and_exits_as_node_did() {
    let golden = golden();
    assert_eq!(golden["format"], 1);
    assert_eq!(golden["platform"], PLATFORM);
    let clock = golden["clock"].as_str().unwrap();
    let cases = golden["cases"].as_array().unwrap();
    let called: BTreeSet<&str> = cases
        .iter()
        .map(|case| case["name"].as_str().unwrap())
        .collect();
    assert_eq!(cases.len(), called.len(), "each case once");
    assert!(cases.len() >= AT_LEAST, "{} cases", cases.len());
    let names = Names::new();
    let mut differ = Vec::new();
    // The cases of each verb, and those that hold a difference meant on purpose, by its reason.
    let mut verbs: BTreeMap<&str, usize> = BTreeMap::new();
    let mut meant: BTreeMap<&str, usize> = BTreeMap::new();
    for case in cases {
        *verbs
            .entry(case["args"][0].as_str().unwrap_or_default())
            .or_default() += 1;
        let (found, reason) = differences(case, &run(case, clock, &names));
        if let Some(reason) = reason {
            *meant.entry(reason).or_default() += 1;
        }
        if !found.is_empty() {
            differ.push(format!(
                "{}:\n  {}",
                case["name"].as_str().unwrap(),
                found.join("\n  ")
            ));
        }
    }
    assert!(
        differ.is_empty(),
        "{} of {} cases differ:\n\n{}",
        differ.len(),
        cases.len(),
        differ.join("\n\n")
    );
    // Every case of `setup` and `doctor` was played, not left out.
    assert!(verbs["setup"] >= 17, "{} cases of setup", verbs["setup"]);
    assert!(verbs["doctor"] >= 22, "{} cases of doctor", verbs["doctor"]);
    // What is paired holds in the cases it is meant for, and in no others.
    assert_eq!(meant[paired::SHAPE], 13, "{meant:?}");
    assert_eq!(meant[paired::COPY], 1, "{meant:?}");
    assert_eq!(meant.len(), 2, "{meant:?}");
}
