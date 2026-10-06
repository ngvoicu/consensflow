//! One trace of the screens, played against the daemon's front. Each step is
//! made as the trace says, and each answer is held to Node's: its status, its
//! type and its bytes (key order, absent against null), the roster file before
//! and after each exchange and no other file of the folder changed, the roster's
//! change told as often as it should be, and the ledger left as Node left it: a
//! trace whose ledger left a database is played to its `close`, where the
//! database is compared.

use serde_json::Value;

use crate::front::{address, send, Reply, Request};
use crate::rig::{start, Rig};
use crate::support::compare::{differs, left};
use crate::support::trace::{self, Tally};
use crate::world::World;
use crate::wrote::{held, snapshot, Snapshot};

/// The pages as Node's generator wrote them, `$TOKEN` and `$VERSION` for what goes in.
const AGENTS_PAGE: &str = include_str!("../goldens/pages/agents.html");
const HARNESSES_PAGE: &str = include_str!("../goldens/pages/harnesses.html");

/// The exchanges whose answer is not Node's to the byte, and why: the request's
/// body is no JSON, which Node said in V8's words (`Expected property name or
/// '}' in JSON at position 1 (line 1 column 2)`) and the daemon says in its own
/// (`the request body is not valid JSON: ` and `serde_json`'s, which say where):
/// by trace, the exchange's `id`. Every other answer is held to its bytes.
pub const DEPARTURES: [(&str, u64); 12] = [
    ("corners-screens-002", 1),
    ("corners-screens-002", 2),
    ("corners-screens-002", 3),
    ("corners-screens-003", 1),
    ("corners-screens-003", 2),
    ("corners-screens-003", 3),
    ("corners-screens-003", 4),
    ("corners-screens-003", 5),
    ("corners-screens-003", 6),
    ("corners-screens-003", 7),
    ("corners-screens-003", 8),
    ("corners-screens-009", 12),
];

/// The words the daemon says where Node said V8's of a body that is no JSON.
const OWN_WORDS: &str = "the request body is not valid JSON: ";

/// Whether `words` are V8's of a text `JSON.parse` would not read.
pub fn v8_json_words(words: &str) -> bool {
    [
        "Expected property name or '}' in JSON at position ",
        "Expected ':' after property name in JSON at position ",
        "Unexpected non-whitespace character after JSON at position ",
        "Unexpected end of JSON input",
    ]
    .iter()
    .any(|start| words.starts_with(start))
        || (words.starts_with("Unexpected token ") && words.ends_with(" is not valid JSON"))
}

/// Plays the trace `name` to its end: what it held of it.
pub async fn play(name: &str) -> Tally {
    let trace = trace::load(name);
    assert_eq!(trace["format"], 1, "{name}: the format of a trace");
    assert_eq!(trace["surface"], "screens", "{name}");
    let mut game = Game {
        name,
        token: trace["ui"]["token"].as_str().unwrap().to_owned(),
        world: World::new(),
        ledgers: tempfile::tempdir().unwrap(),
        rig: None,
        closed: false,
        ledger_closed: false,
        tally: Tally::default(),
    };
    for (at, step) in trace["steps"].as_array().unwrap().iter().enumerate() {
        match step["kind"].as_str().unwrap() {
            "world" => game.world(at, step).await,
            "exchange" => game.exchange(at, step).await,
            "api.close" => game.close().await,
            "ledger" => game.ledger(at, step, &trace["ledger"]),
            other => panic!("{name} step {at}: no screens trace has a {other} step"),
        }
    }
    game.finish(&trace["ledger"]).await
}

struct Game<'a> {
    name: &'a str,
    token: String,
    world: World,
    ledgers: tempfile::TempDir,
    rig: Option<Rig>,
    closed: bool,
    /// Whether the trace's `close` of its ledger was played.
    ledger_closed: bool,
    /// What has been compared so far.
    tally: Tally,
}

