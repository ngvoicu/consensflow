//! `cf` inside a window the daemon opened: the agents' commands on the
//! board, each a request to the daemon's API as the window's participant,
//! answered in a sentence or, with `--json` anywhere, as the API's JSON.
//! A command written wrong exits 2, one the board refused or could not take
//! exits 1.

mod lines;
mod task;
mod usage;
mod words;

use std::io::{self, Read, Write};

use cf_board::{Board, BoardError, Method};
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

/// What a command answers: the API's data for `--json`, a sentence otherwise.
pub struct Said {
    data: Value,
    text: String,
}

/// Runs the command in `args` against `board`, reading a `-` text from
/// `input`: the exit code. Only a failure to write `out` or `err` is an error.
pub fn run(
    args: &[String],
    board: &Board,
    input: &mut dyn Read,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<u8> {
    let json = args.iter().any(|arg| arg == "--json");
    let words: Vec<String> = args
        .iter()
        .filter(|arg| *arg != "--json")
        .cloned()
        .collect();
    match command(&words, board, input) {
        Ok(said) => {
            if json {
                let data = cf_base::json::js_order(said.data);
                writeln!(out, "{}", serde_json::to_string_pretty(&data)?)?;
            } else {
                writeln!(out, "{}", said.text)?;
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
        "help" | "--help" | "-h" => Ok(Said { data: json!({ "usage": usage() }), text: usage().to_string() }),
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

fn inbox(rest: &[String], board: &Board) -> Result<Said, Failure> {
    if rest.first().map(String::as_str) == Some("read") {
        let id = message_id(rest.get(1).map(String::as_str))?;
        let path = format!("/api/inbox/{id}");
        let message = field(board.call(Method::Get, &path, None)?, "message", &path)?;
        let text = format!(
            "{}\n\n{}",
            message_line(&message),
            js::text(message.get("body"))
        );
        return Ok(Said {
            data: message,
            text,
        });
    }
    let messages = field(
        board.call(Method::Get, "/api/inbox", None)?,
        "messages",
        "/api/inbox",
    )?;
    let text = match list(Some(&messages)) {
        [] => "Your inbox is empty.".to_string(),
        all => all.iter().map(message_line).collect::<Vec<_>>().join("\n"),
    };
    Ok(Said {
        data: messages,
        text,
    })
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
    Ok(Said {
        data: message,
        text,
    })
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
    Ok(Said {
        data: message,
        text,
    })
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
    Ok(Said {
        data: message,
        text,
    })
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
    Ok(Said {
        data: message,
        text,
    })
}

fn staff(board: &Board) -> Result<Said, Failure> {
    let members = field(
        board.call(Method::Get, "/api/staff", None)?,
        "members",
        "/api/staff",
    )?;
    let text = match list(Some(&members)) {
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
    Ok(Said {
        data: members,
        text,
    })
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
    let page = board.call(Method::Get, &path, None)?;
    let text = js::text(page.get("text")).into_owned();
    Ok(Said { data: page, text })
}

fn whoami(board: &Board) -> Result<Said, Failure> {
    let me = board.call(Method::Get, "/api/whoami", None)?;
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
        None => {
            return Err(BoardError::Malformed {
                path: "/api/whoami".into(),
                what: "task",
            }
            .into())
        }
    };
    let text = format!(
        "@{} ({}) in project {}{task}",
        js::text(participant.and_then(|it| it.get("handle"))),
        js::text(participant.and_then(|it| it.get("role"))),
        js::text(me.get("project").and_then(|it| it.get("name"))),
    );
    Ok(Said { data: me, text })
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
    field(
        board.call(Method::Post, path, Some(&Value::Object(body)))?,
        "message",
        path,
    )
}

/// The field `key` of the API's answer to `path`, borrowed.
fn part<'a>(answer: &'a Value, key: &'static str, path: &str) -> Result<&'a Value, Failure> {
    answer.get(key).ok_or_else(|| {
        BoardError::Malformed {
            path: path.to_string(),
            what: key,
        }
        .into()
    })
}

/// The field `key` of the API's answer to `path`.
fn field(mut answer: Value, key: &'static str, path: &str) -> Result<Value, Failure> {
    answer.get_mut(key).map(Value::take).ok_or_else(|| {
        BoardError::Malformed {
            path: path.to_string(),
            what: key,
        }
        .into()
    })
}
