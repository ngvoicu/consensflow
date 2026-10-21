//! What the evals (`crates/cf-evals`) and the live tests of `crates/cf-e2e` ask of
//! the harness code. They start the real harnesses themselves, in windows of the
//! pane host, and need what the daemon needs to open one the way the app does
//! and to read what comes of it, so they ask through the test binary
//! `harness-ask` (`live::ask` of `crates/cf-e2e` runs it), which has no logic of
//! its own: this module is the answers, one function per question.
//!
//! A question is a JSON object with an `op`, and its answer one JSON value:
//!
//! | `op` | the question | the answer |
//! |---|---|---|
//! | `records` | `kind`, `session`, `env`: what the harness's own record of the conversation says | `{reading, settled}`: the reading as [`records::answers`] writes it, and whether the window's turn is over, as the dispatcher reads it |
//! | `start` | `agent` (`kind`, `model`, `effort`, `thinking`), `session`, `seed`: the harness's own window on a new conversation, or with `resume` true on the one `session` names | `{command, args, prompt?, env, dropEnv}`, or null for a kind with no window of its own |
//! | `window_text` | `text`: as a window can take it | the text |
//! | `console_text` | `text`: as Windows' console carries it to a window | the text |
//! | `claude_settings` | `env`, `launch`, `boardQuestions`: the settings file of a Claude window | `["--settings", file]`, the file written |
//! | `interrupt` | `kind`: the keys that interrupt a turn in the harness's window | `{presses, closeAfterMs}`: Escape that many times in a row and, where `closeAfterMs` is not null, once more that long after |
//! | `input` | `kind`: how the harness's windows take what they are handed | `{pasted, clear}`: whether it is pasted into the window's input box, and the keys that empty the box (null for a harness whose windows take it over a channel) |
//! | `program` | `argv`, `env`: a program and its arguments to open a window on (the program's absolute path first) | the argv the pane host is asked to open: an npm shim (a `.cmd` on Windows) as the node and the script it runs, anything else as it is; an error for a script of another shape, or a shim that has no node |
//! | `newest_session` | `kind`, `workspace`, `since`, `env`: the newest top-level conversation a harness of that kind (OpenCode, Devin or Codex) opened in `workspace` at or after `since` (milliseconds), from its own store | the session's id, or null while there is none |
//! | `specmint` | `kind`, `variant`, `env`: where the harness has the Specmint variant installed for its user (the adapter's own look, in the places the harness reads), with a home that is the one `env` names | `{installed, skill}`: whether the harness has it, and the file of its `SKILL.md` (where ConsensFlow's own copy would be written, for a launch of a made-up id, where it has none) |
//!
//! The last three are the evals' and the live tests' own: the program a window
//! opens on, which the daemon asks `cf_process::pane_argv` for, the
//! conversation a window of a harness that names its own opened, which only
//! its store says, and the Specmint skill a chief's window is given, which the
//! live test of specs names the copy it was given by.

mod session;

use std::path::Path;

use cf_base::env::Env;
use cf_base::text::{console_text, window_text};
use cf_base::time::{Clock, SystemClock};
use cf_proto::agents::Harness;
use cf_proto::specs::Variant;
use serde_json::{json, Value};

use crate::claude::install;
use crate::contract::{Agent, Input, Interrupt, LaunchId, Skill};
use crate::launch::adapter;
use crate::records::{self, Options};
use crate::shared::record_state::record_state;
use crate::shared::window_args;
use crate::testing::Fakes;

/// The answer to `question`, or why it cannot be answered.
pub fn answer(question: &Value) -> Result<Value, String> {
    match text(question, "op")? {
        "records" => read_record(question),
        "start" => start(question),
        "window_text" => Ok(json!(window_text(text(question, "text")?))),
        "console_text" => Ok(json!(console_text(text(question, "text")?))),
        "claude_settings" => claude_settings(question),
        "interrupt" => interrupt(question),
        "input" => input(question),
        "program" => program(question),
        "newest_session" => newest_session(question),
        "specmint" => specmint(question),
        other => Err(format!(
            "the question asks for {other}, which is none of records, start, window_text, console_text, claude_settings, interrupt, input, program, newest_session and specmint"
        )),
    }
}

/// A text field of `object`: how a question is read.
fn text<'a>(object: &'a Value, name: &str) -> Result<&'a str, String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("the question names no {name}"))
}

/// A text field that may be none: absent, null or not text.
fn optional<'a>(object: &'a Value, name: &str) -> Option<&'a str> {
    object.get(name).and_then(Value::as_str)
}

/// The harness a kind (`claude-code`) names.
fn harness(kind: &str) -> Result<Harness, String> {
    Harness::from_kind(kind).ok_or_else(|| format!("no harness runs as {kind}"))
}

