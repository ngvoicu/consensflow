//! A transcript built like a big one, for the measurements and for the tests
//! that need a long one. It is made of turns: a prompt, attachments, a loop
//! of tool calls (each assistant message written in fragments that repeat its
//! usage, each result carrying the tool's output a second time as
//! `toolUseResult`), the answer, and the boundary record; between them the
//! small records Claude Code writes, queue operations, and snapshots of the
//! files touched. The shapes are Claude Code's and the text is invented; the
//! sizes follow a 367 MB transcript read for the purpose (162,245 lines of
//! 2.3 KB on average, the longest 1.1 MB, most of it under keys no reader
//! reads: a message's usage and thinking, a tool's input and its result).

use std::fmt::Write as _;
use std::io::{self, Write};

/// What to build: about this many lines, whole turns, from this seed.
pub(super) struct Plan {
    pub(super) lines: usize,
    pub(super) seed: u64,
}

/// What was built.
pub(super) struct Made {
    pub(super) lines: usize,
    pub(super) bytes: u64,
    /// The uuid of the first turn's boundary record. The next turn's prompt
    /// had a decision look it up, so a record that claims it again has the
    /// transcript read again.
    pub(super) watched: String,
}

/// Text as JSON writes it inside a string: escapes of every kind, and
/// characters that take more than one byte.
const PIECES: [&str; 12] = [
    "The quick brown fox jumps over the lazy dog. ",
    r#"fn main() {\n    println!(\"hello\");\n}\n"#,
    r"error[E0382]: borrow of moved value: `rows`\n  --> src/lib.rs:12:5\n",
    r"\u001b[31mFAIL\u001b[0m tests/records.test.mjs\n",
    "caf\u{e9} na\u{ef}ve \u{65e5}\u{672c}\u{8a9e} \u{1f600} ",
    r"C:\\Users\\dev\\file.txt\t",
    "records are read in order and replayed once all of them are in. ",
    r#"{\"type\":\"user\",\"uuid\":\"u1\"}\n"#,
    "a turn is settled once the transcript says it ended. ",
    r"     12\t  let looked = read_on(file, seen, visit, None)?;\n",
    "Resets in 2 hours. Weekly usage limit reached. ",
    "the daemon looks at the same conversations every second. ",
];

const MODEL: &str = "claude-opus-5";

/// xorshift64*: the same transcript from the same seed, on every machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    /// An integer from `low` to `high`, both included.
    fn between(&mut self, low: usize, high: usize) -> usize {
        low + self.below((high - low + 1) as u64) as usize
    }

    /// Whether an event of the given odds, one in `one_in`, happened.
    fn one_in(&mut self, one_in: u64) -> bool {
        self.below(one_in) == 0
    }
}

struct Maker<W> {
    out: W,
    session: String,
    rng: Rng,
    line: String,
    lines: usize,
    bytes: u64,
    clock: i64,
    /// The latest record of the conversation: the parent of the next.
    parent: String,
    /// The message queued in a turn, which the next turn's prompt is.
    queued: Option<String>,
    watched: Option<String>,
}

/// Builds the transcript of `session` in `out`, one line at a time.
pub(super) fn write(out: impl Write, session: &str, plan: &Plan) -> io::Result<Made> {
    let mut maker = Maker {
        out,
        session: session.to_owned(),
        rng: Rng(plan.seed | 1),
        line: String::new(),
        lines: 0,
        bytes: 0,
        clock: 1_788_800_000_000,
        parent: String::new(),
        queued: None,
        watched: None,
    };
    let mut turn = 0;
    while maker.lines < plan.lines {
        let last = maker.lines + 80 >= plan.lines;
        maker.turn(turn, last)?;
        turn += 1;
    }
    maker.out.flush()?;
    Ok(Made {
        lines: maker.lines,
        bytes: maker.bytes,
        watched: maker.watched.unwrap_or_default(),
    })
}

impl<W: Write> Maker<W> {
    fn uuid(&mut self) -> String {
        let (a, b) = (self.rng.next(), self.rng.next());
        format!(
            "{:08x}-{:04x}-4{:03x}-a{:03x}-{:012x}",
            a >> 32,
            (a >> 16) & 0xffff,
            a & 0xfff,
            (b >> 52) & 0xfff,
            b & 0xffff_ffff_ffff
        )
    }

    /// An id of the given prefix, as the API writes them.
    fn api_id(&mut self, prefix: &str) -> String {
        const DIGITS: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZabcdefghijkmnpqrstvwxyz";
        let tail: String = (0..22)
            .map(|_| char::from(DIGITS[self.rng.below(DIGITS.len() as u64) as usize]))
            .collect();
        format!("{prefix}01{tail}")
    }

