//! What the window writes of itself, as Claude does: its conversation, one JSON
//! record to a line in `projects/integration/<session>.jsonl` under Claude's
//! config folder, and its live status in `sessions/<pid>.json`. The daemon
//! reads both. And what it writes for the test: the ids of the processes it
//! started, beside the home.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

/// The version Claude's records say it is.
const VERSION: &str = "2.1.277";

/// A window's files.
pub struct Records {
    session: String,
    transcript: PathBuf,
    status_file: PathBuf,
    /// How many records the window has written: each record's `uuid` ends in it.
    ordinal: u64,
}

impl Records {
    /// The files of the window with process id `pid`, running `session`, under
    /// the Claude config folder `config`.
    pub fn new(config: &Path, session: &str, pid: u32) -> io::Result<Self> {
        let transcript = config
            .join("projects")
            .join("integration")
            .join(format!("{session}.jsonl"));
        let status_file = config.join("sessions").join(format!("{pid}.json"));
        for file in [&transcript, &status_file] {
            if let Some(folder) = file.parent() {
                fs::create_dir_all(folder)?;
            }
        }
        Ok(Self {
            session: session.to_owned(),
            transcript,
            status_file,
            ordinal: 0,
        })
    }

    /// Where the window's live status is: removed when the window ends.
    pub fn status_file(&self) -> &Path {
        &self.status_file
    }

    /// The number the next record's message id is made with: of the record
    /// last written, as the records of a turn have always been numbered.
    pub fn ordinal(&self) -> u64 {
        self.ordinal
    }

    /// Writes one record: the session, the version, the time and a `uuid` of
    /// its own, then `fields`.
    pub fn append(&mut self, pid: u32, fields: Value) -> io::Result<()> {
        self.ordinal += 1;
        let mut record = Map::new();
        record.insert("sessionId".to_owned(), json!(self.session));
        record.insert("version".to_owned(), json!(VERSION));
        record.insert(
            "timestamp".to_owned(),
            json!(jiff::Timestamp::now()
                .strftime("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string()),
        );
        record.insert(
            "uuid".to_owned(),
            json!(format!("{}-{pid}-{}", self.session, self.ordinal)),
        );
        if let Value::Object(fields) = fields {
            record.extend(fields);
        }
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.transcript)?;
        write_line(&mut file, &Value::Object(record))
    }

    /// Writes the status the window is in: `idle` or `busy`.
    pub fn status(&self, pid: u32, value: &str) -> io::Result<()> {
        fs::write(
            &self.status_file,
            json!({ "pid": pid, "sessionId": self.session, "kind": "interactive", "status": value })
                .to_string(),
        )
    }
}

/// Writes `record` and its line break to `out`. `writeln!` of a value writes it
/// a token at a time, and a case that reads the record while the agent writes
/// it meets a line cut inside a string; so it is one write.
fn write_line(out: &mut impl Write, record: &Value) -> io::Result<()> {
    let mut line = record.to_string();
    line.push('\n');
    out.write_all(line.as_bytes())
}

/// Writes down that the window's process `pid` runs `session`, in the two
/// files the test reads, beside the daemon's home: its ids alone, and with the
/// session.
pub fn note_process(beside: &Path, pid: u32, session: &str) -> io::Result<()> {
    for (name, line) in [
        ("pids.jsonl", format!("{pid}\n")),
        ("processes.jsonl", format!("{pid}\t{session}\n")),
    ] {
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(beside.join(name))?;
        file.write_all(line.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(file: &Path) -> Vec<Value> {
        fs::read_to_string(file)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn a_record_has_the_session_the_version_the_time_and_a_uuid_before_its_own_fields() {
        let config = tempfile::tempdir().unwrap();
        let mut records = Records::new(config.path(), "sess-1", 4242).unwrap();
        records
            .append(
                4242,
                json!({ "type": "user", "message": { "role": "user", "content": "hi" } }),
            )
            .unwrap();
        records.append(4242, json!({ "type": "system" })).unwrap();
        let written = lines(&config.path().join("projects/integration/sess-1.jsonl"));
        assert_eq!(written.len(), 2);
        let keys: Vec<&String> = written[0].as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            [
                "sessionId",
                "version",
                "timestamp",
                "uuid",
                "type",
                "message"
            ]
        );
        assert_eq!(written[0]["sessionId"], "sess-1");
        assert_eq!(written[0]["version"], "2.1.277");
        assert_eq!(written[0]["uuid"], "sess-1-4242-1");
        assert_eq!(written[1]["uuid"], "sess-1-4242-2");
        let at = written[0]["timestamp"].as_str().unwrap();
        assert!(
            at.len() == 24 && at.ends_with('Z') && at.as_bytes()[10] == b'T',
            "{at}"
        );
        assert_eq!(records.ordinal(), 2);
    }

    /// What a file is given, a write at a time.
    #[derive(Default)]
    struct Writes(Vec<Vec<u8>>);

    impl Write for Writes {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.push(bytes.to_vec());
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_record_is_one_write_so_a_reader_never_meets_a_line_cut_inside_a_string() {
        let mut file = Writes::default();
        let record = json!({
            "type": "assistant",
            "message": { "content": [{ "type": "text", "text": "ran cf: Waiting for a member" }] }
        });
        write_line(&mut file, &record).unwrap();
        assert_eq!(file.0.len(), 1, "{:?}", file.0);
        assert_eq!(file.0[0], format!("{record}\n").into_bytes());
    }

    #[test]
    fn the_status_is_the_pid_the_session_and_what_the_window_is_doing_and_is_written_over() {
        let config = tempfile::tempdir().unwrap();
        let records = Records::new(config.path(), "sess-1", 4242).unwrap();
        assert_eq!(
            records.status_file(),
            config.path().join("sessions/4242.json")
        );
        records.status(4242, "busy").unwrap();
        records.status(4242, "idle").unwrap();
        let status: Value =
            serde_json::from_str(&fs::read_to_string(records.status_file()).unwrap()).unwrap();
        assert_eq!(
            status,
            json!({ "pid": 4242, "sessionId": "sess-1", "kind": "interactive", "status": "idle" })
        );
    }

    #[test]
    fn the_processes_a_window_writes_down_go_to_the_two_files_beside_the_home_in_order() {
        let root = tempfile::tempdir().unwrap();
        note_process(root.path(), 11, "a").unwrap();
        note_process(root.path(), 22, "b").unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("pids.jsonl")).unwrap(),
            "11\n22\n"
        );
        assert_eq!(
            fs::read_to_string(root.path().join("processes.jsonl")).unwrap(),
            "11\ta\n22\tb\n"
        );
    }
}
