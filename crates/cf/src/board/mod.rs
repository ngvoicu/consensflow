//! `cf` inside a window the daemon opened: the agents' commands on the
//! board, each a request to the daemon's API as the window's participant,
//! answered in a sentence or, with `--json` anywhere, as the API's JSON.
//! A command written wrong exits 2, one the board refused or could not take
//! exits 1.

mod cut;
mod lines;
mod task;
mod usage;
mod words;

use std::io::{self, Read, Write};

use cf_board::{Board, BoardError};
use serde_json::{json, Map, Value};

use cf_base::js;
use lines::{list, message_line};
use usage::usage;
use words::{message_id, quoted, require_text, split, task_number};

/// Why a command did not do what it was asked.
#[derive(Debug)]
pub enum Failure {
    /// The command was written wrong.
    Usage(String),
    /// The board refused it or could not be reached, or its text could not be read.
    Failed(String),
}

impl From<BoardError> for Failure {
    fn from(cause: BoardError) -> Self {
        Failure::Failed(cause.to_string())
    }
}

/// What a command answers: the API's data for `--json`, a sentence otherwise,
/// and the answers the output carries whole, if it is one that does.
pub struct Said {
    data: Value,
    text: String,
    wrote: Option<Wrote>,
}

/// The answers a command's output carries whole, and how it was read: what
/// `cf` says to the board, once the output is complete, to have them received.
struct Wrote {
    via: &'static str,
    answers: Vec<i64>,
}

impl Said {
    fn new(data: Value, text: String) -> Self {
        Self {
            data,
            text,
            wrote: None,
        }
    }

    /// Of an output that carries the whole body of each of `answers` (one that
    /// cuts them, or leaves them out, has none to say): those are the ones it
    /// wrote whole, `via` the way it was read.
    fn having_written(mut self, via: &'static str, answers: Vec<i64>) -> Self {
        self.wrote = Some(Wrote { via, answers });
        self
    }
}

impl Wrote {
    /// The output is complete: the board is told which answers it carried
    /// whole, and an answer it says so of is received. What the board says back
    /// is of no use to a command that has printed what it was asked to (a
    /// daemon of Node's knows no such route): it is not raised, and an answer
    /// not acknowledged waits to be pasted, as an unread one does.
    fn acknowledge(&self, board: &Board) {
        if self.answers.is_empty() {
            return;
        }
        let _ = board.post(
            "/api/answers/read",
            &json!({ "answers": self.answers, "via": self.via }),
        );
    }
}

