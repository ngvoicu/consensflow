//! Live: what does Codex's app-server say when a turn is interrupted while its
//! question (`item/tool/requestUserInput`) waits for an answer? It is opt-in
//! (`cargo xtask live codex-interrupt`, `npm run live:codex-interrupt`): it spends
//! the machine's quota on a Codex turn, and needs Codex installed and logged in, so
//! it is ignored unless asked for and is no part of `check` or of CI.
//!
//! The broker holds a member's question until the board answers it, and drops an
//! answer whose turn has already ended: it learns that from `turn/completed` or an
//! idle `thread/status/changed` for the question's thread (the receipt and stop
//! design, §2.7). Without either, an answer given after the human's own interrupt
//! would be taken and lost. This starts the installed Codex's app-server on a cheap
//! model, with the question tool on as a member's window has it, asks for a
//! question, interrupts the turn once it arrives, and says what Codex sent in the
//! next 20 seconds: the turn's end and its status, the thread's status, and whether
//! the pending question was resolved (`serverRequest/resolved`). The thread is
//! ephemeral, the folder the check's own, and every MCP server is switched off for
//! the run, as the evals do. No rig is in it: Codex's app-server is spoken to
//! directly.
//!
//! ```text
//! cargo xtask live codex-interrupt
//! ```
//!
//! The run fails when Codex says neither that the turn ended nor that the thread
//! went idle, and when the model never asked its question: the run says to run it
//! again, and is no proof either way.

use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::codex::{self, isolation, methods, AppServer, Message};
use cf_e2e::live::members::Account;
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{agents, ask, Harness, Log};
use cf_e2e::process::{own_var, Run};
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

/// How long the model has to ask its question, how long Codex is watched after the
/// interrupt, and how long it has to answer a request.
const ASKED_WITHIN: Duration = Duration::from_secs(180);
const WATCHED_FOR: Duration = Duration::from_secs(20);
const REQUEST: Duration = Duration::from_secs(30);

/// What turns the question tool on, as a member's window has it.
const QUESTION_TOOL: [&str; 2] = ["--enable", "default_mode_request_user_input"];

/// What the model is asked.
const ASK: &str = "Use your request_user_input tool to ask me which colour to use, with the options red and blue. Do nothing else until it is answered.";

/// What Codex said of the interrupted turn, from what it sent after the interrupt.
#[derive(Debug, Clone)]
struct Said<'m> {
    /// The turn's end (`turn/completed`).
    ended: Option<&'m Message>,
    /// The thread's going idle (`thread/status/changed` to `idle`).
    idle: Option<&'m Message>,
    /// The question's resolution (`serverRequest/resolved` for its request).
    resolved: Option<&'m Message>,
    /// Every method it said of the thread, in order.
    methods: Vec<String>,
}

/// What of `after` is about the thread `thread`, and what it says of the turn.
/// `question` is the id of the request the question was asked in.
fn classify<'m>(after: &'m [Message], thread: &str, question: &Value) -> Said<'m> {
    let of_thread: Vec<&Message> = after
        .iter()
        .filter(|message| message.body["params"]["threadId"] == thread)
        .collect();
    let find = |wanted: &dyn Fn(&Value) -> bool| {
        of_thread
            .iter()
            .find(|message| wanted(&message.body))
            .copied()
    };
    Said {
        ended: find(&|body| body["method"] == "turn/completed"),
        idle: find(&|body| {
            body["method"] == "thread/status/changed" && body["params"]["status"]["type"] == "idle"
        }),
        resolved: find(&|body| {
            body["method"] == "serverRequest/resolved" && body["params"]["requestId"] == *question
        }),
        methods: methods(
            &of_thread
                .iter()
                .map(|message| (*message).clone())
                .collect::<Vec<_>>(),
        ),
    }
}

/// When a message came, in milliseconds after the interrupt (`interrupted_at`, how
/// old the app-server was then).
fn when(found: &Message, interrupted_at: Duration) -> String {
    let after = i128::try_from(found.at.as_millis()).unwrap_or(i128::MAX)
        - i128::try_from(interrupted_at.as_millis()).unwrap_or(i128::MAX);
    format!("{after} ms after the interrupt")
}

