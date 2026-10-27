//! What Claude Code keeps of a session, and what a live test reads of the
//! machine around it. Claude writes the transcript of a conversation under its
//! config folder (`projects/<folder>/<session>.jsonl`) and a status file for each
//! window (`sessions/<pid>.json`: `busy`, `idle` or `waiting`, and when it last
//! changed), which is where the daemon reads a Claude window's state from. A
//! test reads both as the daemon does, and watches the status as it changes
//! ([`Status`]), because what a test shows is when it read what.
//!
//! The config folder is the one the run's Claude windows run on (a folder of the
//! run's own, with a copy of the machine's login: [`super::login`]); a test is
//! given it, never the `CLAUDE_CONFIG_DIR` of the shell it runs in, and never the
//! person's own `~/.claude`.

#[cfg(test)]
mod tests;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::Value;

use crate::process::Run;
use crate::{Error, Result};

pub use crate::rig::project_name;

/// How often a status is read: as often as it is useful to know when it changed.
const EVERY: Duration = Duration::from_millis(150);

/// One look at the status file of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    /// When it was read, in milliseconds since the epoch.
    pub at_ms: i64,
    /// What it says (`busy`, `idle`, `waiting`, `shell`…); none where there was no
    /// status file for the session.
    pub status: Option<String>,
    /// What a window that waits waits for.
    pub waiting_for: Option<String>,
    /// When Claude says the status last changed, in milliseconds since the epoch
    /// (`statusUpdatedAt`: the daemon reads the effect of an interrupt by it).
    pub changed_at_ms: Option<i64>,
}

/// The status file of the session `session` under the Claude config folder
/// `config`, as it is now: the first file of `sessions/` named by a process id
/// that names the session.
pub fn status_of(config: &Path, session: &str) -> Option<Value> {
    status_where(config, session, false)
}

/// [`status_of`] for the window that is running: a file whose process is gone
/// says nothing, as it says nothing to the daemon. The file a window leaves when
/// it is killed names the conversation, and so does the one a window resumed on
/// that conversation writes: only the second is the window's.
pub fn running_status_of(config: &Path, session: &str) -> Option<Value> {
    status_where(config, session, true)
}

/// The first status file that names `session`, and, where `running` is asked
/// for, whose process is there.
fn status_where(config: &Path, session: &str, running: bool) -> Option<Value> {
    let mut files: Vec<PathBuf> = fs::read_dir(config.join("sessions"))
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|file| names_a_process(file))
        .collect();
    files.sort();
    files.into_iter().find_map(|file| {
        if running && !process_of(&file).is_some_and(crate::process::is_alive) {
            return None;
        }
        let status: Value = serde_json::from_str(&fs::read_to_string(file).ok()?).ok()?;
        (status["sessionId"] == session).then_some(status)
    })
}

/// Whether the file is called `<digits>.json`.
fn names_a_process(file: &Path) -> bool {
    let name = file.file_name().map(|name| name.to_string_lossy());
    name.as_deref()
        .and_then(|name| name.strip_suffix(".json"))
        .is_some_and(|stem| !stem.is_empty() && stem.bytes().all(|byte| byte.is_ascii_digit()))
}

/// The process a status file is of, by its name `<process id>.json`.
fn process_of(file: &Path) -> Option<u32> {
    file.file_name()?
        .to_string_lossy()
        .strip_suffix(".json")?
        .parse()
        .ok()
}

/// A look at the status of `session`, as it is read now: of the window that is
/// running where `running` is asked for.
fn sample(config: &Path, session: &str, running: bool) -> Sample {
    let status = status_where(config, session, running);
    let text = |field: &str| status.as_ref()?[field].as_str().map(str::to_owned);
    Sample {
        at_ms: jiff::Timestamp::now().as_millisecond(),
        status: text("status"),
        waiting_for: text("waitingFor"),
        changed_at_ms: status.as_ref().and_then(|status| {
            let at = &status["statusUpdatedAt"];
            at.as_i64().or_else(|| {
                // Rounded: a time with a fraction of a millisecond is that millisecond.
                at.as_f64().map(|at| at.round() as i64)
            })
        }),
    }
}

