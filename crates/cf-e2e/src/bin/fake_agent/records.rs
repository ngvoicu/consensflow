//! What the window writes of itself, as Claude does: its conversation, one JSON
//! record to a line in `projects/integration/<session>.jsonl` under Claude's
//! config folder, and its live status in `sessions/<pid>.json`. The daemon
//! reads both. And what it writes for the test: the ids of the processes it
//! started, beside the home.
//!
//! A conversation the app's quit cut in a turn (its last record is the user's
//! message, with nothing after it) is resumed as Claude 2.1.296 resumes it: the
//! window waits at its prompt and writes nothing of that turn, and closes it
//! with a message of its own when the next one comes (`No response requested.`,
//! no model behind it, written just before that message).

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

/// The version Claude's records say it is.
const VERSION: &str = "2.1.277";

/// What Claude writes as the user's turn when it interrupts one it was at work on.
pub const INTERRUPTED: &str = "[Request interrupted by user]";

/// What Claude says, in a message of its own, of a turn that was cut and that
/// nothing answered.
const NO_RESPONSE: &str = "No response requested.";

/// A window's files.
pub struct Records {
    session: String,
    transcript: PathBuf,
    status_file: PathBuf,
    /// How many records the window has written: each record's `uuid` ends in it.
    ordinal: u64,
    /// The conversation ends in a turn nothing answered, which the next user's
    /// message finds open and closes.
    cut: bool,
    /// Something was said in the conversation since it began or was last
    /// compacted: a compaction has something to compact.
    said: bool,
}

/// Whether the conversation `text` ends in a turn nothing answered: its last
/// record is a user's message that is not the marker of an interrupt.
fn ends_cut(text: &str) -> bool {
    let Some(last) = text.lines().rev().find(|line| !line.trim().is_empty()) else {
        return false;
    };
    let Ok(record) = serde_json::from_str::<Value>(last) else {
        return false;
    };
    record["type"] == "user" && record["message"]["content"][0]["text"] != INTERRUPTED
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
        let kept = fs::read_to_string(&transcript).unwrap_or_default();
        Ok(Self {
            session: session.to_owned(),
            transcript,
            status_file,
            ordinal: 0,
            cut: ends_cut(&kept),
            said: !kept.trim().is_empty(),
        })
    }

    /// Whether something was said in the conversation since it began or was
    /// last compacted.
    pub fn said(&self) -> bool {
        self.said
    }

    /// The conversation was compacted, or a compaction found nothing to compact:
    /// what its own records say is nothing said.
    pub fn compacted(&mut self) {
        self.said = false;
    }

    /// Where the window's live status is: removed when the window ends.
    pub fn status_file(&self) -> &Path {
        &self.status_file
    }

    /// The number the next record's message id is made with: of the record
    /// last written, as the records of a turn have always been numbered.
    /// The conversation's transcript.
    pub fn transcript(&self) -> &Path {
        &self.transcript
    }

    pub fn ordinal(&self) -> u64 {
        self.ordinal
    }

    /// The `uuid` of the record the window `pid` wrote last: the parent of the
    /// next, where Claude says one.
    pub fn last_uuid(&self, pid: u32) -> String {
        format!("{}-{pid}-{}", self.session, self.ordinal)
    }

    /// Writes one record: the session, the version, the time and a `uuid` of
    /// its own, then `fields`. A user's message that comes to a conversation
    /// that was cut closes the turn that was cut first.
    pub fn append(&mut self, pid: u32, fields: Value) -> io::Result<()> {
        if self.cut && fields["type"] == "user" {
            self.cut = false;
            self.append(
                pid,
                json!({
                    "type": "assistant",
                    "message": {
                        "id": format!("{}-closed-{pid}", self.session),
                        "model": "<synthetic>",
                        "role": "assistant",
                        "stop_reason": "stop_sequence",
                        "content": [{ "type": "text", "text": NO_RESPONSE }],
                    },
                }),
            )?;
        }
        let record = self.record(pid, fields);
        write_line(&mut self.open()?, &record)
    }

    /// Writes the first half of a record and no more, as a short write leaves one
    /// (a full disk, a process killed in a large write): the next record's append
    /// makes one line of it and itself, which is not JSON, for good.
    pub fn cut_short(&mut self, pid: u32, fields: Value) -> io::Result<()> {
        let line = self.record(pid, fields).to_string();
        self.open()?.write_all(&line.as_bytes()[..line.len() / 2])
    }

    /// The record `fields` make, numbered as the next.
    fn record(&mut self, pid: u32, fields: Value) -> Value {
        self.ordinal += 1;
        self.said |= fields["type"] == "user";
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
        Value::Object(record)
    }

    /// The transcript, open to be appended to.
    fn open(&self) -> io::Result<fs::File> {
        make_folder_of(&self.transcript)?;
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.transcript)
    }

    /// Writes the status the window is in: `idle` or `busy`.
    pub fn status(&self, pid: u32, value: &str) -> io::Result<()> {
        make_folder_of(&self.status_file)?;
        fs::write(
            &self.status_file,
            json!({ "pid": pid, "sessionId": self.session, "kind": "interactive", "status": value })
                .to_string(),
        )
    }
}

