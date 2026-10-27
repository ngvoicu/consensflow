//! The tests' own key, standing in for the update key: a fixed Ed25519 key that
//! signs a SHA256SUMS as Tauri's signer does (minisign's prehashed kind, with a
//! trusted comment naming the file), made by Node's crypto, which this crate
//! may not depend on for it. The signatures are the same each time: Ed25519 is
//! deterministic and the timestamp is fixed.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

/// What Node is asked: the key line, or the .sig of stdin's bytes for `NAME`.
const SIGNER: &str = r#"
const crypto = require("crypto");
const seed = crypto.createHash("sha256").update("cf-publish tests: the stand-in update key").digest();
const key = crypto.createPrivateKey({ key: Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), seed]), format: "der", type: "pkcs8" });
const raw = crypto.createPublicKey(key).export({ format: "der", type: "spki" }).subarray(12);
const id = Buffer.from("cf7e57c0ffee0001", "hex");
if (process.env.NAME === undefined) {
  process.stdout.write(Buffer.concat([Buffer.from("Ed"), id, raw]).toString("base64"));
} else {
  const bytes = require("fs").readFileSync(0);
  // minisign's legacy kind ("Ed") signs the file itself; the prehashed kind ("ED") its BLAKE2b-512.
  const legacy = process.env.LEGACY === "1";
  const signature = crypto.sign(null, legacy ? bytes : crypto.createHash("blake2b512").update(bytes).digest(), key);
  const comment = `timestamp:1793000000\tfile:${process.env.NAME}`;
  const global = crypto.sign(null, Buffer.concat([signature, Buffer.from(comment, "latin1")]), key);
  const text = ["untrusted comment: signature from the tests' key", Buffer.concat([Buffer.from(legacy ? "Ed" : "ED"), id, signature]).toString("base64"), `trusted comment: ${comment}`, global.toString("base64"), ""].join("\n");
  process.stdout.write(Buffer.from(text, "latin1").toString("base64"));
}
"#;

/// Runs the signer with `name` (None: the key line) over `bytes`, in minisign's
/// legacy kind where `legacy`.
#[allow(clippy::disallowed_methods)] // The test starts what signs for it.
fn node(name: Option<&str>, bytes: &[u8], legacy: bool) -> String {
    let mut command = Command::new("node");
    command
        .args(["-e", SIGNER])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .env_remove("NAME")
        .env("LEGACY", if legacy { "1" } else { "0" });
    if let Some(name) = name {
        command.env("NAME", name);
    }
    let mut child = command.spawn().expect("node runs the tests' signer");
    child
        .stdin
        .take()
        .expect("its input")
        .write_all(bytes)
        .expect("the bytes reach it");
    let output = child.wait_with_output().expect("the signer ends");
    assert!(output.status.success(), "the tests' signer failed");
    String::from_utf8(output.stdout).expect("base64")
}

/// The tests' key, as a minisign key line.
pub fn test_key() -> &'static str {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(|| node(None, b"", false))
}

/// A signature of `bytes` under `name` by the tests' key in minisign's legacy
/// kind (`Ed`: the file itself signed, not its BLAKE2b-512), which a publisher
/// that allowed it would take; well formed, and refused.
pub fn sign_legacy(bytes: &[u8], name: &str) -> String {
    node(Some(name), bytes, true)
}

/// The signatures made so far, by what was signed and its name.
type Made = Mutex<HashMap<(Vec<u8>, String), String>>;

/// The .sig file Tauri's signer would leave for `bytes` signed under `name`,
/// by the tests' key.
pub fn sign(bytes: &[u8], name: &str) -> String {
    static MADE: OnceLock<Made> = OnceLock::new();
    let made = MADE.get_or_init(Mutex::default);
    let at = (bytes.to_vec(), name.to_string());
    if let Some(sig) = made.lock().expect("the signatures made").get(&at) {
        return sig.clone();
    }
    let sig = node(Some(name), bytes, false);
    made.lock()
        .expect("the signatures made")
        .insert(at, sig.clone());
    sig
}
