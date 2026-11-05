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
//! conversation's too (T-702).
//!
//! # What Muse does with a hook's output (Muse Code 1.4.14-R7309.1, T-1246, measured with
//! # its echo model; the rig's `muse_role` cases hold each)
//!
//! - **A hook may say 16,384 bytes.** One byte more and the hook ends `failed` with
//!   `output_too_large`, and Muse keeps nothing of what it printed. The count is of the
//!   hook's stdout whole, a JSON answer's wrapper and a line break after it included;
//!   plain words have the same ceiling.
//! - **Each hook that prints something is a block of its own,** a developer message
//!   `hook:session_start:session:<n>` with the ceiling to itself and the whole of its text
//!   kept (20 hooks of 16,000 bytes were all there, and 64 hooks of 50 bytes: no count or
//!   sum was a limit; the hooks run in parallel). `<n>` counts the hooks that printed, in
//!   the order of the hooks in the configuration, whenever each finished: a hook that
//!   prints nothing, or fails, takes no number, and every hook after it has one less.
//! - **Muse cuts the white space at both ends of a block.**
//! - **A block stays in the conversation until a hook writes the same number again,** and
//!   a resumed conversation keeps the blocks it has.
//!
//! # So a role goes in parts, each a hook, first in the launch's hooks
//!
//! A role is longer than a hook may say (a chief's is 17 to 38 thousand bytes of output),
//! so the launch gives Muse a hook for each part of it ([`parts`]), and each prints its
//! part alone. The launch's hooks come **before the owner's** (`launch::hooks`): ours
//! always print, so the part `k` of the role is always block `k`, whatever the owner's
//! hooks print (a hook of the owner's that prints only sometimes, or only at a start,
//! shifted every block of ours that came after it).
//!
//! **A role of more than one part is framed** ([`blocks`]): each block is a line that says
//! the role it is of, its part and how many there are, the part itself, and a line that
//! ends it. Muse's cut of the white space at the ends of a block then touches only the
//! lines of the frame, and what the blocks hold between their frames, put together, is
//! the role to the byte (a part that ends in two significant spaces, or begins with an
//! indented line, kept them). The model can see the order and the whole of its role, and
//! that a block of another role (the `id` is a digest of the whole text) is an old one. A
//! role that fits in one hook is given as it always was.
//!
//! **A conversation that is resumed keeps the blocks its earlier launches wrote,** and a
//! role that now needs fewer parts would leave the tail of the old one where the model
//! may act on it. So the launch gives a hook for every block of ours the conversation
//! has ([`cover_of`]: the highest block of the session's log whose last text is ours, and
//! all [`MOST_PARTS`] where the log cannot be read or counted in time), and a hook past
//! the role's last part writes [`filler`] over its block. Nothing of this depends on what
//! the owner's hooks print.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use cf_base::js;
use cf_base::json::{from_slice_lossy, js_order};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// The most of an event the hook reads; past it, the event is not read at all.
const SESSION_EVENT_LIMIT: u64 = 1024 * 1024;

/// The most bytes of output Muse Code takes of a hook: 16,384, inclusive, measured on
/// Muse Code 1.4.14-R7309.1 (a hook of 16,384 gave its block, one of 16,385 `failed`
/// with `output_too_large`). A test holds every part of every role to it.
pub const OUTPUT_LIMIT: usize = 16_384;

/// The most of that a hook is given to say: a little under, so that a Muse that counts
/// a line break or a byte more than 1.4.14 did still takes it.
const PART_OUTPUT: usize = 16_000;

/// The most parts a role may be given in, and the most hooks of ours a launch has: the
/// most hooks of one event measured (T-1246: 20 hooks of 16,000 bytes, and 64 smaller).
/// A role that needs more is refused at its launch in words, not started with none of it.
pub const MOST_PARTS: usize = 20;

/// What every block of ours begins with, a part's frame and a filler alike: how a block of
/// the session's log is known for a ConsensFlow role's, which no hook of the owner's says.
pub const MARK: &str = "<<consensflow-role";

/// How an unframed role (one hook's worth) begins: the front matter of its skill file. A
/// conversation begun by a ConsensFlow that gave its role so, after the owner's hooks, has
/// it at a block of that number still.
const FRONT_MATTER: &str = "---\nname: consensflow-";

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

