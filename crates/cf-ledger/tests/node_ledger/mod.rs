//! Node's ledger (`src/ledger/`) run on a file this crate wrote, while both
//! exist: what the suites that hold the two to each other share. Node is
//! `CONSENSFLOW_NODE`, or `node` on the PATH.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Node's ledger module, beside this crate in the repository.
pub fn ledger_module() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("src")
        .join("ledger")
        .join("index.js")
}

/// Node, running `script` (an ES module) with the ledger module and `file`
/// as its arguments.
pub fn node(script: &str, file: &Path) -> Command {
    let program = std::env::var_os("CONSENSFLOW_NODE").unwrap_or_else(|| "node".into());
    let mut command = Command::new(program);
    command
        .args(["--input-type=module", "-e"])
        .arg(format!(
            "import {{ pathToFileURL }} from 'node:url';\n\
             const {{ openLedger }} = await import(pathToFileURL(process.argv[1]).href);\n\
             const file = process.argv[2];\n{script}"
        ))
        .arg(ledger_module())
        .arg(file)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    command
}

/// What Node printed, whole.
pub fn printed(command: &mut Command) -> String {
    let output = command.output().expect("Node runs");
    assert!(output.status.success(), "Node failed: {output:?}");
    String::from_utf8(output.stdout).expect("Node prints UTF-8")
}
