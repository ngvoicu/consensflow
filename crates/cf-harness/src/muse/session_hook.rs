//! Muse's SessionStart hook for one launch: each session the window starts or resumes
//! (`source` `startup`, `resume`, `clear`, `compact`) is written down for the window to
//! follow, and starts with the window's role text. A child agent's session, any other
//! event, or input that is no event gets nothing; and nothing the hook meets (no role
//! text, a folder it cannot write in) stops Muse.
//!
//! Muse runs a hook with an environment of its own (`PATH`, `HOME`, `MUSE_SESSION_ID`
//! and a few more: T-787's probe, Muse 1.4.13), so the launch names its files on the
//! hook's command line. It fires at a session's first prompt, not when the window
//! opens: what the role text says reaches that prompt's request, a resumed
//! conversation's too (T-702), where Muse replaces the block of the hook's words it
//! began with (`hook:session_start:session:0`), and stacks nothing.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use cf_base::js;
use cf_base::json::{from_slice_lossy, js_order};
use serde_json::{json, Value};

/// The most of an event the hook reads; past it, the event is not read at all.
const SESSION_EVENT_LIMIT: u64 = 1024 * 1024;

/// What `cf hook muse-session` prints for the event on `input`: the role text in
/// `role_file`, as the context the session starts with, and before that a line of
/// `{session, source}` appended to `said`, the launch's word of the sessions its
/// window showed. Muse reads a JSON answer's `additionalContext` (Claude's shape) as
/// it reads plain words: the block it keeps is the text alone.
pub fn session_hook(
    input: &mut dyn Read,
    role_file: Option<&Path>,
    said: Option<&Path>,
) -> Option<String> {
    let (session, source) = started(input)?;
    if let Some(said) = said {
        let line = json!({ "session": session, "source": source });
        // The window learns nothing of this start where the line cannot be written;
        // the session starts with its role text all the same.
        let _ = append(said, &format!("{line}\n"));
    }
    let instructions = String::from_utf8_lossy(&fs::read(role_file?).ok()?).into_owned();
    if instructions.is_empty() {
        return None;
    }
    let said = json!({
        "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": instructions }
    });
    Some(js_order(said).to_string())
}

/// The session the event on `input` starts, and its source (none said is `startup`):
/// none for any other event, a child agent's start, or a session id no session has.
fn started(input: &mut dyn Read) -> Option<(String, String)> {
    let mut bytes = Vec::new();
    input
        .take(SESSION_EVENT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > SESSION_EVENT_LIMIT {
        return None;
    }
    let event = from_slice_lossy(&bytes).ok()?;
    if event.get("hook_event_name").and_then(Value::as_str) != Some("SessionStart")
        || js::truthy(event.get("agent_id"))
        || js::truthy(event.get("parent_session_id"))
    {
        return None;
    }
    let session = event
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|id| session_id(id))?;
    let source = event
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("startup");
    Some((session.to_owned(), source.to_owned()))
}

/// Appends `line` to `file`, made private to this user where it is new.
fn append(file: &Path, line: &str) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(file)?.write_all(line.as_bytes())
}