/// The digest that says which role a block is of: the first 8 hex digits of the SHA-256 of
/// the whole text.
fn id_of(role: &str) -> String {
    Sha256::digest(role.as_bytes())[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The line that begins the block of part `part` (the first is 1) of `of`, of the role
/// `id`. The first says what the rest are.
fn head(id: &str, part: usize, of: usize) -> String {
    if part == 1 {
        format!("{MARK} {id} part 1 of {of}: your role is these {of} parts in order, each marked {id}; ignore any other consensflow-role block>>")
    } else {
        format!("{MARK} {id} part {part} of {of}>>")
    }
}

/// The line that ends the block of part `part` of `of`, of the role `id`.
fn tail(id: &str, part: usize, of: usize) -> String {
    format!("<<end of consensflow-role {id} part {part} of {of}>>")
}

/// What a hook for part `part` (the first is 1) of a role of `of` parts, `id`, says where
/// the role has none: over the block an older, longer role left in a conversation.
pub fn filler(id: &str, part: usize, of: usize) -> String {
    format!(
        "{MARK} {id} part {part}: there is no part {part} of this role, which has {of} part{}>>",
        if of == 1 { "" } else { "s" }
    )
}

/// The bytes a frame takes at the most: a first part's head and a last part's tail of the
/// most parts, and the two line breaks that join them to the part.
fn frame_most() -> usize {
    let id = "0".repeat(8);
    head(&id, 1, MOST_PARTS).len() + tail(&id, MOST_PARTS, MOST_PARTS).len() + 2
}

/// The parts `role` is cut into, each to be said by a hook: consecutive pieces of it
/// that put together are the role, each of which, framed ([`blocks`]) and put in a hook's
/// answer ([`hook_output`]), is at most 16,000 bytes. A role that fits in one hook as it
/// is is one part, with no frame. A part ends at the last line start of its room that
/// is outside a code fence (a line that begins with three backticks or tildes opens one,
/// and closes it again), else at the last line start of it, and in the middle of a line
/// (never of a character) only where a line is longer than a part. No role is no part.
pub fn parts(role: &str) -> Vec<&str> {
    if role.is_empty() {
        return Vec::new();
    }
    if hook_output(role).len() <= PART_OUTPUT {
        return vec![role];
    }
    cut(role, PART_OUTPUT - hook_output("").len() - frame_most())
}

/// `role` cut in pieces each of which takes at most `room` bytes of a JSON string.
fn cut(role: &str, room: usize) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut used) = (0, 0);
    // The last line start of the part so far, outside a fence and anywhere.
    let (mut outside, mut any): (Option<usize>, Option<usize>) = (None, None);
    let mut fence: Option<(char, usize)> = None;
    let mut line_starts = true;
    for (at, character) in role.char_indices() {
        if line_starts {
            if at > start {
                any = Some(at);
                if fence.is_none() {
                    outside = Some(at);
                }
            }
            fence = fenced(fence, &role[at..]);
        }
        let cost = encoded(character);
        if used + cost > room {
            let end = outside.or(any).unwrap_or(at);
            parts.push(&role[start..end]);
            start = end;
            used = role[end..at].chars().map(encoded).sum();
            outside = None;
            any = any.filter(|&line| line > end);
        }
        used += cost;
        line_starts = character == '\n';
    }
    if start < role.len() {
        parts.push(&role[start..]);
    }
    parts
}

/// Whether a code fence is open after the line that begins `text`, given whether one was
/// (`fence`: the character and the length of the run that opened it): a line of up to
/// three spaces and a run of three or more backticks or tildes opens one, and a run of the
/// same character, as long, with nothing but white space after it, closes it.
fn fenced(fence: Option<(char, usize)>, text: &str) -> Option<(char, usize)> {
    let line = text.split('\n').next().unwrap_or("");
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return fence;
    }
    let rest = &line[indent..];
    let Some(mark) = rest.chars().next().filter(|mark| matches!(mark, '`' | '~')) else {
        return fence;
    };
    let run = rest
        .chars()
        .take_while(|&character| character == mark)
        .count();
    if run < 3 {
        return fence;
    }
    match fence {
        None => Some((mark, run)),
        Some((open, length)) => {
            let after = &rest[run..];
            if mark == open && run >= length && after.trim().is_empty() {
                None
            } else {
                fence
            }
        }
    }
}