/// Makes the folder `file` is in, where it is not: Claude makes its own where it writes. The
/// folder is the machine's Claude folder's, which every run of the suites shares, and a run
/// that ends takes the folder of the stand-ins' conversations when it is empty (`rig::forget`),
/// as it is for a chief that has heard nothing yet: a window that did not make it again ended
/// itself with the first thing it was told.
fn make_folder_of(file: &Path) -> io::Result<()> {
    file.parent().map_or(Ok(()), fs::create_dir_all)
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
        assert_eq!(records.last_uuid(4242), "sess-1-4242-2");
        let at = written[0]["timestamp"].as_str().unwrap();
        assert!(
            at.len() == 24 && at.ends_with('Z') && at.as_bytes()[10] == b'T',
            "{at}"
        );
        assert_eq!(records.ordinal(), 2);
    }

    /// The `type` of each record of a conversation, in order.
    fn types(file: &Path) -> Vec<String> {
        lines(file)
            .iter()
            .map(|record| record["type"].as_str().unwrap().to_owned())
            .collect()
    }

    fn user(text: &str) -> Value {
        json!({ "type": "user", "message": { "role": "user", "content": text } })
    }

    #[test]
    fn a_conversation_cut_in_a_turn_is_resumed_waiting_and_closes_it_just_before_the_next_message()
    {
        let config = tempfile::tempdir().unwrap();
        let file = config.path().join("projects/integration/sess-1.jsonl");
        let mut cut = Records::new(config.path(), "sess-1", 11).unwrap();
        cut.append(11, user("first")).unwrap();

        // The window the app opens again writes nothing of that turn.
        let mut again = Records::new(config.path(), "sess-1", 22).unwrap();
        assert_eq!(types(&file), ["user"]);

        again.append(22, user("second")).unwrap();
        assert_eq!(types(&file), ["user", "assistant", "user"]);
        let closed = &lines(&file)[1];
        assert_eq!(closed["message"]["model"], "<synthetic>");
        assert_eq!(closed["message"]["stop_reason"], "stop_sequence");
        assert_eq!(
            closed["message"]["content"][0]["text"],
            "No response requested."
        );

        // A turn is closed once.
        again.append(22, user("third")).unwrap();
        assert_eq!(types(&file), ["user", "assistant", "user", "user"]);
    }

    #[test]
    fn a_conversation_that_ended_in_an_answer_or_in_an_interrupt_has_no_turn_to_close() {
        let answered = tempfile::tempdir().unwrap();
        let mut first = Records::new(answered.path(), "s", 1).unwrap();
        first.append(1, user("first")).unwrap();
        first
            .append(
                1,
                json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "done" }] } }),
            )
            .unwrap();
        let mut again = Records::new(answered.path(), "s", 2).unwrap();
        again.append(2, user("second")).unwrap();
        assert_eq!(
            types(&answered.path().join("projects/integration/s.jsonl")),
            ["user", "assistant", "user"]
        );

        let interrupted = tempfile::tempdir().unwrap();
        let mut first = Records::new(interrupted.path(), "s", 1).unwrap();
        first.append(1, user("first")).unwrap();
        first
            .append(
                1,
                json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "text", "text": INTERRUPTED }] } }),
            )
            .unwrap();
        let mut again = Records::new(interrupted.path(), "s", 2).unwrap();
        again.append(2, user("second")).unwrap();
        assert_eq!(
            types(&interrupted.path().join("projects/integration/s.jsonl")),
            ["user", "user", "user"]
        );

        // A conversation with nothing in it has nothing cut either.
        let fresh = tempfile::tempdir().unwrap();
        let mut window = Records::new(fresh.path(), "s", 1).unwrap();
        window.append(1, user("first")).unwrap();
        assert_eq!(
            types(&fresh.path().join("projects/integration/s.jsonl")),
            ["user"]
        );
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
    fn a_record_cut_short_and_the_next_appended_to_it_are_one_line_that_is_not_json() {
        let config = tempfile::tempdir().unwrap();
        let mut records = Records::new(config.path(), "sess-1", 4242).unwrap();
        let said =
            |text: &str| json!({ "type": "user", "message": { "role": "user", "content": text } });
        records.append(4242, said("before")).unwrap();
        records
            .cut_short(4242, json!({ "type": "assistant" }))
            .unwrap();
        let written = config.path().join("projects/integration/sess-1.jsonl");
        assert!(
            !fs::read_to_string(&written).unwrap().ends_with('\n'),
            "half a line"
        );
        records.append(4242, said("next")).unwrap();
        records.append(4242, said("after")).unwrap();
        let text = fs::read_to_string(&written).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(
            serde_json::from_str::<Value>(lines[1]).is_err(),
            "{}",
            lines[1]
        );
        for whole in [lines[0], lines[2]] {
            serde_json::from_str::<Value>(whole).unwrap();
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
    fn a_window_whose_folders_were_taken_makes_them_again_where_it_writes_as_claude_does() {
        let config = tempfile::tempdir().unwrap();
        let mut records = Records::new(config.path(), "sess-1", 4242).unwrap();
        // The folder of the conversations goes with a rig of another run that ends, and the
        // folder of the status files with whoever tidies the machine's Claude folder.
        fs::remove_dir_all(config.path().join("projects")).unwrap();
        fs::remove_dir_all(config.path().join("sessions")).unwrap();
        records.append(4242, user("heard")).unwrap();
        records.status(4242, "idle").unwrap();
        assert_eq!(
            types(&config.path().join("projects/integration/sess-1.jsonl")),
            ["user"]
        );
        assert!(records.status_file().is_file());
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
