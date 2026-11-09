//! Devin's SessionStart hook for one launch: a session of the conversation
//! the window shows starts with its role text. A subagent's session, another
//! conversation's, or any other event gets nothing; and nothing the hook
//! meets (no role text, no wire log, input that is no event) stops Devin.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use cf_base::js;
use cf_base::json::{from_slice_lossy, js_order};
use serde_json::{json, Value};

use super::wire::selected_session;

/// The most of an event the hook reads; past it, the event is not read at all.
const SESSION_EVENT_LIMIT: u64 = 1024 * 1024;

/// How long the hook waits for the wire log to name the session its event starts ([`shown`]).
/// Devin gives the hook five seconds (`install::SESSION_HOOK_SECONDS`); what the wait leaves of
/// them is a start of `cf` on a machine that is busy, and the log's own writing.
const NAMED_WITHIN: Duration = Duration::from_secs(3);

/// How often the log is looked at while the hook waits for it.
const NAMED_EVERY: Duration = Duration::from_millis(10);

/// What `cf hook devin-session` prints for the event on `input`: the role
/// text in `role_file`, as the context a session starts with, when the event
/// starts a session of the conversation `wire_log` says the window shows.
/// Devin reads a SessionStart hook's output whole: no line break after it.
pub fn session_hook(
    input: &mut dyn Read,
    role_file: Option<&Path>,
    wire_log: Option<&Path>,
) -> Option<String> {
    context(input, role_file, wire_log, NAMED_WITHIN).map(|said| js_order(said).to_string())
}

/// The context `session_hook` gives, as JSON, once the log has named the session within `within`.
fn context(
    input: &mut dyn Read,
    role_file: Option<&Path>,
    wire_log: Option<&Path>,
    within: Duration,
) -> Option<Value> {
    let mut bytes = Vec::new();
    input
        .take(SESSION_EVENT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > SESSION_EVENT_LIMIT {
        return None;
    }
    let event = from_slice_lossy(&bytes).ok()?;
    let instructions = match role_file {
        Some(file) => String::from_utf8_lossy(&fs::read(file).ok()?).into_owned(),
        None => String::new(),
    };
    if event.get("hook_event_name").and_then(Value::as_str) != Some("SessionStart")
        || instructions.is_empty()
        || js::truthy(event.get("agent_id"))
        || js::truthy(event.get("parent_session_id"))
    {
        return None;
    }
    let session = event
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|id| session_id(id))?;
    if !shown(wire_log?, session, within) {
        return None;
    }
    Some(json!({
        "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": instructions }
    }))
}

/// Whether the wire log names `session` as the conversation the window shows, waiting up to
/// `within` for it to.
///
/// Devin runs a session's hook as its `session/new` returns, and its window writes the record
/// that names the conversation (the configuration of its mode) to the log on a path of its own,
/// a few milliseconds before or after: a hook that looked once found the log holding only its
/// leading `{}` lines and said nothing, so the session started with no role text (T-1065).
/// Of 11 live runs with the hook's reading logged (7 of the five harnesses one after another, 4
/// of Devin alone), the log named the session before the hook looked in 9; in the 2 it did not
/// (both of the five-harness runs) it held two or three `{}` lines, and in the one that was
/// measured it named the session 11 ms later. In the 10 whose Devin log was kept the hook
/// started within 6 ms of `session/new` returning. A `/new` or a `/resume` is the same race,
/// with the log still naming the conversation the window left. A log that names another
/// conversation throughout, or none, or cannot be read, gets nothing when the wait is over: a
/// session the window never shows is not given its text.
fn shown(wire_log: &Path, session: &str, within: Duration) -> bool {
    let until = Instant::now() + within;
    loop {
        if selected_session(wire_log).ok().flatten().as_deref() == Some(session) {
            return true;
        }
        let now = Instant::now();
        if now >= until {
            return false;
        }
        std::thread::sleep(NAMED_EVERY.min(until - now));
    }
}

