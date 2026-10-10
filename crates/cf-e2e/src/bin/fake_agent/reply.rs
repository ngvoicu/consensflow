//! What the window answers a message with. Every message is one turn.
//!
//! - `Reply with exactly: X` answers X.
//! - A line `DISPATCH --tier standard <task>` (or `DISPATCH --review --tier
//!   standard <task>`) runs `cf task add` with those words and the task (`\n`
//!   in it becomes a line break), with this window's own token, the way a chief
//!   hands out work.
//! - A line `CF <words>` runs any other `cf` with those words as they stand
//!   (`CF task cancel T-3`), and `CF <words> :: <text>` with the text, spaces
//!   and all, as one last word (`CF tell T-1 :: Stop now`): the chief's own
//!   verbs, as it would type them.
//! - A line `SLEEP <seconds> <words>` is a turn that works that long before it
//!   does what the words say: a window a chief can still tell, or a chief
//!   nothing is delivered to.
//! - A task saying `QUOTA-OUT` is refused with a 429, Claude's way, by the
//!   window whose participant `CF_TEST_QUOTA_OUT` names, for as long as the
//!   file `CF_TEST_QUOTA_FILE` names is there when it is set (taking it away is
//!   the human logging the harness into another account).
//! - A line `ASK <questions JSON>` asks through Claude's question tool: the
//!   PreToolUse hook of the settings file this window was launched with runs on
//!   a synthetic AskUserQuestion event, and the turn answers with what the hook
//!   handed back.
//! - A question whose text says `REPLY <words>` is answered with `cf answer`.
//! - Anything else is acknowledged.

use std::path::Path;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use cf_e2e::pattern;
use cf_e2e::process::{own_var, Run};
use regex::Regex;
use serde_json::{json, Value};

/// What a window knows of itself that decides how it answers.
pub struct Window {
    /// The participant the daemon named this window for (`worker-amber-pine`).
    pub participant: String,
    /// The settings file the window was launched with, whose hooks a question
    /// goes through.
    pub settings: Option<String>,
    /// The conversation, for the hook's event.
    pub session: String,
}

/// What a turn answers.
#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    Text(String),
    /// The provider refuses it: Claude writes the refusal as a record.
    Refused,
}

/// `cf` as an agent runs it: the one first on the window's `PATH`, whichever
/// ConsensFlow installed there. What it printed and said, trimmed.
fn run_cf(words: &[String]) -> String {
    match Run::new("cf")
        .args(words)
        .inheriting_env()
        .unlimited()
        .run()
    {
        Ok(ran) => ran.output().trim().to_owned(),
        Err(failed) => failed.to_string(),
    }
}

/// Claude's question tool: the settings' PreToolUse hook answers, or the
/// window's dialog would. The hook is run in the platform's shell, as Claude
/// runs one: `/bin/sh` here, `cmd.exe` on Windows.
fn ask_through_hook(window: &Window, questions: &Value) -> Result<String, String> {
    let file = window
        .settings
        .as_deref()
        .ok_or("the window was launched with no settings file")?;
    let text = std::fs::read_to_string(file).map_err(|failed| format!("{file}: {failed}"))?;
    let settings: Value =
        serde_json::from_str(&text).map_err(|failed| format!("{file}: {failed}"))?;
    let command = settings["hooks"]["PreToolUse"]
        .as_array()
        .and_then(|hooks| {
            hooks
                .iter()
                .find(|hook| hook["matcher"] == "AskUserQuestion")
        })
        .and_then(|hook| hook["hooks"][0]["command"].as_str())
        .ok_or_else(|| format!("{file} holds no PreToolUse hook for AskUserQuestion"))?;
    let event = json!({
        "session_id": window.session,
        "cwd": std::env::current_dir().map(|dir| dir.to_string_lossy().into_owned()).unwrap_or_default(),
        "permission_mode": "bypassPermissions",
        "hook_event_name": "PreToolUse",
        "tool_name": "AskUserQuestion",
        "tool_input": { "questions": questions },
    });
    let run = shell(command)
        .inheriting_env()
        .unlimited()
        .input(event.to_string());
    let ran = run.run().map_err(|failed| failed.to_string())?;
    let answers = serde_json::from_str::<Value>(&ran.stdout)
        .ok()
        .map(|output| output["hookSpecificOutput"]["updatedInput"]["answers"].clone())
        .filter(Value::is_object);
    if let Some(answers) = answers {
        let said: Vec<String> = questions
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(
                |question| match &answers[question["question"].as_str().unwrap_or_default()] {
                    Value::String(answer) => answer.clone(),
                    Value::Null => String::new(),
                    other => other.to_string(),
                },
            )
            .collect();
        return Ok(format!("answered: {}", said.join(" | ")));
    }
    let errors = ran.stderr.trim();
    let code = ran
        .code
        .map_or_else(|| "null".to_owned(), |code| code.to_string());
    let said = format!(
        "unanswered (exit {code}): {}",
        if errors.is_empty() {
            "no output"
        } else {
            errors
        }
    );
    Ok(said.chars().take(600).collect())
}

