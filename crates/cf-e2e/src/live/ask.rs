//! The harness code of the product, asked what a live test needs of it. A live
//! test starts the real harnesses itself, in windows of the pane host, and
//! needs what the daemon needs to open one the way the app does and to read what
//! comes of it: this crate depends on no crate of the product, so it asks the
//! test binary `harness-ask` (`cf_harness::tooling` says what each question
//! asks and answers), a program of its own, built the first time one is asked
//! ([`crate::cf::harness_ask`]). What it needs of the engine's texts (the note a
//! window the human's Resume brings back is given) it asks `engine-ask`
//! (`cf_engine::testing::ask`) the same way ([`crate::cf::engine_ask`]).
//!
//! A question is one JSON object on its input; its answer is the last line of
//! its output, `{"answered": value}`, or `{"threw": message}` for a question it
//! could not answer, which is an error here that says why.

mod launch;
mod readiness;

use std::path::Path;
use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::process::Run;
use crate::{cf, Error, Result};

pub use launch::{devin_hooks, devin_launch, devin_ready, DevinLaunch, DevinReady};
pub use readiness::{devin_readiness, DevinReadiness};

/// Builds the test binary if it is not built yet, which the first question would
/// do, minutes into a run: a live test asks this first, before it spends anything,
/// so that a build that cannot be made fails the run there and not in the middle of
/// a window's turn.
pub fn prepare() -> Result {
    cf::harness_ask()?;
    cf::engine_ask().map(|_| ())
}

/// One question to the test binary, and what it answered.
pub fn question(body: &Value) -> Result<Value> {
    ask(cf::harness_ask()?, "harness-ask", body)
}

/// One question to `engine-ask`, and what it answered.
pub fn engine_question(body: &Value) -> Result<Value> {
    ask(cf::engine_ask()?, "engine-ask", body)
}

/// One question to the test binary `program`, called `name`, and what it answered.
fn ask(program: &Path, name: &str, body: &Value) -> Result<Value> {
    let ran = Run::new(program)
        .input(format!("{body}\n"))
        .inheriting_env()
        .run()?;
    answered(name, ran.code, &ran.stdout)
}

/// What a run of the test binary `name` answered: the last line it printed, which
/// is `{"answered": value}` or `{"threw": message}`.
fn answered(name: &str, code: Option<i32>, stdout: &str) -> Result<Value> {
    let last: Value = stdout
        .trim_end()
        .lines()
        .next_back()
        .and_then(|line| serde_json::from_str(line).ok())
        .ok_or_else(|| {
            Error::Daemon(format!(
                "{name} ended with {} and said {stdout:?}",
                code.map_or_else(|| "a signal".to_owned(), |code| code.to_string())
            ))
        })?;
    match (last.get("threw"), last.get("answered")) {
        (Some(threw), _) => Err(Error::Daemon(format!(
            "{name} could not answer: {}",
            super::text(threw)
        ))),
        (None, Some(answered)) => Ok(answered.clone()),
        (None, None) => Err(Error::Daemon(format!(
            "{name}'s last line is neither an answer nor a failure: {last}"
        ))),
    }
}

/// The note the engine gives a window the human's Resume brings back when its turn
/// was cut, as the window is given it, the message `message`: a member's about its
/// `tasks`, the chief's where there are none.
pub fn cut_note(message: i64, tasks: &[i64]) -> Result<String> {
    let answer = engine_question(&json!({ "op": "cut_note", "message": message, "tasks": tasks }))?;
    answer
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::Daemon(format!("engine-ask's note is no text: {answer}")))
}

/// `env` as the object a question carries it as: each variable by its name,
/// the last of a name that is given twice.
pub fn env_object(env: &[(String, String)]) -> Value {
    Value::Object(
        env.iter()
            .map(|(name, value)| (name.clone(), json!(value)))
            .collect::<Map<String, Value>>(),
    )
}

/// One item of a harness's own record of a conversation: what the daemon reads
/// of a window by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    /// `user` or `assistant`.
    pub role: String,
    pub text: String,
    /// Whether the record says the item ended a turn.
    pub complete: bool,
}

impl Item {
    fn of(item: &Value) -> Self {
        Self {
            id: super::text(&item["id"]),
            role: item["role"].as_str().unwrap_or_default().to_owned(),
            text: item["text"].as_str().unwrap_or_default().to_owned(),
            complete: item["complete"] == true,
        }
    }
}