/// Whether `id` can be a Devin session's: letters, digits, `_` and `-`, 1 to 200 of them.
fn session_id(id: &str) -> bool {
    (1..=200).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devin::install::SESSION_HOOK_SECONDS;
    use crate::devin::wire::tests::shows;

    struct Launch {
        wire: std::path::PathBuf,
        role: std::path::PathBuf,
        _dir: tempfile::TempDir,
    }

    /// One launch's files: the wire log showing native-a, and the role text.
    fn launch() -> Launch {
        let dir = tempfile::tempdir().unwrap();
        let wire = dir.path().join("wire.jsonl");
        let role = dir.path().join("SKILL.md");
        fs::write(&wire, shows("native-a")).unwrap();
        fs::write(&role, "# ConsensFlow worker\n").unwrap();
        Launch {
            _dir: dir,
            wire,
            role,
        }
    }

    /// The hook's answer to `event`, which looks at the log once.
    fn run(launch: &Launch, event: &Value) -> Option<Value> {
        run_within(launch, event, Duration::ZERO)
    }

    fn run_within(launch: &Launch, event: &Value, within: Duration) -> Option<Value> {
        context(
            &mut event.to_string().as_bytes(),
            Some(&launch.role),
            Some(&launch.wire),
            within,
        )
    }

    /// Appends what the window writes when it shows `session`, `after` the hook began to wait
    /// for it.
    fn window_shows(
        launch: &Launch,
        session: &'static str,
        after: Duration,
    ) -> std::thread::JoinHandle<()> {
        let wire = launch.wire.clone();
        std::thread::spawn(move || {
            std::thread::sleep(after);
            let mut log = fs::OpenOptions::new().append(true).open(wire).unwrap();
            std::io::Write::write_all(&mut log, shows(session).as_bytes()).unwrap();
        })
    }

    fn event(name: &str, session: &str) -> Value {
        json!({ "hook_event_name": name, "session_id": session })
    }

    fn role() -> Value {
        json!({ "hookSpecificOutput": {
            "hookEventName": "SessionStart", "additionalContext": "# ConsensFlow worker\n" } })
    }

    #[test]
    fn a_session_of_the_shown_conversation_starts_with_its_role_text_whatever_its_source() {
        let launch = launch();
        for source in ["startup", "resume", "clear"] {
            let mut start = event("SessionStart", "native-a");
            start["source"] = source.into();
            assert_eq!(run(&launch, &start), Some(role()), "{source}");
        }
        for name in ["UserPromptSubmit", "Stop", "SessionEnd", "PreToolUse"] {
            assert_eq!(run(&launch, &event(name, "native-a")), None, "{name}");
        }
    }

    #[test]
    fn another_conversation_a_subagent_or_a_bad_id_gets_nothing() {
        let launch = launch();
        let mut subagent = event("SessionStart", "native-a");
        subagent["agent_id"] = "subagent-1".into();
        let mut child = event("SessionStart", "native-a");
        child["parent_session_id"] = "native-a".into();
        let mut blank_parent = event("SessionStart", "native-a");
        blank_parent["parent_session_id"] = "".into();
        assert_eq!(run(&launch, &event("SessionStart", "native-b")), None);
        assert_eq!(run(&launch, &subagent), None);
        assert_eq!(run(&launch, &child), None);
        assert_eq!(run(&launch, &event("SessionStart", "../bad")), None);
        assert_eq!(
            run(&launch, &blank_parent),
            Some(role()),
            "an empty parent is none"
        );
    }

    #[test]
    fn after_a_new_conversation_its_start_is_the_one_answered() {
        let launch = launch();
        let mut log = fs::read_to_string(&launch.wire).unwrap();
        log.push_str(&shows("native-b"));
        fs::write(&launch.wire, log).unwrap();
        assert_eq!(
            run(&launch, &event("SessionStart", "native-b")),
            Some(role())
        );
        assert_eq!(run(&launch, &event("SessionStart", "native-a")), None);
    }

    #[test]
    fn nothing_it_meets_stops_devin() {
        let launch = launch();
        let start = event("SessionStart", "native-a");
        let line = start.to_string();
        let missing = launch.role.with_file_name("gone.md");
        assert_eq!(
            context(
                &mut line.as_bytes(),
                None,
                Some(&launch.wire),
                Duration::ZERO
            ),
            None
        );
        assert_eq!(
            context(
                &mut line.as_bytes(),
                Some(&missing),
                Some(&launch.wire),
                Duration::ZERO
            ),
            None
        );
        assert_eq!(
            context(
                &mut line.as_bytes(),
                Some(&launch.role),
                None,
                Duration::ZERO
            ),
            None
        );
        assert_eq!(
            context(
                &mut line.as_bytes(),
                Some(&launch.role),
                Some(&missing),
                Duration::ZERO
            ),
            None
        );
        assert_eq!(
            context(
                &mut &b"{\"hook_event_name\":"[..],
                Some(&launch.role),
                Some(&launch.wire),
                Duration::ZERO
            ),
            None
        );
        let mut huge = start.clone();
        huge["padding"] = "x".repeat(1024 * 1024).into();
        assert_eq!(
            run(&launch, &huge),
            None,
            "past a mebibyte the event is not read"
        );
        fs::write(&launch.role, "").unwrap();
        assert_eq!(
            run(&launch, &start),
            None,
            "an empty role text says nothing"
        );
    }

    #[test]
    fn a_start_before_the_log_names_its_conversation_is_answered_when_the_log_does() {
        // The shape of the live runs that lost (T-1065): the log held its leading `{}` lines.
        let launch = launch();
        fs::write(&launch.wire, "{}\n{}\n{}\n").unwrap();
        let start = event("SessionStart", "native-a");
        assert_eq!(run(&launch, &start), None, "one look finds nothing named");
        let window = window_shows(&launch, "native-a", Duration::from_millis(80));
        let began = Instant::now();
        assert_eq!(
            run_within(&launch, &start, Duration::from_secs(10)),
            Some(role())
        );
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "answered when the log named it, not when the wait ended"
        );
        window.join().unwrap();
    }

    #[test]
    fn a_new_conversations_start_waits_for_the_log_to_leave_the_one_the_window_left() {
        // `/new` and `/resume`: the log names the old conversation when the new one's hook runs.
        let launch = launch();
        let start = event("SessionStart", "native-b");
        assert_eq!(run(&launch, &start), None, "the log names native-a");
        let window = window_shows(&launch, "native-b", Duration::from_millis(80));
        assert_eq!(
            run_within(&launch, &start, Duration::from_secs(30)),
            Some(role())
        );
        window.join().unwrap();
    }

    #[test]
    fn a_start_the_log_never_names_gets_nothing_once_the_wait_is_over() {
        let launch = launch();
        let wait = Duration::from_millis(200);
        let began = Instant::now();
        assert_eq!(
            run_within(&launch, &event("SessionStart", "native-b"), wait),
            None
        );
        assert!(began.elapsed() >= wait, "it waited the whole time");
        fs::write(&launch.wire, "").unwrap();
        assert_eq!(
            run_within(&launch, &event("SessionStart", "native-a"), wait),
            None,
            "a log that names nothing"
        );
        fs::remove_file(&launch.wire).unwrap();
        assert_eq!(
            run_within(&launch, &event("SessionStart", "native-a"), wait),
            None,
            "a log that is not there"
        );
    }

    #[test]
    fn what_needs_no_log_does_not_wait_for_it() {
        // Not a session start, a child agent's, or a bad id: nothing to wait for.
        let launch = launch();
        let mut subagent = event("SessionStart", "native-b");
        subagent["agent_id"] = "subagent-1".into();
        let began = Instant::now();
        for event in [
            event("Stop", "native-b"),
            subagent,
            event("SessionStart", "../bad"),
        ] {
            assert_eq!(run_within(&launch, &event, Duration::from_secs(5)), None);
        }
        assert!(began.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn the_wait_ends_inside_the_time_devin_gives_the_hook() {
        let given = Duration::from_secs(u64::from(SESSION_HOOK_SECONDS));
        assert!(
            NAMED_WITHIN + Duration::from_secs(1) <= given,
            "{NAMED_WITHIN:?} of {given:?}"
        );
        assert!(NAMED_EVERY * 20 <= NAMED_WITHIN);
    }
}
