//! The daemon's agents' door as a window's `cf` reaches it: requests to the
//! API at `CONSENSFLOW_URL` with the window's bearer token, synchronous, so a
//! `cf` run starts no runtime. A refusal (the daemon answered, and said no)
//! is told apart from a daemon that cannot be reached: a hook stays silent on
//! the second and passes the first on to its model.
//!
//! [`door`] is the question door a harness's question tool opens onto the
//! board.

#![forbid(unsafe_code)]

pub mod door;

use std::time::Duration;

use cf_base::env::Env;
use serde_json::{json, Value};

/// How long one request may take. Node's `fetch`, which this replaces, gave a
/// response five minutes; the longest poll the API holds is 25 seconds.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, thiserror::Error)]
pub enum BoardError {
    #[error("CONSENSFLOW_URL is not set: run cf from a window ConsensFlow opened")]
    NoUrl,
    #[error("ConsensFlow is not answering at {url} ({cause})")]
    Unreachable { url: String, cause: String },
    /// Answered, and refused: the API's own message, or its status, and the
    /// code the API gave it, when it gave one.
    #[error("{message}")]
    Refused {
        code: Option<String>,
        message: String,
    },
    /// Answered with a body that lacks what the call reads from it.
    #[error("ConsensFlow's answer to {path} has no {what}")]
    Malformed { path: String, what: &'static str },
}

impl BoardError {
    /// Whether the daemon answered and refused, as opposed to not answering at all.
    pub fn is_refusal(&self) -> bool {
        matches!(self, BoardError::Refused { .. })
    }

    /// The code the API gave a refusal (`door-closed`), when it gave one.
    pub fn code(&self) -> Option<&str> {
        match self {
            BoardError::Refused { code, .. } => code.as_deref(),
            _ => None,
        }
    }
}

/// What the API answered to one request, with the path it answered: what a
/// caller reads of it and does not find is said with that path.
#[derive(Debug)]
pub struct Answer {
    path: String,
    value: Value,
}

impl Answer {
    /// The answer whole.
    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn into_value(self) -> Value {
        self.value
    }

    /// Field `key` of the answer, which the caller reads.
    pub fn part(&self, key: &'static str) -> Result<&Value, BoardError> {
        self.value.get(key).ok_or_else(|| self.lacks(key))
    }

    /// Field `key`, taken out of the answer, which stays to say what else it lacks.
    pub fn take(&mut self, key: &'static str) -> Result<Value, BoardError> {
        match self.value.get_mut(key) {
            Some(value) => Ok(value.take()),
            None => Err(self.lacks(key)),
        }
    }

    /// The list `value`, a part of this answer named `what`, is: what Node
    /// read `.length` or `.map` of, and so threw on when the answer had none.
    pub fn list<'a>(
        &self,
        value: Option<&'a Value>,
        what: &'static str,
    ) -> Result<&'a [Value], BoardError> {
        value
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .ok_or_else(|| self.lacks(what))
    }

    /// That the answer has no `what`.
    pub fn lacks(&self, what: &'static str) -> BoardError {
        BoardError::Malformed {
            path: self.path.clone(),
            what,
        }
    }
}

/// One window's way into the daemon's API.
pub struct Board {
    url: Option<String>,
    token: String,
    agent: ureq::Agent,
}

impl Board {
    /// The API at `url` (none outside a window), as the participant `token` names.
    pub fn new(url: Option<&str>, token: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .http_status_as_error(false)
            // The daemon is on loopback: a proxy from the window's environment
            // (ureq reads one by default; Node's fetch never did) has no part in it.
            .proxy(None)
            .build()
            .into();
        Self {
            url: url.filter(|url| !url.is_empty()).map(str::to_string),
            token: token.to_string(),
            agent,
        }
    }

    /// The board of the window `env` is, none outside one: the API at
    /// `CONSENSFLOW_URL`, as the participant `CONSENSFLOW_TOKEN` names. A token
    /// is any value that says something, as Node tested it, whether or not it
    /// is UTF-8: one that is not is refused by the board rather than taken
    /// for no window, which would hand the command to `cf.mjs`, and back.
    pub fn from_env(env: &Env) -> Option<Self> {
        let token = env
            .os("CONSENSFLOW_TOKEN")
            .filter(|token| !token.is_empty())?;
        Some(Self::new(
            env.text("CONSENSFLOW_URL"),
            &token.to_string_lossy(),
        ))
    }

    /// GET `path` (with its query).
    pub fn get(&self, path: &str) -> Result<Answer, BoardError> {
        self.send(path, None)
    }

    /// POST `body` to `path`, as `JSON.stringify` wrote it: compact, keys in order.
    pub fn post(&self, path: &str, body: &Value) -> Result<Answer, BoardError> {
        self.send(path, Some(body))
    }

