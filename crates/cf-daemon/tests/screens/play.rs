//! One trace of the screens, played against the daemon's front. Each step is
//! made as the trace says, and each answer is held to Node's: its status, its
//! type and its bytes (key order, absent against null), the roster file before
//! and after each exchange and no other file of the folder changed, the roster's
//! change told as often as it should be, and the ledger left as Node left it.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;

use cf_base::env::Env;
use serde_json::{Map, Value};

use crate::client::{send, Ask, Reply};
use crate::ledger::dump;
use crate::rig::{start, Rig};
use crate::world::{changes, is, File, Root};

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

/// A trace by its name, as the file under `tests/goldens/` holds it.
pub fn load(name: &str) -> Value {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join(format!("{name}.json.gz"));
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(&file).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

/// Plays the trace `name` to its end.
pub async fn play(name: &str) {
    let trace = load(name);
    assert_eq!(trace["format"], 1, "{name}: the format of a trace");
    assert_eq!(trace["surface"], "screens", "{name}");
    let mut game = Game {
        name,
        token: trace["ui"]["token"].as_str().unwrap().to_owned(),
        root: Root::new(),
        ledgers: tempfile::tempdir().unwrap(),
        rig: None,
        closed: false,
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
    game.finish().await;
}

struct Game<'a> {
    name: &'a str,
    token: String,
    root: Root,
    ledgers: tempfile::TempDir,
    rig: Option<Rig>,
    closed: bool,
}

impl Game<'_> {
    fn rig(&self) -> &Rig {
        self.rig.as_ref().expect("a world comes first")
    }

    /// The first world is whole and starts the daemon's front over it; the
    /// later ones hold the files that changed since.
    async fn world(&mut self, at: usize, step: &Value) {
        let files = step.get("files").and_then(Value::as_object);
        self.root.put(files.unwrap_or(&Map::new()));
        let Some(variables) = step.get("env").and_then(Value::as_object) else {
            return;
        };
        assert!(
            self.rig.is_none(),
            "{} step {at}: a variable changes after the daemon started",
            self.name
        );
        let ledger = self.ledgers.path().join("consensflow.db");
        self.rig = Some(start(&self.token, self.environment(variables), &ledger).await);
    }

    /// The variables a world names, the paths as this machine writes them, and on
    /// Windows what finds and starts a `.cmd` there.
    fn environment(&self, variables: &Map<String, Value>) -> Env {
        let process = Env::from_process();
        let windows = ["SystemRoot", "ComSpec", "PATHEXT"]
            .into_iter()
            .filter(|_| cfg!(windows))
            .filter_map(|name| Some((name.to_owned(), process.text(name)?.to_owned())));
        let home = variables
            .get("HOME")
            .and_then(Value::as_str)
            .map(|home| self.root.expand(home, false));
        let given = variables.iter().map(|(name, value)| {
            (
                name.clone(),
                self.root.expand(value.as_str().unwrap(), false),
            )
        });
        Env::from_vars(
            given
                .chain(windows)
                .chain(home.map(|home| ("USERPROFILE".to_owned(), home))),
        )
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

        let (before, told, kicked) = (
            self.root.snapshot(),
            self.rig().told.get(),
            self.rig().kicks.get(),
        );
        let reply = send(
            &self.rig().address(),
            &Ask {
                method,
                target,
                authorization: request["authorization"].as_str(),
                content_type: request["contentType"].as_str(),
                body: request["body"].as_str().map(str::as_bytes),
            },
        )
        .await;
        let after = self.root.snapshot();

        self.holds_the_answer(&label, step, &reply);
        self.holds_the_files(&label, step, &before, &after);
        assert_eq!(
            self.rig().told.get() - told,
            u32::from(tells(method, target, reply.status)),
            "{label}: the roster's change is told once for a write that was made, else never"
        );
        assert_eq!(
            self.rig().kicks.get(),
            kicked,
            "{label}: no screen wakes the dispatcher"
        );
    }

    /// The status, the type and the bytes of the answer.
    fn holds_the_answer(&self, label: &str, step: &Value, reply: &Reply) {
        let response = &step["response"];
        assert_eq!(
            u64::from(reply.status),
            response["status"].as_u64().unwrap(),
            "{label}: the status ({})",
            String::from_utf8_lossy(&reply.body)
        );
        assert_eq!(
            reply.content_type.as_deref(),
            response["contentType"].as_str(),
            "{label}: the type"
        );
        let expected = match (response["page"].as_str(), response["body"].as_str()) {
            (Some(page), _) => self.page(page),
            (None, Some(body)) => self.root.expand(body, true),
            (None, None) => String::new(),
        };
        let actual = String::from_utf8_lossy(&reply.body);
        let id = step["id"].as_u64().unwrap();
        if DEPARTURES.contains(&(self.name, id)) {
            assert!(
                says_it_in_its_own_words(&expected, &reply.body),
                "{label}: Node said {expected}, which the daemon says in its own words, not {actual}"
            );
        } else {
            assert!(
                reply.body == expected.as_bytes(),
                "{label}: the bytes\n   Node: {expected:.300}\n  daemon: {actual:.300}"
            );
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
    fn holds_the_files(
        &self,
        label: &str,
        step: &Value,
        before: &crate::world::Snapshot,
        after: &crate::world::Snapshot,
    ) {
        let recorded = step.get("wrote").and_then(Value::as_object);
        let actual = changes(before, after);
        let (recorded_paths, actual_paths): (BTreeSet<&str>, BTreeSet<&str>) = (
            recorded
                .into_iter()
                .flatten()
                .map(|(path, _)| path.as_str())
                .collect(),
            actual.keys().map(String::as_str).collect(),
        );
        assert_eq!(
            actual_paths, recorded_paths,
            "{label}: the files it changed"
        );
        for (path, (was, now)) in &actual {
            let sides = &recorded.unwrap()[path];
            assert!(
                is(was.as_ref(), &sides["before"], &self.root),
                "{label}: {path} before\n  Node: {}\n  here: {}",
                sides["before"],
                shown(was.as_ref())
            );
            assert!(
                is(now.as_ref(), &sides["after"], &self.root),
                "{label}: {path} after\n  Node: {}\n  here: {}",
                sides["after"],
                shown(now.as_ref())
            );
        }
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
        if let Some(recorded) = ledger.get("final").filter(|recorded| !recorded.is_null()) {
            assert_eq!(
                &dump(&self.rig().ledger_file),
                recorded,
                "{} step {at}: the database the ledger left",
                self.name
            );
        }
    }

    async fn finish(mut self) {
        if !self.closed {
            self.close().await;
        }
    }
}

/// The text of a file as a failure shows it.
fn shown(file: Option<&File>) -> String {
    file.map_or_else(|| "none".to_owned(), |file| format!("{:?}", file.text))
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