/// What the check says of what Codex sent: a line for each thing it looked for,
/// and the kinds of message it sent, and whether it held: Codex said that the
/// turn ended or that the thread went idle.
fn say(said: &Said, interrupted_at: Duration) -> Account {
    let mut case = Account::new();
    case.say(match said.ended {
        Some(ended) => format!(
            "ok   turn/completed, status {}, {}",
            ended.body["params"]["turn"]["status"]
                .as_str()
                .unwrap_or("undefined"),
            when(ended, interrupted_at)
        ),
        None => "MISS turn/completed never came".to_owned(),
    });
    case.say(match said.idle {
        Some(idle) => format!(
            "ok   thread/status/changed to idle, {}",
            when(idle, interrupted_at)
        ),
        None => "MISS the thread never went idle".to_owned(),
    });
    case.say(match said.resolved {
        Some(resolved) => format!(
            "ok   serverRequest/resolved for the question, {}",
            when(resolved, interrupted_at)
        ),
        None => "note the question was never resolved (serverRequest/resolved)".to_owned(),
    });
    case.say(format!(
        "     the thread's messages after the interrupt: {}",
        if said.methods.is_empty() {
            "none".to_owned()
        } else {
            said.methods.join(", ")
        }
    ));
    case.require(said.ended.is_some() || said.idle.is_some());
    case
}

/// The `app-server` Codex is started as: the question tool on, the cheap model,
/// and every MCP server of the human's shut out.
fn app_server_words(codex: &str, model: &str, isolation: Vec<String>) -> Vec<String> {
    let mut words = vec![codex.to_owned(), "app-server".to_owned()];
    words.extend(QUESTION_TOOL.map(str::to_owned));
    words.extend([
        "-c".to_owned(),
        "suppress_unstable_features_warning=true".to_owned(),
        "-c".to_owned(),
        format!("model={}", Value::String(model.to_owned())),
    ]);
    words.extend(isolation);
    words
}