/// What the hooks of a launch say, the text of the block each gives, in the order of the
/// hooks: the parts of the role ([`parts`]), a role of more than one framed, a role of one
/// as it is, and none for no role.
pub fn blocks(role: &str) -> Vec<String> {
    let parts = parts(role);
    if parts.len() <= 1 {
        return parts.into_iter().map(str::to_owned).collect();
    }
    let (id, of) = (id_of(role), parts.len());
    parts
        .iter()
        .enumerate()
        .map(|(at, part)| {
            format!(
                "{}\n{part}\n{}",
                head(&id, at + 1, of),
                tail(&id, at + 1, of)
            )
        })
        .collect()
}

/// What `cf hook muse-session` prints for the event on `input`: the block number `part`
/// (the first is 0) of the role text in `role_file`, as the context the session starts
/// with, and, for the first, before that a line of `{session, source}` appended to
/// `said`, the launch's word of the sessions its window showed. Muse reads a JSON
/// answer's `additionalContext` (Claude's shape) as it reads plain words: the block it
/// keeps is the text alone. A number past the role's last prints [`filler`].
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
    let blocks = blocks(&instructions);
    if blocks.is_empty() {
        return None;
    }
    let block = blocks
        .get(part)
        .cloned()
        .unwrap_or_else(|| filler(&id_of(&instructions), part + 1, blocks.len()));
    Some(hook_output(&block))
}

/// Whether `text`, a block of a session's log, is a ConsensFlow role's: a part or a filler
/// of ours ([`MARK`]), or a role given as it is.
fn ours(text: &str) -> bool {
    text.starts_with(MARK) || text.starts_with(FRONT_MATTER)
}

/// The longest log [`cover_of`] reads: past it the answer is that it cannot tell.
const LONGEST_LOG: u64 = 1024 * 1024 * 1024;

/// The longest line of a log [`cover_of`] looks into: a block of a role is a few tens of
/// thousands of bytes, and a line of a tool's output may be many megabytes.
const LONGEST_LINE: usize = 4 * 1024 * 1024;

/// How many hooks of ours a conversation's earlier launches wrote blocks for, which this
/// launch's hooks must write over: one more than the highest block number of the session's
/// log `log` whose last text is ours ([`ours`]); none where it has no block of ours. None
/// where it cannot tell: the log cannot be opened or read, is past [`LONGEST_LOG`], or
/// takes more than `within` to read; the launch then covers every block a role could have
/// ([`MOST_PARTS`]). The owner's blocks are no part of it: a block counts for its last
/// text, and an owner's text is no role of ours.
pub(crate) fn cover_of(log: &Path, within: Duration) -> Option<usize> {
    const BLOCKS: &[u8] = b"hook:session_start:session:";
    const WRITTEN: &[u8] = b"context_block_updated";
    let began = Instant::now();
    let file = fs::File::open(log).ok()?;
    if file.metadata().ok()?.len() > LONGEST_LOG {
        return None;
    }
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut last: std::collections::BTreeMap<usize, bool> = std::collections::BTreeMap::new();
    let (mut line, mut too_long) = (Vec::new(), false);
    loop {
        if began.elapsed() > within {
            return None;
        }
        let chunk = reader.fill_buf().ok()?;
        let end = chunk.is_empty();
        let newline = memchr::memchr(b'\n', chunk);
        let take = newline.unwrap_or(chunk.len());
        if !too_long {
            if line.len() + take > LONGEST_LINE {
                too_long = true;
                line.clear();
            } else {
                line.extend_from_slice(&chunk[..take]);
            }
        }
        let consumed = newline.map_or(chunk.len(), |at| at + 1);
        reader.consume(consumed);
        if newline.is_some() || end {
            if !too_long
                && memchr::memmem::find(&line, WRITTEN).is_some()
                && memchr::memmem::find(&line, BLOCKS).is_some()
            {
                if let Some((number, text)) = block_of(&line) {
                    last.insert(number, ours(&text));
                }
            }
            line.clear();
            too_long = false;
        }
        if end {
            break;
        }
    }
    Some(
        last.iter()
            .rev()
            .find(|(_, is_ours)| **is_ours)
            .map_or(0, |(number, _)| number + 1),
    )
}

