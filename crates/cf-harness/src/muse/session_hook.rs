//! Muse's SessionStart hooks for one launch: each session the window starts or resumes
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
//!
//! **A hook may say 16,384 bytes** (Muse Code 1.4.14-R7309.1, measured on 2026-11-05 with
//! its echo model, T-1246): one byte more and the hook ends `failed` with
//! `output_too_large`, and Muse keeps nothing of what it printed. The count is of the
//! hook's stdout whole, a JSON answer's wrapper and a line break after it included, and
//! plain words have the same ceiling; a hook that prints only blanks gives no block. A
//! role is longer than that (a chief's is 17 to 29 thousand bytes), so it goes in parts:
//! the launch gives Muse one hook for each ([`parts`]), and each hook prints its part
//! alone. Muse gives each hook that prints something a block of its own, a developer
//! message `hook:session_start:session:<n>` numbered in the hooks' order, each with the
//! ceiling to itself and the whole of its text kept: 20 hooks of 16,000 bytes each
//! were all there, in order, and no number of them or sum of their sizes was a limit.
//! A block stays in the conversation until a hook writes the same number again, and a
//! resumed one keeps the blocks it has, so a role that is shorter than it was at the
//! launch before (a part fewer) would leave the tail of the old one in the conversation:
//! the launch gives hooks for those numbers too, which print [`NO_MORE`] over it
//! ([`blocks_given`]).

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;

use cf_base::js;
use cf_base::json::{from_slice_lossy, js_order};
use serde_json::{json, Value};

/// The most of an event the hook reads; past it, the event is not read at all.
const SESSION_EVENT_LIMIT: u64 = 1024 * 1024;

/// The most bytes of output Muse Code takes of a hook: 16,384, inclusive, measured on
/// Muse Code 1.4.14-R7309.1 (a hook of 16,384 gave its block, one of 16,385 `failed`
/// with `output_too_large`). A test holds every part of every role to it.
pub const OUTPUT_LIMIT: usize = 16_384;

/// The most of that a hook is given to say: a little under, so that a Muse that counts
/// a line break or a byte more than 1.4.14 did still takes it.
const PART_OUTPUT: usize = 16_000;

/// The most parts a role may be given in: the most hooks of one event measured (20
/// hooks of 16,000 bytes, T-1246). A role that needs more is refused at its launch in
/// words, not started with none of it.
pub const MOST_PARTS: usize = 20;

/// What a hook says over a block of a role that is no more (`blocks_given`): a conversation
/// resumed on a role that fits in fewer parts than the one before it.
pub const NO_MORE: &str =
    "(No more of your ConsensFlow role here: its text ends in an earlier part.)";

/// What a hook prints to give `text` as the context a session starts with: Claude's
/// answer (`additionalContext`), which Muse reads as it reads plain words (the block it
/// keeps is the text alone), and which cannot be mistaken for a start of JSON that does
/// not parse (a plain part that opened with a `{`).
pub fn hook_output(text: &str) -> String {
    let said = json!({
        "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": text }
    });
    js_order(said).to_string()
}

/// The bytes `character` takes in a JSON string as serde_json writes it: `"` and `\`
/// and the line, tab and form breaks two, any other control character six, the rest as
/// they are.
fn encoded(character: char) -> usize {
    match character {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
        '\u{0}'..='\u{1f}' => 6,
        other => other.len_utf8(),
    }
}

/// The parts `role` is given to Muse in: consecutive pieces of it that, put one by one in a
/// hook's answer ([`hook_output`]), are each at most 16,000 bytes, and that put together are
/// the role. Muse keeps a block with the white space at both ends of it cut off (a hook
/// that printed `"\n\n  lead\n  two\n\n"` gave `"lead\n  two"`), so a part is cut where
/// that loses nothing: at the last line break of the part that leaves a line that begins
/// with a character that is not white space after it, else at the last line break, and in
/// the middle of a line (never of a character) only where a line is longer than a part.
/// A role no longer than a part is one part, and its hook says what it always did. No role
/// is no part.
pub fn parts(role: &str) -> Vec<&str> {
    let room = PART_OUTPUT - hook_output("").len();
    let mut parts = Vec::new();
    let (mut start, mut used) = (0, 0);
    // Where a line begins that a part may be cut before: one that begins with a character
    // that is none of white space's, and one that does, the first the better.
    let (mut plain, mut any): (Option<usize>, Option<usize>) = (None, None);
    let mut after_break = false;
    for (at, character) in role.char_indices() {
        if after_break && at > start {
            if character.is_whitespace() {
                any = Some(at);
            } else {
                plain = Some(at);
            }
        }
        let cost = encoded(character);
        if used + cost > room {
            let cut = plain.or(any).unwrap_or(at);
            parts.push(&role[start..cut]);
            start = cut;
            used = role[cut..at].chars().map(encoded).sum();
            plain = None;
            any = any.filter(|&line| line > cut);
        }
        used += cost;
        after_break = character == '\n';
    }
    if start < role.len() {
        parts.push(&role[start..]);
    }
    parts
}

