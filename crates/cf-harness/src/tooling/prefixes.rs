//! A Claude Code transcript read, as the product reads it, after each of its lines
//! (`claude_prefixes`). The long run's watch (`crates/cf-evals/src/long_run/record.rs`)
//! keeps a reader of its own, on purpose: it holds the daemon to a reading that is not
//! the daemon's. This is how that reader is held to the product's: a test asks for the
//! product's reading of every prefix of a transcript and compares it with the watch's
//! (`crates/cf-evals/tests/long_run/parity.rs`), so a rule the product's reader learns
//! and the watch's does not fails there and not in a run of hours.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cf_base::env::Env;
use cf_base::time::{Clock, SystemClock};
use cf_proto::agents::Harness;
use serde_json::{json, Value};

use super::text;
use crate::records::{self, Options};

/// The product's reading of the first `n` lines of the transcript `text`, for each `n`
/// from 1 to the number of lines: `[{lines, reading}]`, each reading as
/// [`records::answers`] writes it with its items left out (the watch's judgments are the
/// turn's state and what it left running, not the words).
pub(super) fn claude_prefixes(question: &Value) -> Result<Value, String> {
    let transcript = text(question, "text")?;
    let session = text(question, "session")?;
    let lines: Vec<&str> = transcript.lines().collect();
    let folder = scratch()?;
    let read = read_all(&folder, session, &lines);
    // A folder this question made, by its own name.
    let _ = std::fs::remove_dir_all(&folder);
    read
}

/// A config folder of Claude Code's of its own, made new, with its `projects/p` folder.
fn scratch() -> Result<PathBuf, String> {
    // Its own name: the process and the question, so that two asked at once share none.
    static ASKED: AtomicU64 = AtomicU64::new(0);
    let asked = ASKED.fetch_add(1, Ordering::Relaxed);
    let folder = std::env::temp_dir().join(format!(
        "cf-claude-prefixes-{}-{}-{asked}",
        std::process::id(),
        SystemClock.now_ms()
    ));
    std::fs::create_dir_all(folder.join("projects").join("p"))
        .map_err(|failed| format!("{}: {failed}", folder.display()))?;
    Ok(folder)
}

/// The reading after each prefix of `lines`, in the config folder `folder`.
fn read_all(folder: &Path, session: &str, lines: &[&str]) -> Result<Value, String> {
    let file = folder
        .join("projects")
        .join("p")
        .join(format!("{session}.jsonl"));
    let root = folder
        .to_str()
        .ok_or_else(|| format!("{} is no text", folder.display()))?;
    let env = Env::from_vars([("CLAUDE_CONFIG_DIR", root)]);
    let mut read = Vec::with_capacity(lines.len());
    for n in 1..=lines.len() {
        let mut prefix = lines[..n].join("\n");
        prefix.push('\n');
        std::fs::write(&file, prefix).map_err(|failed| format!("{}: {failed}", file.display()))?;
        // A reader of its own for each prefix: the file is a new one each time.
        let reading = records::answers(
            Harness::Claude,
            session,
            &env,
            &Options::default(),
            &records::machine_zone(),
            SystemClock.now_ms(),
        );
        let mut reading = serde_json::to_value(&*reading).map_err(|failed| failed.to_string())?;
        if let Some(object) = reading.as_object_mut() {
            object.remove("items");
        }
        read.push(json!({ "lines": n, "reading": reading }));
    }
    Ok(Value::Array(read))
}