/// A command line run by the platform's shell.
#[cfg(unix)]
fn shell(command: &str) -> Run {
    Run::new("/bin/sh").args(["-c", command])
}

/// A command line run by the platform's shell, as Node's `shell: true` runs
/// one: `cmd.exe /d /s /c "<command>"`, the quotes around it and its own quotes
/// written as they are.
#[cfg(windows)]
fn shell(command: &str) -> Run {
    let interpreter = own_var("ComSpec").unwrap_or_else(|| "cmd.exe".to_owned());
    Run::new(interpreter)
        .args(["/d", "/s", "/c"])
        .raw_arg(format!("\"{command}\""))
}

/// The answer to `text`.
pub fn reply_to(text: &str, window: &Window) -> Result<Reply, String> {
    static SLEEP: OnceLock<Regex> = OnceLock::new();
    static ASK: OnceLock<Regex> = OnceLock::new();
    static ASKED: OnceLock<Regex> = OnceLock::new();
    static REPLY: OnceLock<Regex> = OnceLock::new();
    static DISPATCH: OnceLock<Regex> = OnceLock::new();
    static VERB: OnceLock<Regex> = OnceLock::new();
    static REFUSED: OnceLock<Regex> = OnceLock::new();
    static EXACT: OnceLock<Regex> = OnceLock::new();

    if let Some(slow) = pattern::once(&SLEEP, r"(?m)^SLEEP (\d+) (.+)$").captures(text) {
        let seconds: u64 = slow[1].parse().map_err(|_| format!("SLEEP {}", &slow[1]))?;
        thread::sleep(Duration::from_secs(seconds));
        return reply_to(&slow[2], window);
    }
    if let Some(ask) = pattern::once(&ASK, r"(?m)^ASK (.+)$").captures(text) {
        let questions: Value =
            serde_json::from_str(&ask[1]).map_err(|failed| format!("ASK {}: {failed}", &ask[1]))?;
        return ask_through_hook(window, &questions).map(Reply::Text);
    }
    let asked =
        pattern::once(&ASKED, r"(?m)^\[ConsensFlow m-(\d+)[^\]]*question from @").captures(text);
    let reply = pattern::once(&REPLY, r"(?m)REPLY (.+)$").captures(text);
    if let (Some(asked), Some(reply)) = (asked, reply) {
        let words = [
            "answer".to_owned(),
            format!("m-{}", &asked[1]),
            reply[1].to_owned(),
        ];
        return Ok(Reply::Text(format!("replied: {}", run_cf(&words))));
    }
    if let Some(dispatch) = pattern::once(
        &DISPATCH,
        r"(?m)^DISPATCH ((?:(?:--(?:advice|review|design|self)|--\S+ \S+) )+)(.+)$",
    )
    .captures(text)
    {
        let mut words = vec!["task".to_owned(), "add".to_owned()];
        words.extend(dispatch[1].trim().split(' ').map(str::to_owned));
        words.push(dispatch[2].replace("\\n", "\n"));
        return Ok(Reply::Text(format!("dispatched: {}", run_cf(&words))));
    }
    if let Some(verb) = pattern::once(&VERB, r"(?m)^CF (.+)$").captures(text) {
        let mut parts = verb[1].split(" :: ");
        let mut words: Vec<String> = parts
            .next()
            .unwrap_or_default()
            .split(' ')
            .map(str::to_owned)
            .collect();
        let said: Vec<&str> = parts.collect();
        if !said.is_empty() {
            words.push(said.join(" :: "));
        }
        return Ok(Reply::Text(format!("ran cf: {}", run_cf(&words))));
    }
    if refused(text, window, pattern::once(&REFUSED, "QUOTA-OUT")) {
        return Ok(Reply::Refused);
    }
    if let Some(exact) = pattern::once(&EXACT, r"Reply with exactly: (\S+)").captures(text) {
        return Ok(Reply::Text(exact[1].to_owned()));
    }
    Ok(Reply::Text(format!(
        "noted: {}",
        text.split('\n').next().unwrap_or_default()
    )))
}