/// The number and the text of the block a log's line writes (a `context_block_updated`
/// of `hook:session_start:session:<n>`), none for any other line.
fn block_of(line: &[u8]) -> Option<(usize, String)> {
    let record: Value = serde_json::from_slice(line).ok()?;
    let event = record.get("payload")?.get("event")?;
    if event.get("kind")?.as_str()? != "context_block_updated" {
        return None;
    }
    let number = event
        .get("id")?
        .as_str()?
        .strip_prefix("hook:session_start:session:")?
        .parse::<usize>()
        .ok()?;
    Some((number, event.get("text")?.as_str()?.to_owned()))
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
        assert!(
            frame_most() < 400,
            "a frame is lines, not a page: {}",
            frame_most()
        );
    }

    /// A block of a framed role as its parts are read back: the part, its number and how
    /// many there are, after the frame's two lines were checked to be those of that part of
    /// a role of that id.
    fn unframed(block: &str) -> (usize, usize, &str, String) {
        let (first, rest) = block.split_once('\n').expect("a head line");
        let (inner, last) = rest.rsplit_once('\n').expect("a tail line");
        let words = first.strip_prefix(MARK).expect("the mark").trim_start();
        let (id, words) = words.split_once(' ').expect("an id");
        let words = words.strip_prefix("part ").expect("a part");
        let number: usize = words
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .expect("its number");
        let of: usize = words
            .split_once(" of ")
            .expect("of")
            .1
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .expect("how many");
        assert_eq!(first, head(id, number, of), "the head line");
        assert_eq!(last, tail(id, number, of), "the tail line");
        (number, of, inner, id.to_owned())
    }

    /// What Muse keeps of `blocks`, each as it cuts it (white space off both ends), with the
    /// frames of the parts taken off and the parts put together: what the model has of the
    /// role. Each block's number, how many parts it says there are and its id are checked to
    /// be those of the one role, in order.
    fn kept(blocks: &[String]) -> String {
        let mut role = String::new();
        let mut id = None;
        for (at, block) in blocks.iter().enumerate() {
            let cut = block.trim();
            assert_eq!(
                cut, block,
                "Muse's cut of block {at} touches no more than it has"
            );
            let (number, of, inner, this) = unframed(cut);
            assert_eq!(
                (number, of),
                (at + 1, blocks.len()),
                "block {at} says its place"
            );
            assert_eq!(*id.get_or_insert(this.clone()), this, "one role's id");
            role.push_str(inner);
        }
        role
    }

    /// A role of everything that has cost a cut a meaning: a fence of lines that end in two
    /// spaces, text indented under it, a line longer than three parts, two-, three- and
    /// four-byte characters, quotes and backslashes (two bytes each in the answer), tabs and
    /// carriage returns, and blank lines, each many times over, `shift` characters in.
    fn hard_role(shift: usize) -> String {
        let mut role = format!("{}\n", "s".repeat(shift));
        for section in 0..4 {
            role += &format!("## Section {section}\n\n");
            role += "```text\n";
            for line in 0..120 {
                role += &format!("rule {line} of the fence, and a trailing space pair  \n");
            }
            role += "```\n\n";
            for line in 0..60 {
                role += &format!("    indented {line}: code that Muse would cut the indent of\n");
            }
            role += &format!("\n{}\n\n", "a".repeat(40_000));
            for _ in 0..40 {
                role += "é é é é 日本語の行 😀 😀 \"quoted\" \\back\\slash\ttab\r\n\n";
            }
        }
        role
    }

    #[test]
    fn the_blocks_of_a_long_role_keep_it_to_the_byte_once_muse_cuts_their_ends_and_each_goes_in_a_hook(
    ) {
        for shift in 0..40 {
            let role = hard_role(shift);
            let blocks = blocks(&role);
            assert!(blocks.len() > 3, "{shift}: {} parts", blocks.len());
            assert!(
                blocks.len() <= MOST_PARTS,
                "{shift}: {} parts",
                blocks.len()
            );
            assert_eq!(kept(&blocks), role, "shift {shift}");
            for (at, block) in blocks.iter().enumerate() {
                let output = hook_output(block);
                assert!(
                    output.len() <= PART_OUTPUT && PART_OUTPUT < OUTPUT_LIMIT,
                    "shift {shift}, block {at}: {} bytes",
                    output.len()
                );
            }
        }
    }

    #[test]
    fn every_character_a_json_string_escapes_is_sized_as_serde_json_writes_it() {
        let overhead = hook_output("").len();
        for character in [
            'a', '"', '\\', '\n', '\r', '\t', '\u{8}', '\u{c}', '\u{1}', '\u{1f}', '\u{7f}', 'é',
            '日', '😀', '\u{2028}',
        ] {
            let text = character.to_string().repeat(9);
            assert_eq!(
                hook_output(&text).len(),
                overhead + 9 * encoded(character),
                "{character:?}"
            );
        }
        // The most escaped text the ledger takes for a staff's rules: 8,000 bytes of quotes.
        let quotes = "\"".repeat(8_000);
        assert_eq!(hook_output(&quotes).len(), overhead + 16_000);
    }

    #[test]
    fn a_role_that_fits_in_a_hook_is_one_block_with_no_frame_and_is_said_as_it_always_was() {
        let launch = launch();
        let role = "# ConsensFlow worker\n";
        assert_eq!(parts(role), [role]);
        assert_eq!(blocks(role), [role]);
        assert_eq!(parts(""), Vec::<&str>::new());
        assert!(blocks("").is_empty());
        assert_eq!(
            session_hook(
                &mut event("SessionStart", "s1").to_string().as_bytes(),
                Some(&launch.role),
                None,
                0
            )
            .as_deref(),
            Some(ROLE)
        );
        // The most that goes in one: the answer is at its budget, under Muse's limit.
        let room = PART_OUTPUT - hook_output("").len();
        let exact = "a".repeat(room);
        assert_eq!(parts(&exact), [exact.as_str()]);
        assert_eq!(hook_output(&exact).len(), PART_OUTPUT);
        let over = format!("{exact}a");
        let blocks = blocks(&over);
        assert_eq!(blocks.len(), 2, "one byte more is two framed parts");
        assert_eq!(kept(&blocks), over);
    }

    #[test]
    fn a_part_ends_at_a_line_start_outside_a_fence_where_one_fits_and_inside_one_where_none_does() {
        // A fence of 100 lines (about 3 KB) that a cut would fall inside, among prose.
        let prose = |lines: usize, tag: &str| {
            (0..lines)
                .map(|line| format!("{tag} line {line}: some words of the role\n"))
                .collect::<String>()
        };
        let fence = format!("```text\n{}```\n", prose(100, "fenced"));
        let role = format!("{}{fence}{}", prose(300, "before"), prose(600, "after"));
        let cut = parts(&role);
        assert_eq!(cut.concat(), role);
        assert!(cut.len() >= 2);
        let mut at = 0;
        for part in &cut[..cut.len() - 1] {
            at += part.len();
            let upto = &role[..at];
            let fences = upto.lines().filter(|line| line.starts_with("```")).count();
            assert!(
                fences % 2 == 0,
                "the cut at {at} is inside a fence: {}",
                &role[at..at + 20]
            );
            assert!(upto.ends_with('\n'), "a cut at a line start");
        }
        // A fence longer than a part is cut inside it, at line starts.
        let long = format!("```text\n{}```\n", prose(900, "long"));
        let cut = parts(&long);
        assert!(cut.len() >= 2 && cut.concat() == long);
        assert!(cut[..cut.len() - 1].iter().all(|part| part.ends_with('\n')));
        // A `~~~` fence is closed by `~~~`, and a ``` inside it does not close it.
        assert_eq!(fenced(None, "~~~\nx"), Some(('~', 3)));
        assert_eq!(fenced(Some(('~', 3)), "```\nx"), Some(('~', 3)));
        assert_eq!(fenced(Some(('~', 3)), "~~~~  \nx"), None);
        assert_eq!(
            fenced(Some(('`', 4)), "```\nx"),
            Some(('`', 4)),
            "a shorter run does not close"
        );
        assert_eq!(
            fenced(None, "    ```\nx"),
            None,
            "four spaces of indent is code, not a fence"
        );
        assert_eq!(fenced(None, "``\nx"), None);
    }

    #[test]
    fn a_line_longer_than_a_part_is_cut_in_whole_characters() {
        let long = "日".repeat(40_000);
        let blocks = blocks(&long);
        assert!(blocks.len() >= 8, "{}", blocks.len());
        assert_eq!(kept(&blocks), long);
    }

    #[test]
    fn a_hook_past_the_roles_last_part_says_it_has_none_and_only_the_first_writes_the_session_down()
    {
        let launch = launch();
        let role = hard_role(0);
        let blocks = blocks(&role);
        let last = blocks.len();
        let (id, said) = (id_of(&role), |part| {
            fs::write(&launch.role, &role).unwrap();
            let printed = session_hook(
                &mut event("SessionStart", "s1").to_string().as_bytes(),
                Some(&launch.role),
                Some(&launch.said),
                part,
            )
            .expect("a hook has a block to print");
            let said: Value = serde_json::from_str(&printed).expect("JSON");
            said["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .expect("its text")
                .to_owned()
        });
        for (at, block) in blocks.iter().enumerate().skip(1) {
            assert_eq!(&said(at), block);
        }
        assert!(
            !launch.said.exists(),
            "only the first writes the session down"
        );
        assert_eq!(said(0), blocks[0]);
        let over = said(last + 3);
        assert_eq!(over, filler(&id, last + 4, last));
        assert!(over.starts_with(MARK) && ours(&over) && !over.contains("part 1 of"));
        assert_eq!(fs::read_to_string(&launch.said).unwrap().lines().count(), 1);
        // No role text says nothing, whatever the block.
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
    fn a_block_is_a_role_of_ours_by_its_mark_or_by_the_front_matter_of_an_unframed_role() {
        let role = hard_role(0);
        assert!(blocks(&role).iter().all(|block| ours(block)));
        assert!(ours(&filler("0123abcd", 3, 2)));
        assert!(ours("---\nname: consensflow-chief\ndescription: x\n---\n"));
        assert!(!ours("the owner's own words"));
        assert!(
            !ours("# A skill\n---\nname: consensflow-chief"),
            "not at its start"
        );
        assert!(!ours(""));
        assert_eq!(id_of("a role").len(), 8);
        assert_ne!(id_of("a role"), id_of("a role."));
    }

    /// A log with the block records `blocks` say (number, text), each a `context_block_updated`
    /// of `hook:session_start:session:<number>` as Muse writes it, with others around them.
    fn log_with(dir: &Path, blocks: &[(usize, &str)]) -> std::path::PathBuf {
        let log = dir.join("session.jsonl");
        let mut lines = vec![
            r#"{"payload_type":"runtime.session","payload":{"kind":"run","event":{"kind":"context_block_diagnostic","block_id":"hook:session_start:session:9"}}}"#.to_owned(),
        ];
        for (number, text) in blocks {
            lines.push(
                json!({
                    "payload_type": "runtime.session",
                    "payload": { "kind": "run", "event": {
                        "kind": "context_block_updated",
                        "id": format!("hook:session_start:session:{number}"),
                        "text": text,
                    }},
                })
                .to_string(),
            );
            lines.push(
                r#"{"payload":{"kind":"run","event":{"kind":"assistant_message_committed"}}}"#
                    .to_owned(),
            );
        }
        fs::write(&log, lines.join("\n") + "\n").unwrap();
        log
    }

    const SOON: Duration = Duration::from_secs(30);

    #[test]
    fn the_blocks_of_ours_a_conversation_has_are_the_highest_block_whose_last_text_is_ours_and_no_owners(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let framed = blocks(&hard_role(0));
        let ours_at = |log: &[(usize, &str)]| cover_of(&log_with(dir.path(), log), SOON);
        assert_eq!(ours_at(&[]), Some(0), "no block");
        assert_eq!(
            ours_at(&[(0, "an owner's words")]),
            Some(0),
            "an owner's block is none of ours"
        );
        // Two parts of ours, and an owner's block above them that prints this launch.
        assert_eq!(
            ours_at(&[
                (0, &framed[0]),
                (1, &framed[1]),
                (2, "an owner's words"),
                (7, "more")
            ]),
            Some(2),
            "what is above ours is the owner's, and no part of the count"
        );
        // A filler is ours, and an unframed role is, at any number (a legacy launch).
        assert_eq!(
            ours_at(&[(0, &framed[0]), (4, &filler("00000000", 5, 1))]),
            Some(5)
        );
        assert_eq!(
            ours_at(&[(3, "---\nname: consensflow-worker\n---\nx")]),
            Some(4)
        );
        // The last text of a block counts: an owner's words written over ours.
        assert_eq!(
            ours_at(&[
                (0, &framed[0]),
                (1, &framed[1]),
                (1, "an owner's words now")
            ]),
            Some(1)
        );
        assert_eq!(
            ours_at(&[(1, "an owner's words"), (1, &framed[1]), (0, &framed[0])]),
            Some(2)
        );
    }

    #[test]
    fn a_log_that_cannot_be_counted_is_none_so_the_launch_covers_every_block_a_role_could_have() {
        let dir = tempfile::tempdir().unwrap();
        let framed = blocks(&hard_role(0));
        assert_eq!(
            cover_of(&dir.path().join("none.jsonl"), SOON),
            None,
            "no log"
        );
        assert_eq!(cover_of(dir.path(), SOON), None, "a folder is no log");
        let log = log_with(dir.path(), &[(0, &framed[0]), (1, &framed[1])]);
        assert_eq!(cover_of(&log, SOON), Some(2));
        assert_eq!(cover_of(&log, Duration::ZERO), None, "no time to count in");
        // Past the longest log it reads: a sparse file, nothing written.
        let huge = dir.path().join("huge.jsonl");
        let file = fs::File::create(&huge).unwrap();
        file.set_len(LONGEST_LOG + 1).unwrap();
        assert_eq!(cover_of(&huge, SOON), None);
        // At the longest it reads, it reads: sparse zeros are no line of a log.
        file.set_len(0).unwrap();
        // A log a person cannot read is none (not under a user who can read anything).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&log, fs::Permissions::from_mode(0o000)).unwrap();
            let unreadable = fs::File::open(&log).is_err();
            if unreadable {
                assert_eq!(cover_of(&log, SOON), None, "a log it cannot open");
            }
            fs::set_permissions(&log, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }

    #[test]
    fn the_log_is_counted_past_a_torn_last_line_and_a_line_too_long_to_hold() {
        let dir = tempfile::tempdir().unwrap();
        let framed = blocks(&hard_role(0));
        let log = log_with(dir.path(), &[(0, &framed[0]), (1, &framed[1])]);
        let mut text = fs::read_to_string(&log).unwrap();
        // A line many megabytes long that would be a block of ours at number 5 (a tool's
        // output that quotes one) is passed: no block of the hooks is that long.
        text += &json!({ "payload": { "kind": "run", "event": {
            "kind": "context_block_updated",
            "id": "hook:session_start:session:5",
            "text": format!("{MARK} 00000000 part 6{}", "z".repeat(LONGEST_LINE + 10)) }}})
        .to_string();
        text += "\n";
        text += "{\"payload\":{\"event\":{\"kind\":\"context_block_updated\",\"id\":\"hook:session_start:session:3\",\"text\":\"<<consensflow-r";
        fs::write(&log, text).unwrap();
        assert_eq!(
            cover_of(&log, SOON),
            Some(2),
            "neither the long line nor the torn one is a block"
        );
    }

    /// A log of about `megabytes` MiB: lines like a long session's (a model request's
    /// bookkeeping of a few KB, a tool's result of a few hundred KB now and then, a
    /// diagnostic that names a block of the hooks, as every request writes for each), and the
    /// blocks of ours (`framed`, one of them first, and one near each boundary of the
    /// reader's buffer) among them. Written to `log`.
    fn long_log(log: &Path, megabytes: usize, framed: &[String]) {
        let mut out = std::io::BufWriter::new(fs::File::create(log).unwrap());
        let record = |number: usize, text: &str| {
            json!({ "payload": { "kind": "run", "event": {
                "kind": "context_block_updated",
                "id": format!("hook:session_start:session:{number}"),
                "text": text }}})
            .to_string()
        };
        let bookkeeping = format!(
            "{{\"payload_type\":\"runtime.session\",\"payload\":{{\"kind\":\"run\",\"event\":{{\"kind\":\"model_request_configured\",\"toolset\":\"{}\"}}}}}}\n",
            "t".repeat(3_000)
        );
        let diagnostic = "{\"payload\":{\"kind\":\"run\",\"event\":{\"kind\":\"context_block_diagnostic\",\"block_id\":\"hook:session_start:session:1\",\"message\":\"context block observed\"}}}\n";
        let result = format!("{{\"payload\":{{\"kind\":\"run\",\"event\":{{\"kind\":\"tool_result\",\"output\":\"{}\"}}}}}}\n", "r".repeat(300_000));
        for (number, text) in framed.iter().enumerate() {
            writeln!(out, "{}", record(number, text)).unwrap();
        }
        let target = megabytes * 1024 * 1024;
        let (mut written, mut turn) = (0, 0);
        while written < target {
            let line: &str = match turn % 40 {
                0 => &result,
                n if n % 3 == 0 => diagnostic,
                _ => &bookkeeping,
            };
            out.write_all(line.as_bytes()).unwrap();
            written += line.len();
            turn += 1;
        }
        out.flush().unwrap();
    }

    #[test]
    fn a_log_of_many_megabytes_is_counted_across_the_boundaries_of_its_buffer() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("session.jsonl");
        let framed = blocks(&hard_role(0));
        long_log(&log, 20, &framed);
        assert_eq!(cover_of(&log, SOON), Some(framed.len()));
        // A block whose line begins just before a boundary of the 1 MiB buffer and ends past it.
        let mut text = fs::read_to_string(&log).unwrap();
        let line = |number: usize| {
            json!({ "payload": { "kind": "run", "event": {
                "kind": "context_block_updated",
                "id": format!("hook:session_start:session:{number}"),
                "text": framed[0] }}})
            .to_string()
                + "\n"
        };
        // Pad with lines of one byte and two to a place just before a boundary of the buffer.
        let boundary = text.len() / (1024 * 1024) * (1024 * 1024) + 1024 * 1024;
        let padding = boundary - 40 - text.len();
        text += &"x\n".repeat(padding / 2);
        if padding % 2 == 1 {
            text += "\n";
        }
        assert_eq!(text.len(), boundary - 40);
        let above = framed.len() + 4;
        text += &line(above);
        fs::write(&log, text).unwrap();
        assert_eq!(cover_of(&log, SOON), Some(above + 1));
    }

    /// A timing, not a check: how long the count of a long session's log takes at a resume
    /// launch (T-1246's review). `CF_COVER_LOG=<a real session.jsonl>` repeats a real log;
    /// else a synthetic one of `CF_COVER_MIB` MiB (512) is made.
    ///
    /// `cargo test -p cf-harness --lib muse::session_hook::tests::timing -- --ignored --nocapture`
    #[test]
    #[ignore = "a timing: run by hand with --ignored --nocapture"]
    #[allow(clippy::disallowed_methods)] // The knobs of a timing run by hand, read where it is.
    fn timing_of_the_count_on_a_long_log() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("session.jsonl");
        let framed = blocks(&hard_role(0));
        let mib: usize = std::env::var("CF_COVER_MIB")
            .ok()
            .and_then(|mib| mib.parse().ok())
            .unwrap_or(512);
        match std::env::var("CF_COVER_LOG") {
            Ok(real) => {
                let one = fs::read(&real).unwrap();
                let mut out = std::io::BufWriter::new(fs::File::create(&log).unwrap());
                let mut written = 0;
                while written < mib * 1024 * 1024 {
                    out.write_all(&one).unwrap();
                    written += one.len();
                }
                out.flush().unwrap();
            }
            Err(_) => long_log(&log, mib, &framed),
        }
        let size = fs::metadata(&log).unwrap().len();
        for pass in ["first (the file just written, in the cache)", "second"] {
            let began = Instant::now();
            let found = cover_of(&log, Duration::from_secs(60));
            println!(
                "TIMING {} MiB, {pass}: {found:?} in {} ms",
                size / 1024 / 1024,
                began.elapsed().as_millis()
            );
        }
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