impl Game<'_> {
    fn rig(&self) -> &Rig {
        self.rig.as_ref().expect("a world comes first")
    }

    /// The first world is whole and starts the daemon's front over it; the
    /// later ones hold the files that changed since.
    async fn world(&mut self, at: usize, step: &Value) {
        self.world.put(step);
        if step.get("env").and_then(Value::as_object).is_none() {
            return;
        }
        assert!(
            self.rig.is_none(),
            "{} step {at}: a variable changes after the daemon started",
            self.name
        );
        let ledger = self.ledgers.path().join("consensflow.db");
        self.rig = Some(start(&self.token, self.world.env(), &ledger).await);
    }

    async fn exchange(&mut self, at: usize, step: &Value) {
        let request = &step["request"];
        let (method, target) = (
            request["method"].as_str().unwrap(),
            request["target"].as_str().unwrap(),
        );
        let label = format!("{} step {at}: {method} {target}", self.name);
        for what in ["clock", "names", "events", "seams"] {
            assert!(
                step[what].as_array().is_some_and(Vec::is_empty),
                "{label}: the screens read no ledger, so there is no {what} to hold"
            );
        }
        assert_eq!(
            step["kicks"], 0,
            "{label}: the API is not woken by a screen"
        );
        assert!(
            request.get("bodyBase64").is_none(),
            "{label}: a body that is no UTF-8"
        );
        assert!(
            !request["authorization"]
                .as_str()
                .is_some_and(|header| header.contains('«')),
            "{label}: a token of a window is none of a screen's"
        );

        // What was told and woken before this exchange is no part of it.
        self.rig().take_told();
        self.rig().take_kicks();
        let before = snapshot(&self.world);
        let reply = send(
            &address(&self.rig().api),
            Request {
                method,
                target,
                authorization: request["authorization"].as_str(),
                content_type: request["contentType"].as_str(),
                body: request["body"].as_str().map(str::as_bytes),
            },
        )
        .await
        .unwrap_or_else(|why| panic!("{label}: no answer: {why}"));
        let after = snapshot(&self.world);

        self.holds_the_answer(&label, step, &reply);
        self.holds_the_files(&label, step, &before, &after);
        assert_eq!(
            self.rig().take_told(),
            usize::from(tells(method, target, reply.status)),
            "{label}: the roster's change is told once for a write that was made, else never"
        );
        assert_eq!(
            self.rig().take_kicks(),
            0,
            "{label}: no screen wakes the dispatcher"
        );
        self.tally.exchanges += 1;
    }

    /// The status, the type and the bytes of the answer.
    fn holds_the_answer(&self, label: &str, step: &Value, reply: &Reply) {
        let response = &step["response"];
        if let Some(why) = reply.head_differs(response) {
            panic!("{label}: {why}");
        }
        let expected = match (response["page"].as_str(), response["body"].as_str()) {
            (Some(page), _) => self.page(page),
            (None, Some(body)) => self.world.expand(body, true),
            (None, None) => String::new(),
        };
        let id = step["id"].as_u64().unwrap();
        if DEPARTURES.contains(&(self.name, id)) {
            assert!(
                says_it_in_its_own_words(&expected, &reply.body),
                "{label}: Node said {expected}, which the daemon says in its own words, not {}",
                String::from_utf8_lossy(&reply.body)
            );
        } else if let Some(why) = differs("the answer", &reply.body, &expected) {
            panic!("{label}: {why}");
        }
    }

    /// A page as Node served it for the UI token: the recorded one with the token
    /// and the version of this build put in.
    fn page(&self, name: &str) -> String {
        let page = match name {
            "agents" => AGENTS_PAGE,
            "harnesses" => HARNESSES_PAGE,
            other => panic!("{}: a page named {other}", self.name),
        };
        page.replace("$TOKEN", &self.token)
            .replace("$VERSION", cf_daemon::start::VERSION)
    }

    /// The files the exchange changed are the ones the trace says, each as it was
    /// and as it became.
    fn holds_the_files(&self, label: &str, step: &Value, before: &Snapshot, after: &Snapshot) {
        let problems = held(step, before, after);
        assert!(problems.is_empty(), "{label}: {}", problems.join("\n"));
    }

    async fn close(&mut self) {
        self.rig().api.close().await;
        self.closed = true;
    }

    /// The only ledger step of a screens trace: the ledger is closed, and the
    /// database it left is the one Node's left.
    fn ledger(&mut self, at: usize, step: &Value, ledger: &Value) {
        assert_eq!(step["method"], "close", "{} step {at}", self.name);
        self.rig().ledger.borrow_mut().close_in_place().unwrap();
        self.ledger_closed = true;
        if let Some(recorded) = ledger.get("final").filter(|recorded| !recorded.is_null()) {
            if let Some(why) = left(&self.rig().ledger_file, recorded) {
                panic!("{} step {at}: {why}", self.name);
            }
            self.tally.databases += 1;
        }
    }

    /// The API closed, if no step did, and what the trace held. A trace whose
    /// ledger left a database was played to the `close` that compares it: one
    /// that stopped short held nothing of the ledger.
    async fn finish(mut self, ledger: &Value) -> Tally {
        let left_one = ledger
            .get("final")
            .is_some_and(|database| !database.is_null());
        assert!(
            self.ledger_closed || !left_one,
            "{}: the trace never reached the close of its ledger, whose database Node left",
            self.name
        );
        if !self.closed {
            self.close().await;
        }
        self.tally.traces += 1;
        self.tally
    }
}

/// Whether the daemon answered as it says a body that is no JSON: a 400 whose one
/// key is `error`, in `OWN_WORDS`, where `recorded` is Node's, in V8's.
fn says_it_in_its_own_words(recorded: &str, body: &[u8]) -> bool {
    let (Ok(Value::Object(node)), Ok(Value::Object(daemon))) = (
        serde_json::from_str::<Value>(recorded),
        serde_json::from_slice::<Value>(body),
    ) else {
        return false;
    };
    node.len() == 1
        && daemon.len() == 1
        && node["error"].as_str().is_some_and(v8_json_words)
        && daemon["error"]
            .as_str()
            .is_some_and(|words| words.starts_with(OWN_WORDS))
}

/// Whether an exchange is a write to the roster that was made: the four routes
/// that change it, answered with a success.
fn tells(method: &str, target: &str, status: u16) -> bool {
    let path = target.split('?').next().unwrap_or(target);
    let writes = match method {
        "POST" => path == "/api/agents" || path == "/api/preferences",
        "PATCH" | "DELETE" => path
            .strip_prefix("/api/agents/")
            .is_some_and(|name| !name.is_empty() && !name.contains('/')),
        _ => false,
    };
    writes && (200..300).contains(&status)
}