    /// A GET, or a POST of `body`: the response's JSON, `{}` when it has none.
    fn send(&self, path: &str, body: Option<&Value>) -> Result<Answer, BoardError> {
        let url = self.url.as_deref().ok_or(BoardError::NoUrl)?;
        let address = format!("{url}{path}");
        let authorization = format!("Bearer {}", self.token);
        let sent = match body {
            None => self
                .agent
                .get(&address)
                .header("authorization", &authorization)
                .header("content-type", "application/json")
                .call(),
            Some(body) => self
                .agent
                .post(&address)
                .header("authorization", &authorization)
                .header("content-type", "application/json")
                .send(body.to_string()),
        };
        let mut response = sent.map_err(|cause| BoardError::Unreachable {
            url: url.to_string(),
            cause: cause.to_string(),
        })?;
        let status = response.status().as_u16();
        // A body of any size, as Node read one (a task's whole thread can run
        // to megabytes); one that is no JSON reads as none, as Node's did.
        let value = response
            .body_mut()
            .with_config()
            .limit(u64::MAX)
            .read_to_vec()
            .ok()
            .and_then(|bytes| cf_base::json::from_slice_lossy(&bytes).ok())
            .unwrap_or_else(|| json!({}));
        if !(200..300).contains(&status) {
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .map_or_else(|| format!("ConsensFlow answered {status}"), str::to_string);
            let code = value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string);
            return Err(BoardError::Refused { code, message });
        }
        Ok(Answer {
            path: path.to_string(),
            value,
        })
    }
}

// A test fixture other crates' tests compile too: a failure in it is the test's.
#[cfg(any(test, feature = "test-support"))]
#[allow(clippy::expect_used, clippy::unwrap_used)]
pub mod scripted {
    //! A scripted daemon API on loopback, for tests: it answers each request
    //! with the next scripted reply, as written, and keeps what it was asked.

    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use serde_json::Value;

    /// One request as the API received it, its body as sent.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Received {
        pub method: String,
        pub path: String,
        pub authorization: Option<String>,
        pub body: Option<String>,
    }

    impl Received {
        /// The body read as JSON.
        pub fn json(&self) -> Option<Value> {
            self.body
                .as_deref()
                .map(|body| serde_json::from_str(body).expect("a JSON body"))
        }
    }

    /// One scripted reply: a status, a body as sent, and how long the API
    /// holds the request before it answers, as it holds a door's poll.
    pub struct Reply {
        status: u16,
        text: String,
        hold: Duration,
    }

    /// A reply of `body`, written as JSON.
    pub fn reply(status: u16, body: Value) -> Reply {
        reply_text(status, body.to_string())
    }

    /// A reply of `text` exactly.
    pub fn reply_text(status: u16, text: impl Into<String>) -> Reply {
        Reply {
            status,
            text: text.into(),
            hold: Duration::ZERO,
        }
    }

    impl Reply {
        pub fn held(self, hold: Duration) -> Reply {
            Reply { hold, ..self }
        }
    }

    /// The API, at `url` until its replies run out.
    pub struct ScriptedApi {
        pub url: String,
        received: Arc<Mutex<Vec<Received>>>,
    }

    impl ScriptedApi {
        /// What it was asked so far.
        pub fn received(&self) -> Vec<Received> {
            self.received.lock().expect("the requests").clone()
        }
    }

    /// Serves `replies`, one per connection, in order.
    pub fn scripted(replies: Vec<Reply>) -> ScriptedApi {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
        let url = format!("http://{}", listener.local_addr().expect("its address"));
        let received = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&received);
        thread::spawn(move || {
            for Reply { status, text, hold } in replies {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream.try_clone().expect("clone the stream"));
                let mut line = String::new();
                reader.read_line(&mut line).expect("request line");
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let path = parts.next().unwrap_or_default().to_string();
                let mut length = 0;
                let mut authorization = None;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).expect("header line");
                    let header = header.trim_end();
                    if header.is_empty() {
                        break;
                    }
                    let (name, value) = header.split_once(':').unwrap_or((header, ""));
                    match name.to_ascii_lowercase().as_str() {
                        "content-length" => length = value.trim().parse().unwrap_or(0),
                        "authorization" => authorization = Some(value.trim().to_string()),
                        _ => {}
                    }
                }
                let mut raw = vec![0; length];
                reader.read_exact(&mut raw).expect("request body");
                let body = (length > 0).then(|| String::from_utf8_lossy(&raw).into_owned());
                kept.lock().expect("the requests").push(Received {
                    method,
                    path,
                    authorization,
                    body,
                });
                thread::sleep(hold);
                let answer = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
                    text.len()
                );
                let mut stream = stream;
                stream.write_all(answer.as_bytes()).expect("reply");
            }
        });
        ScriptedApi { url, received }
    }
}

#[cfg(test)]
mod tests {
    use super::scripted::{reply, reply_text, scripted, Received};
    use super::*;

