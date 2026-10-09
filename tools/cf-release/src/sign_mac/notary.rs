//! Apple's notary: a file submitted to it, waited for, and its ticket stapled.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::name;
use super::secrets::Credentials;
use super::tools::{args, fail, Tools};
use crate::cli::Failure;

/// How long one notarization may take before the release gives up on it. The
/// notary answers most in minutes, but a new team's first submission sat in
/// Apple's queue past 20 (2026-10-05).
const WAIT: &str = "60m";

/// Who asks the notary: its key, in a file of this run's, and the key's id and
/// issuer.
pub(super) struct Notary {
    key: PathBuf,
    key_id: String,
    issuer: String,
}

impl Notary {
    /// The notary's key, written to `scratch` for `notarytool` to read.
    pub fn new(credentials: &Credentials, scratch: &Path) -> Result<Self, Failure> {
        let key = scratch.join("notary.p8");
        credentials.write_key(&key)?;
        Ok(Self {
            key,
            key_id: credentials.key_id().to_string(),
            issuer: credentials.issuer().to_string(),
        })
    }

    /// How `notarytool` is told who asks.
    fn arguments(&self) -> Vec<OsString> {
        args![
            "--key",
            &self.key,
            "--key-id",
            &self.key_id,
            "--issuer",
            &self.issuer,
        ]
    }
}

/// Has Apple's notary check `target`, waits for its answer, and staples the
/// ticket to it.
pub(super) fn notarize(
    tools: &mut Tools,
    target: &Path,
    notary: &Notary,
    scratch: &Path,
) -> Result<(), Failure> {
    let label = name(target);
    // The notary takes an app as a zip; the ticket goes on the app itself.
    let upload = if target
        .extension()
        .is_some_and(|extension| extension == "app")
    {
        let zip = scratch.join(format!("{label}.zip"));
        tools.run("ditto", args!["-c", "-k", "--keepParent", target, &zip])?;
        zip
    } else {
        target.to_path_buf()
    };
    tools.say(&format!("the notary checks {label}"));
    let mut submit = args!["notarytool", "submit", &upload];
    submit.extend(notary.arguments());
    submit.extend(args![
        "--wait",
        "--timeout",
        WAIT,
        "--output-format",
        "json"
    ]);
    // The notary answers on stdout whatever its code: it is the answer that counts.
    let submitted = tools.capture("xcrun", submit)?;
    let answer: Value = serde_json::from_str(&submitted.stdout).map_err(|_| {
        fail(format!(
            "notarytool submit failed: {}",
            submitted.stderr.trim()
        ))
    })?;
    let status = answer.get("status").and_then(Value::as_str);
    if status != Some("Accepted") {
        // The notary's log is the one place that says which file it refused, and why.
        if let Some(id) = answer.get("id").and_then(Value::as_str) {
            let mut log = args!["notarytool", "log", id];
            log.extend(notary.arguments());
            if let Ok(logged) = tools.capture("xcrun", log) {
                let said = [&logged.stdout, &logged.stderr]
                    .into_iter()
                    .find(|text| !text.is_empty());
                tools.echo(said.map_or("", String::as_str).trim_end());
            }
        }
        let status = status.unwrap_or("no status");
        return Err(fail(format!("the notary answered {status} for {label}")));
    }
    tools.run("xcrun", args!["stapler", "staple", target])?;
    tools.run("xcrun", args!["stapler", "validate", target])?;
    Ok(())
}