/// Runs the command in `words` against `board`, reading a `-` text from
/// `input`, answering with the API's JSON when `json` asks: the exit code.
/// Only a failure to write `out` or `err` is an error. The answers an output
/// carried whole are acknowledged to the board once all of it is written, and
/// not when it was not: a read is not a receipt. Nor when the output is one
/// that a harness may cut before its model reads it (`cut`): what was
/// printed is measured, the text or the JSON, whichever it was.
pub fn run(
    words: &[String],
    json: bool,
    board: &Board,
    input: &mut dyn Read,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<u8> {
    match command(words, board, input) {
        Ok(Said { data, text, wrote }) => {
            let printed = if json {
                let data = cf_base::json::js_order(data);
                format!("{}\n", serde_json::to_string_pretty(&data)?)
            } else {
                format!("{text}\n")
            };
            out.write_all(printed.as_bytes())?;
            out.flush()?;
            if let Some(wrote) = wrote.filter(|_| cut::seen_whole(&printed)) {
                wrote.acknowledge(board);
            }
            Ok(0)
        }
        Err(Failure::Usage(message)) => {
            writeln!(err, "cf: {message}")?;
            Ok(2)
        }
        Err(Failure::Failed(message)) => {
            writeln!(err, "cf: {message}")?;
            Ok(1)
        }
    }
}

fn command(words: &[String], board: &Board, input: &mut dyn Read) -> Result<Said, Failure> {
    let (verb, rest) = match words.split_first() {
        Some((verb, rest)) => (verb.as_str(), rest),
        None => ("help", words),
    };
    match verb {
        "help" | "--help" | "-h" => Ok(Said::new(json!({ "usage": usage() }), usage().to_string())),
        "task" => task::command(rest, board, input),
        "inbox" => inbox(rest, board),
        "note" => note(rest, board, input),
        "ask" => ask(rest, board, input),
        "tell" => tell(rest, board, input),
        "answer" => answer(rest, board, input),
        "staff" => staff(board),
        "history" => history(rest, board),
        "whoami" => whoami(board),
        other => Err(Failure::Usage(format!(
            "unknown command {}: use task, inbox, ask, note, tell, answer, staff, whoami or history",
            quoted(other)
        ))),
    }
}

/// `cf inbox read m-N` prints a message whole, and so says of an answer that it
/// wrote it whole. `cf inbox` lists first lines, cut, and says nothing of what
/// it printed: no preview is a body, and a list does not know which are.
fn inbox(rest: &[String], board: &Board) -> Result<Said, Failure> {
    if rest.first().map(String::as_str) == Some("read") {
        let id = message_id(rest.get(1).map(String::as_str))?;
        let path = format!("/api/inbox/{id}");
        let message = board.get(&path)?.take("message")?;
        let text = format!(
            "{}\n\n{}",
            message_line(&message),
            js::text(message.get("body"))
        );
        let answers = lines::waiting_answers(std::slice::from_ref(&message));
        return Ok(Said::new(message, text).having_written("inbox", answers));
    }
    let mut answer = board.get("/api/inbox")?;
    let messages = answer.take("messages")?;
    let text = match answer.list(Some(&messages), "messages")? {
        [] => "Your inbox is empty.".to_string(),
        all => all.iter().map(message_line).collect::<Vec<_>>().join("\n"),
    };
    Ok(Said::new(messages, text))
}

fn note(rest: &[String], board: &Board, input: &mut dyn Read) -> Result<Said, Failure> {
    let words = split(rest, &["--human"], &[]);
    let human = words.on("--human");
    let mut body = Map::new();
    body.insert(
        "body".into(),
        require_text(text_of(words.text, input)?, "cf note \"what to know\"")?.into(),
    );
    if human {
        body.insert("to".into(), "human".into());
    }
    let message = posted(board, "/api/notes", body)?;
    let text = format!(
        "m-{} noted to @{}; nothing waits on it.",
        js::text(message.get("id")),
        js::text(message.get("recipient"))
    );
    Ok(Said::new(message, text))
}

fn ask(rest: &[String], board: &Board, input: &mut dyn Read) -> Result<Said, Failure> {
    // Nobody asks the human on the board: the chief asks them in its terminal.
    if rest.iter().any(|word| word == "--human") {
        return Err(Failure::Usage(
            "the human is not asked with cf ask: the chief asks them in its own terminal".into(),
        ));
    }
    let question = require_text(text_of(rest.join(" "), input)?, "cf ask \"your question\"")?;
    let message = posted(board, "/api/questions", body_of(question))?;
    let text = format!(
        "m-{} asked @{}. The answer arrives as a message; end your turn now.",
        js::text(message.get("id")),
        js::text(message.get("recipient"))
    );
    Ok(Said::new(message, text))
}

fn tell(rest: &[String], board: &Board, input: &mut dyn Read) -> Result<Said, Failure> {
    let number = task_number(rest.first().map(String::as_str))?;
    let words = rest.get(1..).unwrap_or_default().join(" ");
    let what = require_text(
        text_of(words, input)?,
        "cf tell T-<n> \"what to put to its window now\"",
    )?;
    let message = posted(board, &format!("/api/tasks/{number}/tell"), body_of(what))?;
    let text = format!(
        "T-{number} is paused and m-{} put to @{}; its answer arrives as a message. Then: cf task resume T-{number} \"…\"",
        js::text(message.get("id")),
        js::text(message.get("recipient"))
    );
    Ok(Said::new(message, text))
}

fn answer(rest: &[String], board: &Board, input: &mut dyn Read) -> Result<Said, Failure> {
    let id = message_id(rest.first().map(String::as_str))?;
    let words = rest.get(1..).unwrap_or_default().join(" ");
    let mut body = Map::new();
    body.insert("question".into(), id.into());
    body.insert(
        "body".into(),
        require_text(text_of(words, input)?, "cf answer m-<id> \"your answer\"")?.into(),
    );
    let message = posted(board, "/api/answers", body)?;
    let gated = message.get("state").and_then(Value::as_str) == Some("gated");
    let text = format!(
        "m-{} answered @{}{}",
        js::text(message.get("id")),
        js::text(message.get("recipient")),
        if gated {
            "; the human passes it on first."
        } else {
            "."
        }
    );
    Ok(Said::new(message, text))
}

fn staff(board: &Board) -> Result<Said, Failure> {
    let mut answer = board.get("/api/staff")?;
    let members = answer.take("members")?;
    let text = match answer.list(Some(&members), "members")? {
        [] => {
            "No agents are on this project staff yet; the human adds them in the app.".to_string()
        }
        all => all
            .iter()
            .map(|member| {
                format!(
                    "@{} · {} · {}",
                    js::text(member.get("handle")),
                    js::join(list(member.get("roles")), "+"),
                    js::text(member.get("tier"))
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
    };
    Ok(Said::new(members, text))
}

fn history(rest: &[String], board: &Board) -> Result<Said, Failure> {
    let words = split(rest, &["--tools"], &["--page", "--find"]);
    let mut query = form_urlencoded::Serializer::new(String::new());
    if let Some(page) = words.value("--page") {
        query.append_pair("page", page);
    }
    if let Some(find) = words.value("--find") {
        query.append_pair("find", find);
    }
    if words.on("--tools") {
        query.append_pair("tools", "1");
    }
    let query = query.finish();
    let path = if query.is_empty() {
        "/api/history".to_string()
    } else {
        format!("/api/history?{query}")
    };
    let page = board.get(&path)?.into_value();
    let text = js::text(page.get("text")).into_owned();
    Ok(Said::new(page, text))
}

fn whoami(board: &Board) -> Result<Said, Failure> {
    let answer = board.get("/api/whoami")?;
    let me = answer.value();
    let participant = me.get("participant");
    let task = match me.get("task") {
        Some(Value::Null) => String::new(),
        Some(task) => {
            format!(
                ", on T-{}: {}",
                js::text(task.get("number")),
                js::text(task.get("title"))
            )
        }
        None => return Err(answer.lacks("task").into()),
    };
    let text = format!(
        "@{} ({}) in project {}{task}",
        js::text(participant.and_then(|it| it.get("handle"))),
        js::text(participant.and_then(|it| it.get("role"))),
        js::text(me.get("project").and_then(|it| it.get("name"))),
    );
    Ok(Said::new(answer.into_value(), text))
}

/// The text a command was given, or standard input when it is `-`: a brief
/// in a quoted heredoc (`cf task add --tier standard - <<'BRIEF'`) reaches
/// the board as written, where the shell would run the backticks and `$( )`
/// of a double-quoted one. The final newline is the heredoc's, not the text's.
fn text_of(text: String, input: &mut dyn Read) -> Result<String, Failure> {
    if text != "-" {
        return Ok(text);
    }
    let mut bytes = Vec::new();
    input
        .read_to_end(&mut bytes)
        .map_err(|cause| Failure::Failed(format!("cannot read standard input ({cause})")))?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(text.strip_suffix('\n').unwrap_or(&text).to_string())
}

/// `{ body }`, the request most commands send.
fn body_of(text: String) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("body".into(), text.into());
    body
}

/// POSTs `body` to `path`: the message the API answers with.
fn posted(board: &Board, path: &str, body: Map<String, Value>) -> Result<Value, Failure> {
    Ok(board.post(path, &Value::Object(body))?.take("message")?)
}

#[cfg(test)]
mod tests;