/// Whether the provider refuses this window the message `text`: it says
/// `QUOTA-OUT`, and the window is a session of the member `CF_TEST_QUOTA_OUT`
/// names, and the account is the refused one (`CF_TEST_QUOTA_FILE`, when it is
/// set, is still there).
fn refused(text: &str, window: &Window, quota_out: &Regex) -> bool {
    let refusing = own_var("CF_TEST_QUOTA_OUT").unwrap_or_default();
    let refused = own_var("CF_TEST_QUOTA_FILE").is_none_or(|account| Path::new(&account).exists());
    quota_out.is_match(text)
        && !refusing.is_empty()
        && refused
        && is_session_of(&window.participant, &refusing)
}

/// Whether `participant` is the member `member`, or a session of it
/// (`member-amber-pine`).
pub fn is_session_of(participant: &str, member: &str) -> bool {
    participant == member || participant.starts_with(&format!("{member}-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> Window {
        Window {
            participant: "chief".into(),
            settings: None,
            session: "sess".into(),
        }
    }

    fn text(reply: Result<Reply, String>) -> String {
        match reply {
            Ok(Reply::Text(text)) => text,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_message_that_asks_for_a_word_is_answered_with_it() {
        let reply = reply_to("Reply with exactly: WORKER_OK", &window());
        assert_eq!(text(reply), "WORKER_OK");
        // Anywhere in the message, as a task delivered to a window has its header first.
        let reply = reply_to(
            "[ConsensFlow m-4 · T-1 · task from @chief]\nReply with exactly: ONE_DONE",
            &window(),
        );
        assert_eq!(text(reply), "ONE_DONE");
    }

    #[test]
    fn any_other_message_is_acknowledged_by_its_first_line() {
        assert_eq!(text(reply_to("hello\nsecond", &window())), "noted: hello");
        assert_eq!(text(reply_to("", &window())), "noted: ");
    }

    #[test]
    fn a_window_is_a_session_of_a_member_by_its_name_and_the_two_words_after_it() {
        assert!(is_session_of("worker", "worker"));
        assert!(is_session_of("worker-amber-pine", "worker"));
        assert!(!is_session_of("worker2-amber-pine", "worker"));
        assert!(!is_session_of("chief", "worker"));
        assert!(!is_session_of("worker", "worker-amber"));
    }

    #[test]
    fn a_message_that_says_sleep_works_that_long_before_it_does_what_the_rest_says() {
        let started = std::time::Instant::now();
        let reply = reply_to("SLEEP 1 Reply with exactly: LATE", &window());
        assert_eq!(text(reply), "LATE");
        assert!(started.elapsed() >= Duration::from_secs(1));
    }

    #[test]
    fn a_question_is_asked_through_the_hook_of_the_settings_and_not_without_them() {
        let failed = reply_to(r#"ASK [{"question":"Which?"}]"#, &window()).unwrap_err();
        assert_eq!(failed, "the window was launched with no settings file");
        let failed = reply_to("ASK not json", &window()).unwrap_err();
        assert!(failed.starts_with("ASK not json: "), "{failed}");
    }

    #[cfg(unix)]
    #[test]
    fn the_hook_is_given_the_event_and_what_it_hands_back_is_the_answer() {
        let folder = tempfile::tempdir().unwrap();
        let settings = folder.path().join("settings.json");
        // A hook that answers the question it is asked with its own words.
        std::fs::write(
            &settings,
            json!({ "hooks": { "PreToolUse": [
                { "matcher": "Other", "hooks": [{ "command": "exit 9" }] },
                { "matcher": "AskUserQuestion", "hooks": [{ "command":
                    "cat > /dev/null; printf '%s' '{\"hookSpecificOutput\":{\"updatedInput\":{\"answers\":{\"Which colour?\":\"blue\",\"And?\":\"two\"}}}}'" }] },
            ] } })
            .to_string(),
        )
        .unwrap();
        let window = Window {
            settings: Some(settings.to_string_lossy().into_owned()),
            ..window()
        };
        let questions = json!([{ "question": "Which colour?" }, { "question": "And?" }, { "question": "Unasked" }]);
        assert_eq!(
            ask_through_hook(&window, &questions).unwrap(),
            "answered: blue | two | "
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_hook_that_answers_nothing_is_unanswered_with_its_exit_and_what_it_said() {
        let folder = tempfile::tempdir().unwrap();
        let settings = folder.path().join("settings.json");
        std::fs::write(
            &settings,
            json!({ "hooks": { "PreToolUse": [
                { "matcher": "AskUserQuestion", "hooks": [{ "command": "cat > /dev/null; echo no way >&2; exit 2" }] },
            ] } })
            .to_string(),
        )
        .unwrap();
        let window = Window {
            settings: Some(settings.to_string_lossy().into_owned()),
            ..window()
        };
        assert_eq!(
            ask_through_hook(&window, &json!([])).unwrap(),
            "unanswered (exit 2): no way"
        );
    }
}