/// What the harness's own record of a conversation says, read whole as the
/// daemon reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub items: Vec<Item>,
    /// Whether the window's turn is over, as the daemon's dispatcher reads it.
    pub settled: bool,
    /// The record's word on the turn: `in-flight` or `settled`.
    pub settlement: String,
    /// What the record says of a turn that failed, and of a quota that ran out.
    pub failed: Value,
    pub quota: Value,
}

/// What the record of the conversation `session` of a harness of the kind
/// `kind` (`claude-code`, `devin`…) says, read in the places `env` names. None
/// where it says nothing: no record of the conversation, or one that cannot be
/// read.
pub fn records(kind: &str, session: &str, env: &[(String, String)]) -> Result<Option<Record>> {
    let answer = question(&json!({
        "op": "records", "kind": kind, "session": session, "env": env_object(env),
    }))?;
    Ok(record_of(&answer))
}

/// The record an answer to `records` holds.
fn record_of(answer: &Value) -> Option<Record> {
    let reading = &answer["reading"];
    if reading["unknown"] == true {
        return None;
    }
    Some(Record {
        items: reading["items"]
            .as_array()
            .map(|items| items.iter().map(Item::of).collect())
            .unwrap_or_default(),
        settled: answer["settled"] == true,
        settlement: reading["settlement"]["state"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        failed: reading["failed"].clone(),
        quota: reading["quota"].clone(),
    })
}

/// The harness's own window on a new conversation, as the app opens it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    /// The command's name (`claude`): find the program with
    /// [`super::machine::command`], or take the installed harness's. A launch the
    /// adapter prepared ([`devin_launch`]) names the program it found, whole.
    pub command: String,
    pub args: Vec<String>,
    /// The variables it is started with besides the window's, and the ones
    /// taken away from them.
    pub env: Vec<(String, String)>,
    pub drop_env: Vec<String>,
    /// The first message, for the harness that takes it in another way than as
    /// an argument (Devin).
    pub prompt: Option<String>,
}

/// The window of `agent` (`kind`, `model`, `effort`, `thinking`) on a new
/// conversation: `session` is the conversation's id where the harness takes one
/// (Claude, Pi), `seed` its first message where the window takes one. None for a
/// harness with no window of its own, and for one that needs an id when none is
/// given.
pub fn start(agent: &Value, session: Option<&str>, seed: Option<&str>) -> Result<Option<Start>> {
    let answer = question(&json!({
        "op": "start", "agent": agent, "session": session, "seed": seed,
    }))?;
    Ok(start_of(&answer))
}

/// The window of `agent` on the conversation `session`, which the harness
/// already keeps (`resume`, as the daemon opens a window on a conversation a quit
/// cut or a reopened task goes on in). None for a harness with no window of its
/// own.
pub fn resume(agent: &Value, session: &str) -> Result<Option<Start>> {
    let answer = question(&json!({
        "op": "start", "agent": agent, "session": session, "resume": true,
    }))?;
    Ok(start_of(&answer))
}

/// The window an answer to `start` holds.
fn start_of(answer: &Value) -> Option<Start> {
    let words = |list: &Value| -> Vec<String> {
        list.as_array()
            .map(|words| words.iter().map(super::text).collect())
            .unwrap_or_default()
    };
    Some(Start {
        command: answer["command"].as_str()?.to_owned(),
        args: words(&answer["args"]),
        env: answer["env"]
            .as_object()
            .map(|vars| {
                vars.iter()
                    .map(|(name, value)| (name.clone(), super::text(value)))
                    .collect()
            })
            .unwrap_or_default(),
        drop_env: words(&answer["dropEnv"]),
        prompt: answer["prompt"].as_str().map(str::to_owned),
    })
}

/// `text` as a window can take it: a carriage return before a newline goes, and
/// a control character is shown as its picture.
pub fn window_text(text: &str) -> Result<String> {
    Ok(super::text(&question(
        &json!({ "op": "window_text", "text": text }),
    )?))
}

/// `text` as Windows' console carries it to a window that reads key presses:
/// what the console would drop (a dash, an arrow, a currency sign) is spelled in
/// ASCII.
pub fn console_text(text: &str) -> Result<String> {
    Ok(super::text(&question(
        &json!({ "op": "console_text", "text": text }),
    )?))
}

/// The keys that interrupt a turn in a harness's window, as its adapter says
/// them to the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interrupt {
    /// Escape this many times in a row…
    pub presses: u32,
    /// …and, where this is not none, once more after that long: it closes the
    /// dialog the presses opened at a turn that had just ended (Devin's rewind).
    pub close_after: Option<Duration>,
}