/// The environment the question carries, in `env`: its variables that are
/// text (a variable given null is one the caller removed).
fn env(question: &Value) -> Result<Env, String> {
    let variables = question
        .get("env")
        .and_then(Value::as_object)
        .ok_or("the question names no env")?;
    Ok(Env::from_vars(variables.iter().filter_map(
        |(name, value)| value.as_str().map(|value| (name.as_str(), value)),
    )))
}

/// What the harness's own record of a conversation says, read whole once.
fn read_record(question: &Value) -> Result<Value, String> {
    let reading = records::answers(
        harness(text(question, "kind")?)?,
        text(question, "session")?,
        &env(question)?,
        &Options::default(),
        &records::machine_zone(),
        SystemClock.now_ms(),
    );
    let settled = record_state(reading.clone()).settled;
    Ok(json!({ "reading": &*reading, "settled": settled }))
}

/// The harness's own window on a new conversation, or, when the question says
/// `resume`, on the one `session` names, which the harness already keeps.
fn start(question: &Value) -> Result<Value, String> {
    let agent = question.get("agent").ok_or("the question names no agent")?;
    let fields = Agent {
        model: optional(agent, "model"),
        effort: optional(agent, "effort"),
        thinking: optional(agent, "thinking"),
        designer: false,
    };
    let resumed = question.get("resume").and_then(Value::as_bool) == Some(true);
    let invocation = harness(text(agent, "kind")?).ok().and_then(|harness| {
        let open = if resumed {
            window_args::resume
        } else {
            window_args::start
        };
        open(
            harness,
            fields,
            optional(question, "session"),
            optional(question, "seed"),
        )
    });
    Ok(invocation.map_or(Value::Null, |invocation| invocation.written()))
}

/// The settings file of a Claude window, written under the home `env` names.
fn claude_settings(question: &Value) -> Result<Value, String> {
    let launch = LaunchId::new(text(question, "launch")?)
        .ok_or("the question's launch is not a launch id (a lowercase uuid)")?;
    let board_questions = question
        .get("boardQuestions")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    Ok(json!(install::settings(
        &env(question)?,
        &launch,
        board_questions
    )?))
}

/// The argv a window opens on, for the program and arguments the question
/// gives, as the daemon opens a harness's window (`cf_process::pane_argv`).
fn program(question: &Value) -> Result<Value, String> {
    let words = question
        .get("argv")
        .and_then(Value::as_array)
        .ok_or("the question names no argv")?;
    let argv: Vec<String> = words
        .iter()
        .map(|word| {
            word.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("a word of the argv is not text: {word}"))
        })
        .collect::<Result<_, _>>()?;
    Ok(json!(cf_process::pane_argv(&argv, &env(question)?)?))
}

/// The newest conversation of a harness that names its own in its store, in a
/// folder, since a time.
fn newest_session(question: &Value) -> Result<Value, String> {
    let kind = text(question, "kind")?;
    let since = question
        .get("since")
        .and_then(Value::as_i64)
        .ok_or("the question names no since, in milliseconds")?;
    let found = session::newest(
        harness(kind)?,
        kind,
        text(question, "workspace")?,
        since,
        &env(question)?,
    )?;
    Ok(found.map_or(Value::Null, Value::from))
}

/// Where the harness has the Specmint variant installed for its user: the
/// adapter's own answer (`Adapter::specmint`) with the home `env` names, which
/// is the one the daemon would ask it in. Where it has none, the answer is where
/// ConsensFlow's own copy is kept for it, which is the same for every launch.
fn specmint(question: &Value) -> Result<Value, String> {
    let variant = Variant::from_name(text(question, "variant")?)
        .ok_or("the question's variant is none of core, core-html, tdd and tdd-html")?;
    let env = env(question)?;
    let services = Fakes::new(&env).services(&env, Path::new(""));
    let skill = adapter(harness(text(question, "kind")?)?, &services).specmint(variant)?;
    Ok(json!({
        "installed": matches!(skill, Skill::Global(_)),
        "skill": skill.path(),
    }))
}

/// The keys that interrupt a turn in the harness's window, as its adapter
/// says them: what the adapter is built with does not matter to them.
fn interrupt(question: &Value) -> Result<Value, String> {
    let env = Env::from_vars::<&str, &str>([]);
    let services = Fakes::new(&env).services(&env, Path::new(""));
    let Interrupt {
        presses,
        close_after,
    } = adapter(harness(text(question, "kind")?)?, &services).interrupt();
    Ok(json!({
        "presses": presses,
        "closeAfterMs": close_after.and_then(|after| u64::try_from(after.as_millis()).ok()),
    }))
}

/// How the harness's windows take what they are handed, as its adapter says it.
fn input(question: &Value) -> Result<Value, String> {
    let env = Env::from_vars::<&str, &str>([]);
    let services = Fakes::new(&env).services(&env, Path::new(""));
    Ok(
        match adapter(harness(text(question, "kind")?)?, &services).input() {
            Input::Channel => json!({ "pasted": false, "clear": null }),
            Input::Pasted { clear } => json!({ "pasted": true, "clear": clear }),
        },
    )
}

#[cfg(test)]
mod tests;