/// Whether `id` can be a Muse session's: letters, digits and `-`, 1 to 200 of them.
fn session_id(id: &str) -> bool {
    (1..=200).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// The session a launch's word of its window's sessions, `said`, names last (the one
/// the window shows since its latest prompt), and when that was written, in ms since
/// the epoch. None where it names none yet.
pub(crate) fn last_said(said: &Path) -> Option<(String, i64)> {
    let file = fs::File::open(said).ok()?;
    let at = file
        .metadata()
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|since| i64::try_from(since.as_millis()).ok())?;
    let mut text = String::new();
    std::io::BufReader::new(file)
        .read_to_string(&mut text)
        .ok()?;
    text.lines().rev().find_map(|line| {
        let said: Value = serde_json::from_str(line).ok()?;
        said.get("session")
            .and_then(Value::as_str)
            .filter(|id| session_id(id))
            .map(|id| (id.to_owned(), at))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Launch {
        _dir: tempfile::TempDir,
        role: std::path::PathBuf,
        said: std::path::PathBuf,
    }

    fn launch() -> Launch {
        let dir = tempfile::tempdir().unwrap();
        let role = dir.path().join("SKILL.md");
        let said = dir.path().join("sessions.jsonl");
        fs::write(&role, "# ConsensFlow worker\n").unwrap();
        Launch {
            _dir: dir,
            role,
            said,
        }
    }

    fn run(launch: &Launch, event: &Value) -> Option<String> {
        session_hook(
            &mut event.to_string().as_bytes(),
            Some(&launch.role),
            Some(&launch.said),
        )
    }

    fn event(name: &str, session: &str) -> Value {
        json!({ "hook_event_name": name, "session_id": session, "cwd": "/work", "transcript_path": null })
    }

    const ROLE: &str = r##"{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"# ConsensFlow worker\n"}}"##;

    #[test]
    fn each_session_the_window_starts_is_written_down_and_starts_with_the_role_text() {
        let launch = launch();
        for (source, session) in [
            ("startup", "01a17bc7-6aab-75a0-bfae-b627eaa5ecd8"),
            ("clear", "01a17bd0-0000-7000-8000-000000000003"),
            ("resume", "01a17bc7-6aab-75a0-bfae-b627eaa5ecd8"),
        ] {
            let mut start = event("SessionStart", session);
            start["source"] = source.into();
            assert_eq!(run(&launch, &start).as_deref(), Some(ROLE), "{source}");
            assert_eq!(
                last_said(&launch.said).map(|(id, _)| id).as_deref(),
                Some(session),
                "{source}"
            );
        }
        let lines = fs::read_to_string(&launch.said).unwrap();
        assert_eq!(lines.lines().count(), 3);
        assert!(lines
            .lines()
            .nth(1)
            .unwrap()
            .contains(r#""source":"clear""#));
        for name in ["UserPromptSubmit", "Stop", "SessionEnd", "PreToolUse"] {
            assert_eq!(run(&launch, &event(name, "s-other")), None, "{name}");
        }
        assert_eq!(
            lines,
            fs::read_to_string(&launch.said).unwrap(),
            "nothing else said"
        );
    }

    #[test]
    fn a_child_agent_or_a_bad_id_gets_nothing_and_is_not_followed() {
        let launch = launch();
        let mut child = event("SessionStart", "child-1");
        child["agent_id"] = "child-1".into();
        let mut parented = event("SessionStart", "child-2");
        parented["parent_session_id"] = "01a17bc7-6aab-75a0-bfae-b627eaa5ecd8".into();
        for bad in [
            child,
            parented,
            event("SessionStart", "../up"),
            event("SessionStart", ""),
        ] {
            assert_eq!(run(&launch, &bad), None, "{bad}");
        }
        assert_eq!(last_said(&launch.said), None);
    }

    #[test]
    fn nothing_it_meets_stops_muse() {
        let launch = launch();
        let start = event("SessionStart", "s1");
        let line = start.to_string();
        // No role text: the start is still written down, and nothing is said.
        assert_eq!(
            session_hook(&mut line.as_bytes(), None, Some(&launch.said)),
            None
        );
        assert_eq!(
            last_said(&launch.said).map(|(id, _)| id).as_deref(),
            Some("s1")
        );
        let gone = launch.role.with_file_name("gone.md");
        assert_eq!(session_hook(&mut line.as_bytes(), Some(&gone), None), None);
        // A folder it cannot write in: the role text all the same.
        let nowhere = launch.said.with_file_name("no/such/folder/sessions.jsonl");
        assert_eq!(
            session_hook(&mut line.as_bytes(), Some(&launch.role), Some(&nowhere)).as_deref(),
            Some(ROLE)
        );
        assert_eq!(
            session_hook(&mut &b"{\"hook_event_name\":"[..], Some(&launch.role), None),
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
    fn the_last_session_said_is_read_past_a_torn_line() {
        let launch = launch();
        assert_eq!(last_said(&launch.said), None, "no file");
        fs::write(
            &launch.said,
            "{\"session\":\"s1\",\"source\":\"startup\"}\n{\"sess",
        )
        .unwrap();
        assert_eq!(
            last_said(&launch.said).map(|(id, _)| id).as_deref(),
            Some("s1")
        );
    }
}