    fn stamp(&mut self) -> String {
        self.clock += i64::try_from(self.rng.between(5, 4000)).unwrap_or(5);
        jiff::Timestamp::from_millisecond(self.clock)
            .map(|time| time.to_string())
            .unwrap_or_default()
    }

    /// Escaped text of about `length` bytes.
    fn text(&mut self, length: usize) -> String {
        let mut text = String::with_capacity(length + 80);
        while text.len() < length {
            text.push_str(PIECES[self.rng.below(PIECES.len() as u64) as usize]);
        }
        text
    }

    /// A length: mostly about `typical`, rarely a whole file's worth.
    fn size(&mut self, typical: usize) -> usize {
        match self.rng.below(2000) {
            0 => self.rng.between(300_000, 1_100_000),
            1..=40 => self.rng.between(typical * 4, typical * 12),
            _ => self.rng.between(typical / 4, typical * 3 / 2),
        }
    }

    /// The line built so far, written out.
    fn emit(&mut self) -> io::Result<()> {
        self.line.push('\n');
        self.out.write_all(self.line.as_bytes())?;
        self.bytes += self.line.len() as u64;
        self.lines += 1;
        self.line.clear();
        Ok(())
    }

    /// The fields every message record ends with.
    fn tail(&mut self) {
        let session = &self.session;
        let _ = write!(
            self.line,
            r#","userType":"external","entrypoint":"cli","cwd":"/Users/dev/Projects/synthetic","sessionId":"{session}","version":"2.1.263","gitBranch":"main"}}"#
        );
    }

    /// A record of the conversation's chain: its opening (the fields after
    /// are the caller's), and its uuid.
    fn chained(&mut self) -> String {
        let (parent, uuid) = (std::mem::take(&mut self.parent), self.uuid());
        let _ = write!(
            self.line,
            r#"{{"parentUuid":"{parent}","isSidechain":false"#
        );
        self.parent = uuid.clone();
        uuid
    }