/// The keys that interrupt a turn in the window of a harness of the kind `kind`.
pub fn interrupt(kind: &str) -> Result<Interrupt> {
    let answer = question(&json!({ "op": "interrupt", "kind": kind }))?;
    Ok(Interrupt {
        presses: answer["presses"]
            .as_u64()
            .and_then(|presses| u32::try_from(presses).ok())
            .unwrap_or(1),
        close_after: answer["closeAfterMs"].as_u64().map(Duration::from_millis),
    })
}

/// The settings file of a Claude window's launch (`launch`, a lowercase uuid),
/// written in the ConsensFlow folder `env` names, and the arguments that load it
/// (`--settings <file>`). `board_questions` has a member's question tool
/// answered from the board.
pub fn claude_settings(
    env: &[(String, String)],
    launch: &str,
    board_questions: bool,
) -> Result<Vec<String>> {
    let answer = question(&json!({
        "op": "claude_settings", "env": env_object(env), "launch": launch,
        "boardQuestions": board_questions,
    }))?;
    Ok(answer
        .as_array()
        .map(|words| words.iter().map(super::text).collect())
        .unwrap_or_default())
}

/// The command line a window opens on, for the program and arguments `argv` (the
/// program's absolute path first): the pane host starts a file with each argument
/// quoted the way programs read them, which `cmd.exe` does not, so an
/// npm-installed harness on Windows (a `.cmd` shim) opens as the shim's own node
/// and script; anywhere else it is `argv`. A script of any other shape cannot
/// open a window, and is the error that says so. `env` is the window's (the Node
/// a shim runs on is the one on its `PATH`).
pub fn program(argv: &[String], env: &[(String, String)]) -> Result<Vec<String>> {
    let answer = question(&json!({
        "op": "program", "argv": argv, "env": env_object(env),
    }))?;
    Ok(answer
        .as_array()
        .map(|words| words.iter().map(super::text).collect())
        .unwrap_or_default())
}

/// The newest top-level conversation a harness of the kind `kind` (`opencode`,
/// `devin`, `codex`: the ones that name theirs only in their own store) opened in
/// `workspace` at or after `since_ms` (milliseconds since the epoch), from its
/// store in the places `env` names. None while there is none: the folder is the
/// run's own, and a window of a run with no staff is its chief's, so the newest
/// one there is the window's.
pub fn newest_session(
    kind: &str,
    workspace: &Path,
    since_ms: i64,
    env: &[(String, String)],
) -> Result<Option<String>> {
    let answer = question(&json!({
        "op": "newest_session", "kind": kind, "workspace": workspace,
        "since": since_ms, "env": env_object(env),
    }))?;
    Ok(answer.as_str().map(str::to_owned))
}

/// Where a harness has a Specmint variant installed for its user, as its adapter
/// finds it (`Adapter::specmint`): the file of its `SKILL.md`, and whether the
/// harness has it there, or has none and ConsensFlow would write its own where
/// `skill` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecmintSkill {
    pub installed: bool,
    pub skill: String,
}

/// Where the harness of the kind `kind` has the Specmint variant `variant`
/// (`core`, `core-html`, `tdd`, `tdd-html`) installed for its user, in the places
/// `env` names (the machine's own, for a run of real harnesses).
pub fn specmint(kind: &str, variant: &str, env: &[(String, String)]) -> Result<SpecmintSkill> {
    let answer = question(&json!({
        "op": "specmint", "kind": kind, "variant": variant, "env": env_object(env),
    }))?;
    Ok(specmint_of(&answer))
}