/// What the run fails with when the model never used its tool to ask: the run says
/// nothing of the interrupt either way, so it is no pass and no proof, and it says to
/// run it again (the JavaScript check ended with its own exit code, 2, for it, which
/// is not a pass of a test).
fn never_asked(version: &str, model: &str, within: Duration) -> Error {
    Error::Daemon(format!(
        "{version} on {model} never asked its question in {} s: run it again (that says nothing of the interrupt)",
        within.as_secs()
    ))
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a Codex turn; cargo xtask live codex-interrupt (npm run live:codex-interrupt)"]
fn an_interrupted_question_ends_the_turn_or_idles_the_thread_in_what_codex_says() -> Result {
    let log = Log::new("live codex-interrupt");
    ask::prepare()?;
    let install = cf_e2e::live::codex()?;
    let version = install.version();
    let model = agents::cheap(Harness::Codex).0;
    log.say(format!(
        "codex is {} ({version}); its login is read from {}",
        install.command().display(),
        install.home().display()
    ));
    let flags = isolation(&codex::mcp_servers(install.command())?)?;
    // The folder is the check's own, and removed with it; Codex knows it by the path it resolves to.
    let scratch = tempfile::tempdir().map_err(|source| Error::File {
        action: "make a folder in",
        path: std::env::temp_dir(),
        source,
    })?;
    let folder = std::fs::canonicalize(scratch.path()).map_err(|source| Error::File {
        action: "resolve",
        path: scratch.path().to_path_buf(),
        source,
    })?;
    // An npm-installed Codex on Windows is a `.cmd` that is started as the node and the script it runs.
    let env: Vec<(String, String)> = ["PATH", "PATHEXT"]
        .into_iter()
        .filter_map(|name| Some((name.to_owned(), own_var(name)?)))
        .collect();
    let argv = ask::program(
        &app_server_words(&install.command().to_string_lossy(), model, flags),
        &env,
    )?;
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| Error::Daemon("Codex's app-server has no command line".to_owned()))?;
    let server = AppServer::start(Run::new(program).args(args).cwd(&folder).inheriting_env())?;

    server.request(
        "initialize",
        &json!({ "clientInfo": { "name": "consensflow-live", "version": "0" } }),
        REQUEST,
    )?;
    server.notify("initialized");
    let thread = server.request(
        "thread/start",
        &json!({
            "cwd": folder, "approvalPolicy": "never",
            "sandbox": "danger-full-access", "ephemeral": true,
        }),
        REQUEST,
    )?["thread"]["id"]
        .as_str()
        .ok_or_else(|| Error::Daemon("thread/start answered no thread".to_owned()))?
        .to_owned();
    let mark = server.mark();
    let turn = server.request(
        "turn/start",
        &json!({ "threadId": thread, "input": [{ "type": "text", "text": ASK }] }),
        REQUEST,
    )?["turn"]["id"]
        .as_str()
        .ok_or_else(|| Error::Daemon("turn/start answered no turn".to_owned()))?
        .to_owned();
    log.say(format!(
        "turn {turn} of thread {thread}: waiting up to {} s for its question",
        ASKED_WITHIN.as_secs()
    ));
    let Some(question) = server.next(
        mark,
        |message| {
            message["method"] == "item/tool/requestUserInput"
                && message["params"]["threadId"] == thread.as_str()
        },
        ASKED_WITHIN,
    ) else {
        return Err(never_asked(&version, model, ASKED_WITHIN));
    };
    let interrupted_at = server.age();
    let from = server.mark();
    server.request(
        "turn/interrupt",
        &json!({ "threadId": thread, "turnId": turn }),
        REQUEST,
    )?;
    log.say(format!(
        "the question {} is interrupted: watching for {} s",
        question.body["id"],
        WATCHED_FOR.as_secs()
    ));
    sleep(WATCHED_FOR);
    let after = server.since(from);

    let said = classify(&after, &thread, &question.body["id"]);
    let case = say(&said, interrupted_at);
    log.block(
        &format!(
            "{version} on {model}: question {} of turn {turn}, interrupted",
            question.body["id"]
        ),
        &case.text(),
    );
    let checks = vec![Check::new(
        "codex",
        "an interrupted question ends the turn or idles the thread",
        case.held(),
        "Codex said that the turn ended or that the thread went idle",
    )];
    report::verdict(&checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(at_ms: u64, body: Value) -> Message {
        Message {
            at: Duration::from_millis(at_ms),
            body,
        }
    }

    /// What Codex sent after the interrupt of the question `7` of the thread `t-1`:
    /// the thread's own, and a notification of another thread's that is none of it.
    fn after() -> Vec<Message> {
        vec![
            said(
                4_100,
                json!({ "method": "serverRequest/resolved", "params": { "threadId": "t-1", "requestId": 7 } }),
            ),
            said(
                4_200,
                json!({ "method": "turn/completed", "params": { "threadId": "t-1", "turn": { "id": "u-1", "status": "interrupted" } } }),
            ),
            said(
                4_250,
                json!({ "method": "thread/status/changed", "params": { "threadId": "t-1", "status": { "type": "idle" } } }),
            ),
            said(
                4_300,
                json!({ "method": "turn/completed", "params": { "threadId": "other", "turn": { "status": "completed" } } }),
            ),
        ]
    }

    #[test]
    fn what_codex_said_of_the_thread_is_its_end_its_idle_and_the_resolution_of_the_question() {
        let after = after();
        let said = classify(&after, "t-1", &json!(7));
        assert_eq!(said.ended.unwrap().at, Duration::from_millis(4_200));
        assert_eq!(said.idle.unwrap().at, Duration::from_millis(4_250));
        assert_eq!(said.resolved.unwrap().at, Duration::from_millis(4_100));
        // Of its own thread only, in the order it said it.
        assert_eq!(
            said.methods,
            [
                "serverRequest/resolved",
                "turn/completed",
                "thread/status/changed"
            ]
        );
    }

    #[test]
    fn a_resolution_is_the_questions_when_it_names_the_request_the_question_was_asked_in() {
        let after = after();
        assert!(classify(&after, "t-1", &json!(8)).resolved.is_none());
        // A thread that said nothing has no end and does not idle.
        let none = classify(&after, "t-2", &json!(7));
        assert!(none.ended.is_none() && none.idle.is_none() && none.methods.is_empty());
        // A status that is not idle is not the thread going idle.
        let busy = [said(
            1,
            json!({ "method": "thread/status/changed", "params": { "threadId": "t-1", "status": { "type": "active" } } }),
        )];
        assert!(classify(&busy, "t-1", &json!(7)).idle.is_none());
    }

    #[test]
    fn the_check_holds_when_the_turn_ended_or_the_thread_idled_and_says_both_and_what_else() {
        let after = after();
        let case = say(
            &classify(&after, "t-1", &json!(7)),
            Duration::from_millis(4_000),
        );
        assert!(case.held());
        assert_eq!(
            case.lines(),
            [
                "ok   turn/completed, status interrupted, 200 ms after the interrupt",
                "ok   thread/status/changed to idle, 250 ms after the interrupt",
                "ok   serverRequest/resolved for the question, 100 ms after the interrupt",
                "     the thread's messages after the interrupt: serverRequest/resolved, turn/completed, thread/status/changed",
            ]
        );
    }

    #[test]
    fn an_end_alone_or_an_idle_alone_is_enough_and_neither_is_the_failure() {
        let only_end = [after().remove(1)];
        let case = say(&classify(&only_end, "t-1", &json!(7)), Duration::ZERO);
        assert!(case.held());
        assert_eq!(case.lines()[1], "MISS the thread never went idle");
        assert_eq!(
            case.lines()[2],
            "note the question was never resolved (serverRequest/resolved)"
        );
        let only_idle = [after().remove(2)];
        let case = say(&classify(&only_idle, "t-1", &json!(7)), Duration::ZERO);
        assert!(case.held());
        assert_eq!(case.lines()[0], "MISS turn/completed never came");

        let neither = say(&classify(&[], "t-1", &json!(7)), Duration::ZERO);
        assert!(!neither.held());
        assert_eq!(
            neither.lines()[3],
            "     the thread's messages after the interrupt: none"
        );
        // The question being resolved does not make the turn end.
        let resolved = [after().remove(0)];
        assert!(!say(&classify(&resolved, "t-1", &json!(7)), Duration::ZERO).held());
    }

    #[test]
    fn a_model_that_never_asked_is_a_failure_that_says_to_run_it_again_and_proves_nothing() {
        let failed = never_asked(
            "codex-cli 0.160.1",
            "gpt-5.6-luna",
            Duration::from_secs(180),
        );
        assert_eq!(
            failed.to_string(),
            "codex-cli 0.160.1 on gpt-5.6-luna never asked its question in 180 s: run it again (that says nothing of the interrupt)"
        );
        // It is an error a test returns, which fails it: the old exit 2 passed for nothing.
        assert!(matches!(failed, Error::Daemon(_)));
    }

    #[test]
    fn a_message_before_the_interrupt_is_said_to_have_come_that_long_before_it() {
        let early = said(900, json!({}));
        assert_eq!(
            when(&early, Duration::from_millis(1_000)),
            "-100 ms after the interrupt"
        );
    }

    #[test]
    fn the_app_server_starts_with_the_question_tool_the_cheap_model_and_no_mcp_server_of_the_humans(
    ) {
        let words = app_server_words(
            "/bin/codex",
            "gpt-5.6-luna",
            vec!["-c".to_owned(), "mcp_servers.a.enabled=false".to_owned()],
        );
        assert_eq!(
            words,
            [
                "/bin/codex",
                "app-server",
                "--enable",
                "default_mode_request_user_input",
                "-c",
                "suppress_unstable_features_warning=true",
                "-c",
                "model=\"gpt-5.6-luna\"",
                "-c",
                "mcp_servers.a.enabled=false",
            ]
        );
    }
}