    fn small(&mut self, fields: &str) -> io::Result<()> {
        let session = &self.session;
        let _ = write!(self.line, r#"{{{fields},"sessionId":"{session}"}}"#);
        self.emit()
    }

    /// The small records between the messages, as many as `count`.
    fn noise(&mut self, count: usize) -> io::Result<()> {
        for _ in 0..count {
            let leaf = self.uuid();
            let words = self.text(80);
            let fields = match self.rng.below(8) {
                0 => r#""type":"mode","mode":"normal""#.to_owned(),
                1 => r#""type":"permission-mode","permissionMode":"auto""#.to_owned(),
                2 => r#""type":"atis-latch","atis":"""#.to_owned(),
                3 => format!(r#""type":"last-prompt","lastPrompt":"{words}","leafUuid":"{leaf}""#),
                4 => format!(r#""type":"ai-title","aiTitle":"{words}""#),
                5 => format!(r#""type":"bridge-session","bridgeSessionId":"session_{leaf}""#),
                6 => format!(r#""type":"frame-link","frameId":"{leaf}","parentFrameId":null"#),
                _ => r#""type":"history-suppression","suppressed":false"#.to_owned(),
            };
            self.small(&fields)?;
        }
        Ok(())
    }

    /// A snapshot of the files a session touched.
    fn snapshot(&mut self) -> io::Result<()> {
        let (message, at) = (self.api_id("msg_"), self.stamp());
        let mut backups = String::new();
        for file in 0..self.rng.between(80, 220) {
            let _ = write!(
                backups,
                r#"{}"src/module{file}/file{file}.ts":{{"backupFileName":"{:016x}@v{}","version":{},"backupTime":"{at}"}}"#,
                if backups.is_empty() { "" } else { "," },
                self.rng.next(),
                file % 7 + 1,
                file % 7 + 1
            );
        }
        let fields = format!(
            r#""type":"file-history-snapshot","messageId":"{message}","snapshot":{{"messageId":"{message}","trackedFileBackups":{{{backups}}},"timestamp":"{at}"}},"isSnapshotUpdate":false"#
        );
        self.small(&fields)
    }

    /// An operation on the queue of messages.
    fn queue(&mut self, operation: &str, content: Option<&str>) -> io::Result<()> {
        let at = self.stamp();
        let content = content.map_or_else(String::new, |text| format!(r#","content":"{text}""#));
        let fields = format!(
            r#""type":"queue-operation","operation":"{operation}","timestamp":"{at}"{content}"#
        );
        self.small(&fields)
    }

    fn attachment(&mut self, hook: bool) -> io::Result<()> {
        let uuid = self.chained();
        let at = self.stamp();
        let body = if hook {
            let context = self.text(300);
            format!(
                r#"{{"type":"hook_additional_context","content":["{context}"],"hookName":"UserPromptSubmit","toolUseID":"hook-1","hookEvent":"UserPromptSubmit"}}"#
            )
        } else {
            let (names, listing) = (self.size(250), self.size(400));
            let (names, listing) = (self.text(names), self.text(listing));
            format!(
                r#"{{"type":"skill_listing","content":"{listing}","names":["{names}"],"skillCount":12,"isInitial":false}}"#
            )
        };
        let rendered = if self.rng.one_in(3) {
            let length = self.size(900);
            format!(r#","rendered":"{}""#, self.text(length))
        } else {
            String::new()
        };
        let _ = write!(
            self.line,
            r#","attachment":{body},"type":"attachment","uuid":"{uuid}","timestamp":"{at}"{rendered}"#
        );
        self.tail();
        self.emit()
    }

    fn prompt(&mut self, queued: Option<String>) -> io::Result<()> {
        let uuid = self.chained();
        let (at, prompt_id) = (self.stamp(), self.uuid());
        let (text, source) = match queued {
            Some(text) => (text, "queued"),
            None => {
                let length = self.rng.between(80, 1500);
                (self.text(length), "typed")
            }
        };
        let _ = write!(
            self.line,
            r#","promptId":"{prompt_id}","type":"user","message":{{"role":"user","content":"{text}"}},"uuid":"{uuid}","timestamp":"{at}","permissionMode":"auto","origin":{{"kind":"human"}},"promptSource":"{source}""#
        );
        self.tail();
        self.emit()
    }

    /// One fragment of the assistant's message `message`: the content block
    /// it holds, how the message stopped, and the tool input the wire kept.
    fn fragment(
        &mut self,
        message: &str,
        index: usize,
        block: &str,
        stop: &str,
        wire: Option<(&str, &str)>,
    ) -> io::Result<String> {
        let uuid = self.chained();
        let (at, request) = (self.stamp(), self.api_id("req_"));
        let (input, cached) = (self.rng.between(2, 90), self.rng.between(10_000, 250_000));
        let (output, session) = (self.rng.between(20, 4000), self.session.clone());
        let _ = write!(
            self.line,
            r#","message":{{"model":"{MODEL}","id":"{message}","type":"message","role":"assistant","content":[{block}],"stop_reason":{stop},"stop_sequence":null,"stop_details":null,"usage":{{"input_tokens":{input},"cache_creation_input_tokens":4633,"cache_read_input_tokens":{cached},"output_tokens":{output},"output_tokens_details":{{"thinking_tokens":312}},"server_tool_use":{{"web_search_requests":0,"web_fetch_requests":0}},"service_tier":"standard","cache_creation":{{"ephemeral_1h_input_tokens":3134,"ephemeral_5m_input_tokens":0}},"inference_geo":"not_available","iterations":[{{"input_tokens":2,"output_tokens":294,"cache_read_input_tokens":110915,"cache_creation_input_tokens":3134,"cache_creation":{{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":3134}},"type":"message"}},{{"input_tokens":{cached},"output_tokens":{output},"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"cache_creation":{{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":0}},"type":"advisor_message","model":"claude-fable-5-1"}}],"speed":"standard"}},"diagnostics":null}},"apiBlockIndex":{index},"requestId":"{request}","type":"assistant","uuid":"{uuid}","timestamp":"{at}""#
        );
        if let Some((call, input)) = wire {
            let _ = write!(self.line, r#","wireToolInputs":{{"{call}":{input}}}"#);
        }
        let _ = write!(
            self.line,
            r#","advisorModel":"claude-fable-5-1","effort":"max","session_id":"{session}""#
        );
        self.tail();
        self.emit()?;
        Ok(uuid)
    }

    /// A tool's result, as the user's record that carries it.
    fn result(&mut self, call: &str, made_by: &str, output: usize) -> io::Result<()> {
        let uuid = self.chained();
        let (at, prompt_id, session) = (self.stamp(), self.uuid(), self.session.clone());
        let (said, again) = (self.text(output), self.text(output / 2));
        let _ = write!(
            self.line,
            r#","promptId":"{prompt_id}","type":"user","message":{{"role":"user","content":[{{"tool_use_id":"{call}","type":"tool_result","content":"{said}","is_error":false}}]}},"uuid":"{uuid}","timestamp":"{at}","toolUseResult":{{"stdout":"{said}","stderr":"","interrupted":false,"isImage":false,"noOutputExpected":false,"structured":{{"type":"text","file":{{"filePath":"/Users/dev/Projects/synthetic/src/lib.rs","content":"{again}","numLines":{output},"startLine":1,"totalLines":{output}}}}}}},"sourceToolAssistantUUID":"{made_by}","serverClassifierContext":{{"mode":"auto","decision":"allow","reason":"read-only tool"}},"session_id":"{session}""#
        );
        self.tail();
        self.emit()
    }

    /// A message of the assistant that ends in a call to a tool, and the
    /// result of the call.
    fn step(&mut self, advisor: bool) -> io::Result<()> {
        let message = self.api_id("msg_");
        let (thinking, signature) = (self.size(3500), self.text(1400));
        let thinking = self.text(thinking);
        let block =
            format!(r#"{{"type":"thinking","thinking":"{thinking}","signature":"{signature}"}}"#);
        self.fragment(&message, 0, &block, r#""tool_use""#, None)?;
        if self.rng.one_in(3) {
            let length = self.size(400);
            let block = format!(r#"{{"type":"text","text":"{}"}}"#, self.text(length));
            self.fragment(&message, 1, &block, r#""tool_use""#, None)?;
        }
        if advisor {
            let call = self.api_id("srvtoolu_");
            let block = format!(
                r#"{{"type":"server_tool_use","id":"{call}","name":"advisor","input":{{}}}}"#
            );
            self.fragment(&message, 2, &block, r#""tool_use""#, None)?;
            let sealed = self.text(4000);
            let block = format!(
                r#"{{"type":"advisor_tool_result","tool_use_id":"{call}","content":{{"type":"advisor_redacted_result","encrypted_content":"{sealed}"}}}}"#
            );
            self.fragment(&message, 3, &block, r#""tool_use""#, None)?;
        }
        let call = self.api_id("toolu_");
        let length = self.size(1500);
        let command = self.text(length);
        let input = format!(r#"{{"command":"{command}","description":"Run the checks"}}"#);
        let block = format!(
            r#"{{"type":"tool_use","id":"{call}","name":"Bash","input":{input},"caller":{{"type":"direct"}}}}"#
        );
        let made_by = self.fragment(&message, 4, &block, r#""tool_use""#, Some((&call, &input)))?;
        let output = self.size(1800);
        self.result(&call, &made_by, output)
    }

    /// A turn: the prompt (the one the queue held, after a turn that queued
    /// it), the work, the answer, and the boundary record. The last turn
    /// leaves nothing open.
    fn turn(&mut self, number: usize, last: bool) -> io::Result<()> {
        let queued = self.queued.take();
        if queued.is_some() {
            self.queue("dequeue", None)?;
        }
        self.prompt(queued)?;
        self.noise(3)?;
        let attachments = self.rng.between(2, 9);
        for at in 0..attachments {
            self.attachment(at == 0 && number % 6 == 1)?;
        }
        let steps = self.rng.between(1, 9);
        for step in 0..steps {
            self.step(step == 0 && number % 5 == 2)?;
            let between = self.rng.between(1, 4);
            self.noise(between)?;
            if step == 0 && number % 3 == 2 && !last {
                let text = self.text(120);
                self.queue("enqueue", Some(&text))?;
                self.queued = Some(text);
            }
            if self.rng.one_in(60) {
                self.snapshot()?;
            }
        }
        let message = self.api_id("msg_");
        let length = self.size(800);
        let block = format!(r#"{{"type":"text","text":"{}"}}"#, self.text(length));
        self.fragment(&message, 0, &block, r#""end_turn""#, None)?;
        self.noise(2)?;
        self.boundary(number)
    }

    /// The system's record that the turn is over, a child of the answer.
    fn boundary(&mut self, number: usize) -> io::Result<()> {
        let uuid = self.chained();
        let (at, took) = (self.stamp(), self.rng.between(2000, 900_000));
        let count = self.rng.between(5, 400);
        let _ = write!(
            self.line,
            r#","type":"system","subtype":"turn_duration","durationMs":{took},"messageCount":{count},"timestamp":"{at}","uuid":"{uuid}","isMeta":false"#
        );
        self.tail();
        self.emit()?;
        if number == 0 {
            self.watched = Some(uuid);
        }
        Ok(())
    }
}
