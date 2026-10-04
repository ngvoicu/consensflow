//! What the goldens' scenarios never reach, played against a session file in a
//! temporary Pi home. The stage and the records it is written with are here;
//! the tests are by what they look at: the records (`steps`), the file and the
//! clock (`looks`), and the files the extension writes beside the session
//! (`evidence_files`).

mod evidence_files;
mod looks;
mod steps;

use std::fs::{self, File};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde_json::json;
use tempfile::TempDir;

use super::*;
use crate::shared::record::cache::PiSettlement;

const SESSION: &str = "hazy-ridge";

/// When the first session file was written, in milliseconds.
const WRITTEN: i64 = 1_790_000_000_000;

/// A session file in a Pi home of its own, and the reader of its session.
struct Stage {
    home: TempDir,
    reader: Box<dyn Look + Send>,
    /// When the session file was last written: each write is a millisecond
    /// after the one before, so no write leaves the file's time as it was.
    written: i64,
}

impl Stage {
    /// A stage with no session file, whose reader has `vars` as well as a home.
    fn with_vars(vars: &[(&str, &str)]) -> Self {
        let home = tempfile::tempdir().unwrap();
        let mut all = vec![("HOME", home.path().to_str().unwrap())];
        all.extend_from_slice(vars);
        let reader = reader(SESSION, &Env::from_vars(all), &TimeZone::UTC);
        Self {
            home,
            reader,
            written: WRITTEN - 1,
        }
    }

    fn new() -> Self {
        Self::with_vars(&[])
    }

    /// A stage whose session file holds `records`.
    fn with(records: &[Value]) -> Self {
        let mut stage = Self::new();
        stage.write(records);
        stage
    }

    fn file(&self) -> PathBuf {
        self.home
            .path()
            .join(".pi")
            .join("agent")
            .join("sessions")
            .join("project")
            .join(format!("2026-08-24T18-00-00-000Z_{SESSION}.jsonl"))
    }

    /// The session file holding `records`, one a line.
    fn write(&mut self, records: &[Value]) {
        let lines: Vec<String> = records.iter().map(Value::to_string).collect();
        self.write_text(&format!("{}\n", lines.join("\n")));
    }

    fn write_text(&mut self, text: &str) {
        fs::create_dir_all(self.file().parent().unwrap()).unwrap();
        fs::write(self.file(), text).unwrap();
        self.written += 1;
        self.written_at(0);
    }

    /// The session file's time of writing set `nanos` nanoseconds past the
    /// time of its last write.
    fn written_at(&self, nanos: u32) {
        let since = Duration::new(
            u64::try_from(self.written / 1000).unwrap(),
            u32::try_from(self.written % 1000).unwrap() * 1_000_000 + nanos,
        );
        File::options()
            .write(true)
            .open(self.file())
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + since)
            .unwrap();
    }

    /// A look, `after` milliseconds after the file was last written, told
    /// `options`.
    fn look(&mut self, after: i64, options: &Options) -> Arc<Reading> {
        self.reader.look(options, self.written + after)
    }

    /// A look told nothing, `after` milliseconds after the file was last written.
    fn at(&mut self, after: i64) -> Record {
        known(&self.look(after, &Options::default()))
    }
}

/// The record a reading holds, or a failure of the test.
fn known(reading: &Reading) -> Record {
    match reading {
        Reading::Known(record) => record.clone(),
        Reading::Unknown(reason) => panic!("unknown: {reason}"),
    }
}

/// Why a reading says it is unknown, or a failure of the test.
fn reason(reading: &Reading) -> &str {
    match reading {
        Reading::Unknown(reason) => reason,
        Reading::Known(record) => panic!("known: {record:?}"),
    }
}

fn header() -> Value {
    json!({ "type": "session", "version": 3, "id": SESSION, "timestamp": "2026-08-24T18:00:00.703Z" })
}

/// A `message` record holding `fields`, as Pi writes one.
fn message(id: &str, fields: Value) -> Value {
    json!({ "type": "message", "id": id, "timestamp": "2026-08-24T18:00:01.000Z", "message": fields })
}

fn user(id: &str, text: &str) -> Value {
    message(
        id,
        json!({ "role": "user", "content": [{ "type": "text", "text": text }] }),
    )
}

/// An assistant's step that ended `stop`, saying `text`.
fn assistant(id: &str, stop: &str, text: &str) -> Value {
    message(
        id,
        json!({ "role": "assistant", "stopReason": stop, "content": [{ "type": "text", "text": text }] }),
    )
}

/// An assistant's step that called the tool `call`.
fn calling(id: &str, call: &Value) -> Value {
    message(
        id,
        json!({ "role": "assistant", "stopReason": "toolUse",
            "content": [{ "type": "toolCall", "id": call, "name": "read" }] }),
    )
}

/// The result of the tool `call`.
fn result(id: &str, call: &Value) -> Value {
    message(
        id,
        json!({ "role": "toolResult", "toolCallId": call, "content": [{ "type": "text", "text": "ok" }] }),
    )
}

/// An assistant's step that failed with `error`, at the message time `at`.
fn failing(id: &str, error: &Value, at: &Value) -> Value {
    message(
        id,
        json!({ "role": "assistant", "stopReason": "error", "errorMessage": error,
            "timestamp": at, "content": [] }),
    )
}