/// What `cf hook muse-session` prints for the event on `input`: the part number `part`
/// (the first is 0) of the role text in `role_file`, as the context the session starts
/// with, and, for the first part, before that a line of `{session, source}` appended to
/// `said`, the launch's word of the sessions its window showed. Muse reads a JSON
/// answer's `additionalContext` (Claude's shape) as it reads plain words: the block it
/// keeps is the text alone. A part number past the role's last prints [`NO_MORE`].
pub fn session_hook(
    input: &mut dyn Read,
    role_file: Option<&Path>,
    said: Option<&Path>,
    part: usize,
) -> Option<String> {
    let (session, source) = started(input)?;
    if let (0, Some(said)) = (part, said) {
        let line = json!({ "session": session, "source": source });
        // The window learns nothing of this start where the line cannot be written;
        // the session starts with its role text all the same.
        let _ = append(said, &format!("{line}\n"));
    }
    let instructions = String::from_utf8_lossy(&fs::read(role_file?).ok()?).into_owned();
    let parts = parts(&instructions);
    if parts.is_empty() {
        return None;
    }
    Some(hook_output(parts.get(part).copied().unwrap_or(NO_MORE)))
}

/// How many context blocks the SessionStart hooks of the launches before this one gave
/// the session whose log is `log`: one more than the highest number of a block of theirs
/// (`hook:session_start:session:<n>`) in it. Nothing where the log cannot be read, or is
/// too long to read for this (past 512 MiB): a launch then is given no hook to write over
/// the blocks a longer role left.
pub(crate) fn blocks_given(log: &Path) -> usize {
    const MARK: &[u8] = b"hook:session_start:session:";
    const LONGEST_LOG: u64 = 512 * 1024 * 1024;
    let Ok(file) = fs::File::open(log) else {
        return 0;
    };
    if file
        .metadata()
        .map_or(true, |meta| meta.len() > LONGEST_LOG)
    {
        return 0;
    }
    let mut reader = BufReader::new(file);
    let (mut line, mut given) = (Vec::new(), 0);
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return given,
            Ok(_) => {}
        }
        let mut rest = &line[..];
        while let Some(at) = memchr::memmem::find(rest, MARK) {
            rest = &rest[at + MARK.len()..];
            let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
            let number = std::str::from_utf8(&rest[..digits])
                .ok()
                .and_then(|digits| digits.parse::<usize>().ok());
            if let Some(number) = number {
                given = given.max(number.saturating_add(1));
            }
        }
    }
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