/// The status of a session, read every 150 ms until it is stopped.
pub struct Status {
    samples: Arc<Mutex<Vec<Sample>>>,
    stopped: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl Status {
    /// Begins reading the status of `session` under the Claude config folder
    /// `config`.
    pub fn watch(config: &Path, session: &str) -> Self {
        Self::begin(config, session, false)
    }

    /// [`Status::watch`] of the window that is running ([`running_status_of`]):
    /// the file a killed window left says nothing.
    pub fn watch_running(config: &Path, session: &str) -> Self {
        Self::begin(config, session, true)
    }

    fn begin(config: &Path, session: &str, running: bool) -> Self {
        let samples = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let reader = {
            let (samples, stopped) = (Arc::clone(&samples), Arc::clone(&stopped));
            let (config, session) = (config.to_path_buf(), session.to_owned());
            thread::spawn(move || {
                while !stopped.load(Ordering::Relaxed) {
                    let look = sample(&config, &session, running);
                    samples
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(look);
                    thread::sleep(EVERY);
                }
            })
        };
        Self {
            samples,
            stopped,
            reader: Some(reader),
        }
    }

    /// What was read so far, in order.
    pub fn samples(&self) -> Vec<Sample> {
        self.samples
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Stops reading, and answers what was read.
    pub fn stop(mut self) -> Vec<Sample> {
        self.end();
        self.samples()
    }

    fn end(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for Status {
    /// A watch that was not stopped does not read on after the test.
    fn drop(&mut self) {
        self.end();
    }
}

/// Runs of samples that read alike, each as a line: `busy 0.0–12.3 s (82
/// reads)`, the times in seconds after `from_ms`.
pub fn runs(samples: &[Sample], from_ms: i64) -> Vec<String> {
    struct Run {
        word: String,
        from: i64,
        to: i64,
        count: usize,
    }
    let mut found: Vec<Run> = Vec::new();
    for sample in samples {
        let word = format!(
            "{}{}",
            sample.status.as_deref().unwrap_or("no status file"),
            sample
                .waiting_for
                .as_deref()
                .filter(|waiting| !waiting.is_empty())
                .map_or_else(String::new, |waiting| format!(" ({waiting})"))
        );
        match found.last_mut() {
            Some(last) if last.word == word => {
                last.to = sample.at_ms;
                last.count += 1;
            }
            _ => found.push(Run {
                word,
                from: sample.at_ms,
                to: sample.at_ms,
                count: 1,
            }),
        }
    }
    found
        .iter()
        .map(|run| {
            format!(
                "{} {}–{} s ({} reads)",
                run.word,
                super::seconds(run.from - from_ms),
                super::seconds(run.to - from_ms),
                run.count
            )
        })
        .collect()
}

/// Where Claude keeps the transcript of `session` under the config folder
/// `config` (`projects/<folder>/<session>.jsonl`), or none.
pub fn transcript_file(config: &Path, session: &str) -> Option<PathBuf> {
    let mut folders: Vec<PathBuf> = fs::read_dir(config.join("projects"))
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    folders.sort();
    folders
        .into_iter()
        .map(|folder| folder.join(format!("{session}.jsonl")))
        .find(|candidate| candidate.is_file())
}

/// One record of a conversation, as Claude kept it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// When it says it was made, in milliseconds since the epoch.
    pub at_ms: Option<i64>,
    /// What it is: the message's role (`user`, `assistant`) or the record's type,
    /// with its subtype after a slash (`system/turn_duration`).
    pub kind: String,
    /// What it says: a message's words, a tool's call, a tool's result, the
    /// parts joined with `⏎`.
    pub text: String,
    /// The model Claude says made it (`message.model` of an assistant's record,
    /// which is `<synthetic>` for what Claude wrote itself, a refusal or an
    /// interrupt's notice): none for a record that says none.
    pub model: Option<String>,
    /// The start of its line, as it is in the file.
    pub raw: String,
}

/// What Claude kept of a conversation: the records of its transcript, oldest
/// first, and none where it has none yet. A line that is no JSON (the one
/// Claude is writing as this reads) is not a record.
pub fn record(config: &Path, session: &str) -> Vec<Entry> {
    let Some(file) = transcript_file(config, session) else {
        return Vec::new();
    };
    let Ok(text) = crate::files::read_finished_lines(&file) else {
        return Vec::new();
    };
    text.lines().filter_map(entry_of).collect()
}

/// The record a line of a transcript is.
fn entry_of(line: &str) -> Option<Entry> {
    let record: Value = serde_json::from_str(line).ok()?;
    let content = &record["message"]["content"];
    let parts: Vec<Value> = match content {
        Value::String(text) => vec![serde_json::json!({ "type": "text", "text": text })],
        Value::Array(parts) => parts.clone(),
        _ => Vec::new(),
    };
    // A request Claude Code 2.1.310 keeps of what it sent the API carries the message it
    // sent (`api-request-blob`), which is none of the conversation's: it is its type.
    let role = record["type"]
        .as_str()
        .filter(|kind| kind.starts_with("api-request"))
        .or_else(|| record["message"]["role"].as_str())
        .or_else(|| record["type"].as_str())
        .unwrap_or("undefined");
    let kind = match record["subtype"].as_str() {
        Some(subtype) if !subtype.is_empty() => format!("{role}/{subtype}"),
        _ => role.to_owned(),
    };
    Some(Entry {
        at_ms: record["timestamp"]
            .as_str()
            .and_then(|at| at.parse::<jiff::Timestamp>().ok())
            .map(|at| at.as_millisecond()),
        kind,
        text: parts.iter().map(shown).collect::<Vec<_>>().join(" ⏎ "),
        model: record["message"]["model"].as_str().map(str::to_owned),
        raw: line.chars().take(400).collect(),
    })
}

/// What a part of a message says.
fn shown(part: &Value) -> String {
    match part["type"].as_str() {
        Some("text") => super::text(&part["text"]),
        Some("tool_use") => format!("tool_use {} {}", super::text(&part["name"]), part["input"]),
        Some("tool_result") => {
            let error = if part["is_error"] == true {
                " (error)"
            } else {
                ""
            };
            format!("tool_result{error} {}", super::text(&part["content"]))
        }
        Some(other) => other.to_owned(),
        None => "undefined".to_owned(),
    }
}

/// A process of the machine, as `ps` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub ppid: u32,
    pub command: String,
}

/// The processes whose command line holds `text`, from `ps`. For macOS and
/// Linux: a machine with no `ps` is an error.
pub fn processes_with(text: &str) -> Result<Vec<Process>> {
    let ran = Run::new("ps")
        .args(["-axo", "pid=,ppid=,command="])
        .inheriting_env()
        .run()?;
    if ran.code != Some(0) {
        return Err(Error::Program {
            action: "list the processes with",
            program: "ps".to_owned(),
            source: std::io::Error::other(ran.to_string()),
        });
    }
    Ok(listed(&ran.stdout, text))
}

/// The processes of a listing `ps -axo pid=,ppid=,command=` made whose command
/// line holds `text`, but for the listing's own.
fn listed(listing: &str, text: &str) -> Vec<Process> {
    listing
        .lines()
        .filter_map(|line| {
            let (pid, rest) = line.trim_start().split_once(char::is_whitespace)?;
            let (ppid, command) = rest.trim_start().split_once(char::is_whitespace)?;
            // The command is the rest of the line, as it is spaced.
            Some(Process {
                pid: pid.parse().ok()?,
                ppid: ppid.parse().ok()?,
                command: command.trim_start().to_owned(),
            })
        })
        .filter(|process| process.command.contains(text) && !process.command.contains("ps -axo"))
        .collect()
}
