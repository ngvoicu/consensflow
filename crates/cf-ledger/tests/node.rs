//! The ledger file between the two implementations, while both exist: each
//! is refused (`ledger-locked`) while the other holds the file, and Node
//! opens a ledger this crate wrote and reads it as this crate does. Node is
//! `CONSENSFLOW_NODE`, or `node` on the PATH; its ledger is `src/ledger/`.

// The tests start Node themselves; their helpers expect, as the tests do.
#![allow(clippy::disallowed_methods, clippy::expect_used)]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use cf_ledger::{open_ledger, NewChief, NewMember, NewProject, Options};

/// Node's ledger module, beside this crate in the repository.
fn node_ledger() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("src")
        .join("ledger")
        .join("index.js")
}

/// Node, running `script` (an ES module) with the ledger module and `file`
/// as its arguments.
fn node(script: &str, file: &Path) -> Command {
    let program = std::env::var_os("CONSENSFLOW_NODE").unwrap_or_else(|| "node".into());
    let mut command = Command::new(program);
    command
        .args(["--input-type=module", "-e"])
        .arg(format!(
            "import {{ pathToFileURL }} from 'node:url';\n\
             const {{ openLedger }} = await import(pathToFileURL(process.argv[1]).href);\n\
             const file = process.argv[2];\n{script}"
        ))
        .arg(node_ledger())
        .arg(file)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    command
}

/// What Node printed, whole.
fn printed(command: &mut Command) -> String {
    let output = command.output().expect("Node runs");
    assert!(output.status.success(), "Node failed: {output:?}");
    String::from_utf8(output.stdout).expect("Node prints UTF-8")
}

/// Node holding the ledger at `file` until killed, once it says so.
fn node_holding(file: &Path) -> Child {
    let mut holder = node(
        "openLedger(file);\nconsole.log('ready');\nsetInterval(() => {}, 1000);",
        file,
    )
    .spawn()
    .expect("Node starts");
    let said = BufReader::new(holder.stdout.take().expect("its output"))
        .lines()
        .map_while(Result::ok)
        .next();
    assert_eq!(said.as_deref(), Some("ready"));
    holder
}

#[test]
fn a_ledger_node_holds_is_refused_here_and_one_held_here_is_refused_to_node() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");

    let mut holder = node_holding(&file);
    let refused = open_ledger(&file, Options::default()).err().unwrap();
    assert_eq!(refused.code(), Some("ledger-locked"));
    holder.kill().unwrap();
    holder.wait().unwrap();

    let held = open_ledger(&file, Options::default()).unwrap();
    let said = printed(&mut node(
        "try { openLedger(file); console.log('opened') } catch (cause) { console.log(cause.code) }",
        &file,
    ));
    assert_eq!(said.trim(), "ledger-locked");
    held.close().unwrap();
}

#[test]
fn node_opens_a_ledger_written_here_and_reads_it_as_this_crate_does() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    let mut ledger = open_ledger(&file, Options::default()).unwrap();
    let project = ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "codex".into(),
                agent: Some("astraeus".into()),
            },
            staff: vec![NewMember {
                agent: "zeus".into(),
                harness: "pi".into(),
                designer: false,
                roles: vec!["worker".into(), "reviewer".into()],
                tier: "standard".into(),
            }],
            gate: true,
        })
        .unwrap();
    let here = serde_json::to_string(&ledger.projects().unwrap()).unwrap();
    let events = serde_json::to_string(&ledger.events(project.id, 0, 500).unwrap()).unwrap();
    ledger.close().unwrap();

    let read = printed(&mut node(
        "const ledger = openLedger(file);\n\
         console.log(JSON.stringify(ledger.projects()));\n\
         console.log(JSON.stringify(ledger.events(1)));\n\
         ledger.close();",
        &file,
    ));
    let mut lines = read.lines();
    assert_eq!(lines.next(), Some(here.as_str()), "the projects");
    assert_eq!(lines.next(), Some(events.as_str()), "their events");
}