/// The skill an answer to `specmint` holds.
fn specmint_of(answer: &Value) -> SpecmintSkill {
    SpecmintSkill {
        installed: answer["installed"] == true,
        skill: answer["skill"].as_str().unwrap_or_default().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_specmint_answer_says_whether_the_harness_has_the_variant_and_where_its_skill_is() {
        let have = specmint_of(
            &json!({ "installed": true, "skill": "/h/.agents/skills/specmint-tdd/SKILL.md" }),
        );
        assert!(have.installed && have.skill.ends_with("specmint-tdd/SKILL.md"));
        let none =
            specmint_of(&json!({ "installed": false, "skill": "/cf/role/specmint-core/SKILL.md" }));
        assert!(!none.installed);
        // An answer that says nothing is a harness that has none.
        assert_eq!(
            specmint_of(&Value::Null),
            SpecmintSkill {
                installed: false,
                skill: String::new()
            }
        );
    }

    #[test]
    fn the_answer_is_the_last_line_the_binary_printed() {
        let said = "noise\n{\"answered\":{\"a\":1}}\n";
        assert_eq!(
            answered("harness-ask", Some(0), said).unwrap(),
            json!({ "a": 1 })
        );
        // Null is an answer too.
        assert_eq!(
            answered("harness-ask", Some(0), "{\"answered\":null}\n").unwrap(),
            Value::Null
        );
    }

    #[test]
    fn a_question_the_binary_could_not_answer_is_an_error_that_says_why() {
        let failed = answered(
            "harness-ask",
            Some(1),
            "{\"threw\":\"no harness runs as claude\"}\n",
        )
        .unwrap_err();
        assert_eq!(
            failed.to_string(),
            "harness-ask could not answer: no harness runs as claude"
        );
        // Each binary is named by its own name.
        let failed = answered("engine-ask", Some(1), "{\"threw\":\"no op\"}\n").unwrap_err();
        assert_eq!(failed.to_string(), "engine-ask could not answer: no op");
    }

    #[test]
    fn a_binary_that_said_no_answer_is_an_error_with_what_it_did_say() {
        for (code, said) in [
            (Some(101), "thread panicked\n"),
            (None, ""),
            (Some(0), "{}\n"),
        ] {
            let failed = answered("harness-ask", code, said).unwrap_err().to_string();
            assert!(failed.starts_with("harness-ask"), "{failed}");
        }
        let failed = answered("harness-ask", Some(101), "boom\n")
            .unwrap_err()
            .to_string();
        assert_eq!(failed, "harness-ask ended with 101 and said \"boom\\n\"");
        let failed = answered("harness-ask", None, "").unwrap_err().to_string();
        assert!(failed.contains("a signal"), "{failed}");
    }

    #[test]
    fn variables_go_by_name_and_the_last_of_a_name_is_the_one() {
        let env = [
            ("HOME".to_owned(), "/h".to_owned()),
            ("A".to_owned(), "1".to_owned()),
            ("HOME".to_owned(), "/other".to_owned()),
        ];
        assert_eq!(env_object(&env), json!({ "HOME": "/other", "A": "1" }));
        assert_eq!(env_object(&[]), json!({}));
    }

    #[test]
    fn a_record_is_its_items_whether_the_turn_is_settled_and_what_it_says_of_the_turn() {
        let answer = json!({
            "settled": true,
            "reading": {
                "items": [
                    { "id": "u1", "role": "user", "text": "Hello" },
                    { "id": "m1", "role": "assistant", "text": "Hi", "complete": true },
                ],
                "settlement": { "state": "settled" },
                "failed": null,
                "quota": null,
            },
        });
        let record = record_of(&answer).unwrap();
        assert_eq!(
            record.items,
            [
                Item {
                    id: "u1".into(),
                    role: "user".into(),
                    text: "Hello".into(),
                    complete: false
                },
                Item {
                    id: "m1".into(),
                    role: "assistant".into(),
                    text: "Hi".into(),
                    complete: true
                },
            ]
        );
        assert!(record.settled);
        assert_eq!(record.settlement, "settled");
        // A conversation the harness kept no record of is none.
        let unknown =
            json!({ "settled": true, "reading": { "unknown": true, "reason": "no file" } });
        assert_eq!(record_of(&unknown), None);
        // A turn that is still going is not settled.
        let going = json!({ "settled": false, "reading": { "items": [], "settlement": { "state": "in-flight" } } });
        let going = record_of(&going).unwrap();
        assert!(!going.settled && going.items.is_empty());
        assert_eq!(going.settlement, "in-flight");
    }

    #[test]
    fn a_window_is_a_command_its_arguments_its_variables_and_a_first_message_if_it_takes_one() {
        let answer = json!({
            "command": "devin",
            "args": ["--model", "swe-2-medium"],
            "prompt": "Go on",
            "env": { "A": "1" },
            "dropEnv": ["ANTHROPIC_API_KEY"],
        });
        assert_eq!(
            start_of(&answer),
            Some(Start {
                command: "devin".into(),
                args: vec!["--model".into(), "swe-2-medium".into()],
                env: vec![("A".into(), "1".into())],
                drop_env: vec!["ANTHROPIC_API_KEY".into()],
                prompt: Some("Go on".into()),
            })
        );
        // A harness with no window of its own, and one that needed an id, have none.
        assert_eq!(start_of(&Value::Null), None);
        let bare = start_of(&json!({ "command": "codex" })).unwrap();
        assert!(bare.args.is_empty() && bare.env.is_empty() && bare.prompt.is_none());
    }
}