    #[test]
    fn a_post_sends_the_token_and_the_body_and_returns_the_answer() {
        let api = scripted(vec![reply(200, json!({ "task": { "number": 3 } }))]);
        let board = Board::new(Some(&api.url), "tok");
        let answer = board
            .post("/api/tasks", &json!({ "tier": "standard", "body": "x" }))
            .unwrap();
        assert_eq!(answer.value(), &json!({ "task": { "number": 3 } }));
        assert_eq!(
            api.received(),
            vec![Received {
                method: "POST".into(),
                path: "/api/tasks".into(),
                authorization: Some("Bearer tok".into()),
                body: Some(r#"{"tier":"standard","body":"x"}"#.into()),
            }]
        );
    }

    #[test]
    fn a_refusal_carries_the_apis_message_or_its_status_and_the_code_it_gave() {
        let api = scripted(vec![
            reply(
                409,
                json!({ "error": "not-yours", "message": "T-3 is not yours" }),
            ),
            reply(500, json!({})),
        ]);
        let board = Board::new(Some(&api.url), "tok");
        let refused = board.get("/api/tasks/3").unwrap_err();
        assert!(refused.is_refusal());
        assert_eq!(refused.to_string(), "T-3 is not yours");
        assert_eq!(refused.code(), Some("not-yours"));
        let bare = board.get("/api/tasks").unwrap_err();
        assert_eq!(bare.to_string(), "ConsensFlow answered 500");
        assert_eq!(bare.code(), None, "it gave none");
    }

    #[test]
    fn an_answer_cut_through_an_emoji_still_reads() {
        // `{ preview: 'cut 😀'.slice(0, 5) }` as Node's API writes it.
        let lone = format!(r#"{{"message":{{"preview":"cut {}ud83d"}}}}"#, '\\');
        let api = scripted(vec![reply_text(200, lone), reply_text(200, "no json")]);
        let board = Board::new(Some(&api.url), "tok");
        let answer = board.get("/api/inbox/3").unwrap();
        assert_eq!(
            answer.value(),
            &json!({ "message": { "preview": "cut \u{FFFD}" } })
        );
        assert_eq!(board.get("/api/inbox").unwrap().value(), &json!({}));
    }

    #[test]
    fn an_answer_of_many_megabytes_is_read_whole() {
        let body = "x".repeat(12 * 1024 * 1024);
        let api = scripted(vec![reply(200, json!({ "task": { "body": body } }))]);
        let answer = Board::new(Some(&api.url), "tok")
            .get("/api/tasks/3")
            .unwrap();
        assert_eq!(
            answer.value()["task"]["body"].as_str().map(str::len),
            Some(12 * 1024 * 1024)
        );
    }

    #[test]
    fn what_an_answer_lacks_is_said_with_the_path_it_answered() {
        let api = scripted(vec![reply(200, json!({ "board": { "lanes": 3 } }))]);
        let mut answer = Board::new(Some(&api.url), "tok").get("/api/tasks").unwrap();
        let board = answer.take("board").unwrap();
        assert_eq!(
            answer
                .list(board.get("lanes"), "lanes")
                .unwrap_err()
                .to_string(),
            "ConsensFlow's answer to /api/tasks has no lanes"
        );
        assert_eq!(
            answer.part("open").unwrap_err().to_string(),
            "ConsensFlow's answer to /api/tasks has no open"
        );
    }

    #[test]
    fn a_window_is_a_token_that_says_something_utf_8_or_not() {
        use cf_base::env::Env;
        let window = |token: &str| {
            Board::from_env(&Env::from_vars([
                ("CONSENSFLOW_URL", "http://127.0.0.1:1"),
                ("CONSENSFLOW_TOKEN", token),
            ]))
            .is_some()
        };
        assert!(window("tok"));
        assert!(
            !window(""),
            "an empty token is no window, as Node tested it"
        );
        assert!(Board::from_env(&Env::from_vars([("CONSENSFLOW_URL", "u")])).is_none());
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let token = std::ffi::OsStr::from_bytes(b"t\xffk");
            let env = Env::from_vars([("CONSENSFLOW_TOKEN", token)]);
            assert!(
                Board::from_env(&env).is_some(),
                "a token that is no UTF-8 is still a window's, refused by the board"
            );
        }
    }

    #[test]
    fn no_url_and_no_daemon_are_not_refusals() {
        let none = Board::new(None, "tok").get("/api/inbox").unwrap_err();
        assert_eq!(
            none.to_string(),
            "CONSENSFLOW_URL is not set: run cf from a window ConsensFlow opened"
        );
        assert!(!none.is_refusal());
        // A port nothing listens on: bound, then let go.
        let address = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let url = format!("http://{address}");
        let gone = Board::new(Some(&url), "tok").get("/api/inbox").unwrap_err();
        assert!(!gone.is_refusal());
        assert!(gone
            .to_string()
            .starts_with(&format!("ConsensFlow is not answering at {url} (")));
    }
}