/// Whether a launch's word of its window's sessions, `said`, names `session`: the hook
/// greeted it with the window's role text at its first prompt. A session a `/clear`
/// started is never named (Muse runs no SessionStart hook for it).
pub(crate) fn greeted(said: &Path, session: &str) -> bool {
    fs::read_to_string(said).is_ok_and(|text| {
        text.lines().any(|line| {
            serde_json::from_str::<Value>(line)
                .is_ok_and(|said| said.get("session").and_then(Value::as_str) == Some(session))
        })
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
            0,
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
            session_hook(&mut line.as_bytes(), None, Some(&launch.said), 0),
            None
        );
        assert_eq!(
            last_said(&launch.said).map(|(id, _)| id).as_deref(),
            Some("s1")
        );
        let gone = launch.role.with_file_name("gone.md");
        assert_eq!(
            session_hook(&mut line.as_bytes(), Some(&gone), None, 0),
            None
        );
        // A folder it cannot write in: the role text all the same.
        let nowhere = launch.said.with_file_name("no/such/folder/sessions.jsonl");
        assert_eq!(
            session_hook(&mut line.as_bytes(), Some(&launch.role), Some(&nowhere), 0).as_deref(),
            Some(ROLE)
        );
        assert_eq!(
            session_hook(
                &mut &b"{\"hook_event_name\":"[..],
                Some(&launch.role),
                None,
                0
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

    /// The numbers the module rests on, measured on Muse Code 1.4.14-R7309.1 with its echo model
    /// (T-1246; the rig's `muse_role` cases hold them on the machine's Muse): a hook may say
    /// 16,384 bytes, a part is given 16,000, and 20 hooks of one start were all taken. A Muse
    /// that changes them is the cases' to fail, and these to be changed with them.
    #[test]
    fn the_numbers_are_those_measured_on_muse_1_4_14() {
        assert_eq!(OUTPUT_LIMIT, 16_384);
        assert_eq!(PART_OUTPUT, 16_000);
        assert_eq!(MOST_PARTS, 20);
        assert_eq!(hook_output("").len(), 78, "the wrapper of a JSON answer");
    }

    /// A role of `lines` lines of `width` characters of `character`, each ended by a line break.
    fn role_of(character: char, width: usize, lines: usize) -> String {
        let line = format!("{}\n", character.to_string().repeat(width));
        line.repeat(lines)
    }

    /// What a hook prints for `part` of `role`, and the text it says in it.
    fn said_part(launch: &Launch, role: &str, part: usize) -> (String, String) {
        fs::write(&launch.role, role).unwrap();
        let start = event("SessionStart", "s1");
        let printed = session_hook(
            &mut start.to_string().as_bytes(),
            Some(&launch.role),
            Some(&launch.said),
            part,
        )
        .expect("a hook has a part to print");
        let said: Value = serde_json::from_str(&printed).expect("what a hook prints is JSON");
        let text = said["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .expect("its text")
            .to_owned();
        (printed, text)
    }

    #[test]
    fn a_role_is_given_in_parts_that_each_go_in_a_hook_under_the_limit_and_are_the_role_together() {
        let launch = launch();
        let overhead = hook_output("").len();
        // What each part makes of text that is plain, escapes to twice its bytes, has the
        // control characters, and is of two, three and four bytes to a character, with the
        // line a hook's text is cut on at every size.
        for (character, width, lines) in [
            ('a', 70, 480),
            ('"', 30, 480),
            ('\\', 100, 300),
            ('é', 40, 480),
            ('日', 25, 480),
            ('😀', 20, 480),
            ('\u{1}', 9, 480),
            ('a', 70, 1),
            ('a', 40_000, 1),
            ('日', 40_000, 1),
        ] {
            let role = role_of(character, width, lines);
            let cut = parts(&role);
            assert_eq!(cut.concat(), role, "{character:?} {width} {lines}");
            for (at, part) in cut.iter().enumerate() {
                let (printed, text) = said_part(&launch, &role, at);
                assert_eq!(text, *part);
                assert_eq!(
                    printed.len(),
                    overhead + part.chars().map(encoded).sum::<usize>(),
                    "the bytes of a character are as serde_json writes them: {character:?}"
                );
                assert!(
                    printed.len() <= PART_OUTPUT && PART_OUTPUT < OUTPUT_LIMIT,
                    "{character:?} {width} {lines}: part {at} is {} bytes",
                    printed.len()
                );
            }
        }
    }

    #[test]
    fn a_part_ends_at_a_line_break_unless_a_line_is_longer_than_a_part() {
        // 480 lines of 70 characters: each part is whole lines and as many of them as fit.
        let role = role_of('a', 70, 480);
        let cut = parts(&role);
        assert!(cut.len() > 1, "{}", role.len());
        assert!(cut.iter().all(|part| part.ends_with('\n')));
        for pair in cut.windows(2) {
            let next_line = 71;
            assert!(
                hook_output(pair[0]).len() + next_line > PART_OUTPUT,
                "a part left room for the next line"
            );
        }
        // A line of 40,000 characters is cut where it must be, whole characters of it.
        let long = "日".repeat(40_000);
        let cut = parts(&long);
        assert!(cut.len() >= 8, "{}", cut.len());
        assert_eq!(cut.concat(), long);
    }

    /// What Muse keeps of a block a hook printed: its white space at both ends cut off
    /// (Muse Code 1.4.14, measured on its echo model).
    fn kept(part: &str) -> &str {
        part.trim()
    }

    #[test]
    fn a_part_is_cut_before_a_line_muse_would_lose_no_indentation_of_where_the_role_has_one() {
        // Items indented under a heading: each part after the first begins at a heading.
        let role =
            "## Heading\n  - an indented item of the list\n  - and another one\n".repeat(700);
        let cut = parts(&role);
        assert!(cut.len() > 2, "{}", role.len());
        for part in &cut[1..] {
            assert!(part.starts_with("## Heading"), "{:?}", &part[..30]);
        }
        // What Muse keeps of each, in order, with the line breaks it cut between them put
        // back, is the role: nothing but a line break is lost at a seam.
        assert_eq!(
            cut.iter()
                .map(|part| kept(part))
                .collect::<Vec<_>>()
                .join("\n"),
            role.trim_end()
        );
        // A block with no line that begins with a letter is cut at a line break all the same
        // (the indentation of the line it begins with is Muse's to cut), and a part does not
        // begin in the middle of a line.
        let code = format!("start\n{}", "    a line of code\n".repeat(2_000));
        let cut = parts(&code);
        assert!(cut.len() > 2);
        assert!(cut
            .iter()
            .all(|part| part.ends_with('\n') || *part == cut[cut.len() - 1]));
        assert_eq!(cut.concat(), code);
    }

    #[test]
    fn a_role_that_fits_in_a_hook_is_one_part_and_is_said_as_it_always_was() {
        let launch = launch();
        let role = "# ConsensFlow worker\n";
        assert_eq!(parts(role), [role]);
        assert_eq!(parts(""), Vec::<&str>::new());
        let (printed, _) = said_part(&launch, role, 0);
        assert_eq!(printed, ROLE);
        // The most that goes in one: the hook is at its budget, under Muse's limit.
        let room = PART_OUTPUT - hook_output("").len();
        let exact = "a".repeat(room);
        assert_eq!(parts(&exact), [exact.as_str()]);
        assert_eq!(hook_output(&exact).len(), PART_OUTPUT);
        assert_eq!(parts(&format!("{exact}a")).len(), 2);
    }

    #[test]
    fn a_part_past_the_roles_last_says_there_is_no_more_and_only_the_first_writes_the_session_down()
    {
        let launch = launch();
        let role = role_of('a', 70, 480);
        let last = parts(&role).len();
        let (_, over) = said_part(&launch, &role, last);
        assert_eq!(over, NO_MORE);
        let (_, first) = said_part(&launch, &role, 0);
        assert_eq!(first, parts(&role)[0]);
        // Three starts of the one session went through four hooks' runs: one line.
        assert_eq!(fs::read_to_string(&launch.said).unwrap().lines().count(), 1);
        // No role text says nothing, whatever the part.
        assert_eq!(
            session_hook(
                &mut event("SessionStart", "s1").to_string().as_bytes(),
                Some(&launch.role.with_file_name("gone.md")),
                None,
                1
            ),
            None
        );
    }

    #[test]
    fn the_blocks_an_earlier_launch_gave_are_one_more_than_the_highest_number_in_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("session.jsonl");
        assert_eq!(blocks_given(&log), 0, "no log");
        fs::write(
            &log,
            [
                r#"{"event":{"kind":"context_block_updated","id":"hook:session_start:session:0","text":"a"}}"#,
                r#"{"event":{"kind":"context_block_updated","id":"hook:session_start:session:2","text":"b"}}"#,
                r#"{"block_id":"hook:session_start:session:1"} and "hook:session_start:session:11""#,
                r#"{"other":"hook:session_start:session:x","id":"session:99"}"#,
                "{\"event\":{\"id\":\"hook:session_start:session:3",
            ]
            .join("\n"),
        )
        .unwrap();
        assert_eq!(
            blocks_given(&log),
            12,
            "11 is the highest, read past a torn last line"
        );
        fs::write(&log, "{}\n").unwrap();
        assert_eq!(blocks_given(&log), 0, "no block");
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
