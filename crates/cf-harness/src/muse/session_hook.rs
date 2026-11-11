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
//! **A conversation keeps the blocks its earlier launches wrote, and a window can be put on any
//! conversation** (a human's `/resume` inside it runs this window's hooks on a conversation
//! given a longer role by an earlier launch, or an older ConsensFlow). A role that now needs
//! fewer parts would leave the tail of the old one where the model may act on it. So **every
//! launch gives [`MOST_PARTS`] hooks, whatever its role needs**: the hook past the role's last
//! part writes [`FILLER`] over its block, and no block a ConsensFlow role could have left
//! stays, whatever the conversation, the owner's hooks or the launch before. (A count from
//! the session's log at the launch was tried first and was not enough: it knew the
//! conversation the window began with, not the one it was put on.) A filler is 27 bytes of
//! text, and a block's own cost in the context is a few tens of bytes more: measured at the
//! launch in `muse_role.rs`.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

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

/// The most parts a role may be given in, and the hooks every launch gives (a hook for each
/// part and a filler past them, so no block a role could have left stays): a role that needs
/// more is refused at its launch in words, not started with none of it. Muse takes more (20 hooks
/// of 16,000 bytes and 64 small ones were all taken, T-1246), so this is a choice of cost: each
/// hook costs about 45 ms of a start on a Mac and 20 ms on Windows and 30 bytes of context
/// (measured, `muse_role.rs`), and 8 is 2.7 times the 3 parts of the largest role there is
/// (37,760 bytes of answer; the pins keep every role at half of this or fewer).
pub const MOST_PARTS: usize = 8;

/// What every block of ours begins with, a part's frame and the filler alike: how a block of a
/// session's log is known for a ConsensFlow role's, which no hook of the owner's says.
pub const MARK: &str = "<<consensflow-role";

/// What a hook says over the block of a part its role does not have: an older, longer role's
/// may have left one in a conversation. Short, and the same for every block: 27 bytes.
pub const FILLER: &str = "<<consensflow-role: empty>>";

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

/// The bytes a frame takes at the most in a hook's answer: a first part's head and a last
/// part's tail of the most parts, and the two line breaks that join them to the part, which a
/// JSON string writes in two bytes each.
fn frame_most() -> usize {
    let id = "0".repeat(8);
    head(&id, 1, MOST_PARTS).len() + tail(&id, MOST_PARTS, MOST_PARTS).len() + 2 * encoded('\n')
}

/// The parts `role` is cut into, each to be said by a hook: consecutive pieces of it
/// that put together are the role, each of which, framed ([`blocks`]) and put in a hook's
/// answer ([`hook_output`]), is at most 16,000 bytes. A role that fits in one hook as it
/// is is one part, with no frame. A part ends where a paragraph or a heading begins
/// ([`begins_a_block`]) after a blank line, outside a code fence (a line that begins with
/// three backticks or tildes opens one, and closes it again), and not after a heading or a
/// line that ends in a colon, which introduce what follows; of those the last that leaves the
/// part at least half its room. Where there is none (no paragraph or heading begins in the
/// part's second half) it ends at the last line start of its room that is outside a fence, else at
/// the last line start of it, and in the middle of a line (never of a character) only where a line
/// is longer than a part. So a part does not end between a heading or a line and the paragraph,
/// list, table or example it introduces, and the next begins with a sentence or a heading,
/// **except** where no paragraph or heading begins in the part's second half: then the fallback
/// to a line start can split a list, whether the list is longer than half a frame or shorter
/// (the project's rules for its staff, 8,000 bytes at the most, can be 79 list items of escaped
/// JSON; a real project's are not, and the 16 cuts of the texts measured for T-1435 are all at a
/// paragraph or a heading). No role is no part.
pub fn parts(role: &str) -> Vec<&str> {
    if role.is_empty() {
        return Vec::new();
    }
    if hook_output(role).len() <= PART_OUTPUT {
        return vec![role];
    }
    cut(role, PART_OUTPUT - hook_output("").len() - frame_most())
}

/// Whether a part may end before `line`, a whole line: it has text, is not indented (the
/// rest of an item or an example), is not a list item (`- `, `* `, `+ `, `1. `, `1) `), a
/// table row (`|`), a quotation (`>`) or the opening of a code fence, which are lines that
/// go on with what is before them.
fn begins_a_block(line: &str) -> bool {
    let Some(first) = line.chars().next() else {
        return false;
    };
    if first.is_whitespace() || matches!(first, '|' | '>') || fenced(None, line).is_some() {
        return false;
    }
    let marker =
        |rest: Option<&str>| rest.is_some_and(|rest| rest.is_empty() || rest.starts_with(' '));
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    let item = ['-', '*', '+']
        .into_iter()
        .any(|bullet| marker(line.strip_prefix(bullet)))
        || (digits > 0 && marker(line[digits..].strip_prefix(['.', ')'])));
    !item
}

/// `role` cut in pieces each of which takes at most `room` bytes of a JSON string.
fn cut(role: &str, room: usize) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut used) = (0, 0);
    // The last line start of the part so far that begins a block (with the bytes of the part
    // before it), outside a fence, and anywhere.
    let mut block: Option<(usize, usize)> = None;
    let (mut outside, mut any): (Option<usize>, Option<usize>) = (None, None);
    let mut fence: Option<(char, usize)> = None;
    // Whether the line before is blank, and whether the last line with text introduces what
    // follows: it is a heading or ends in a colon.
    let (mut blank_before, mut introduces) = (false, false);
    let mut line_starts = true;
    for (at, character) in role.char_indices() {
        if line_starts {
            let line = role[at..].split('\n').next().unwrap_or("");
            if at > start {
                any = Some(at);
                if fence.is_none() {
                    outside = Some(at);
                    if blank_before && !introduces && begins_a_block(line) {
                        block = Some((at, used));
                    }
                }
            }
            fence = fenced(fence, &role[at..]);
            blank_before = line.trim().is_empty();
            if !blank_before {
                let text = line.trim_end();
                introduces = text.ends_with(':') || text.starts_with('#');
            }
        }
        let cost = encoded(character);
        if used + cost > room {
            let whole = block
                .filter(|&(_, before)| before >= room / 2)
                .map(|(line, _)| line);
            let end = whole.or(outside).or(any).unwrap_or(at);
            parts.push(&role[start..end]);
            start = end;
            used = role[end..at].chars().map(encoded).sum();
            block = None;
            outside = outside.filter(|&line| line > end);
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
/// keeps is the text alone. A number past the role's last prints [`FILLER`].
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
    let block = blocks.get(part).map_or(FILLER, String::as_str);
    Some(hook_output(block))
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
        role: std::path::PathBuf,
        said: std::path::PathBuf,
        _dir: tempfile::TempDir,
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
    /// 16,384 bytes, a part is given 16,000, and 20 hooks of one start were all taken (8 are given). A Muse
    /// that changes them is the cases' to fail, and these to be changed with them.
    #[test]
    fn the_numbers_are_those_measured_on_muse_1_4_14() {
        assert_eq!(OUTPUT_LIMIT, 16_384);
        assert_eq!(PART_OUTPUT, 16_000);
        assert_eq!(MOST_PARTS, 8);
        assert_eq!(hook_output("").len(), 78, "the wrapper of a JSON answer");
        assert!(
            frame_most() < 400,
            "a frame is lines, not a page: {}",
            frame_most()
        );
    }

    #[test]
    fn a_roles_id_is_a_digest_of_its_whole_text_so_another_role_has_another_and_a_part_says_it() {
        assert_eq!(id_of("a role").len(), 8);
        assert_ne!(id_of("a role"), id_of("a role."));
        let (one, other) = (hard_role(0), hard_role(1));
        let heads = |role: &str| {
            blocks(role)
                .iter()
                .map(|block| unframed(block).3)
                .collect::<Vec<_>>()
        };
        let (first, second) = (heads(&one), heads(&other));
        assert!(first.iter().all(|id| *id == id_of(&one)), "{first:?}");
        assert!(second.iter().all(|id| *id == id_of(&other)), "{second:?}");
        assert_ne!(first[0], second[0], "two roles, two ids");
    }

    #[test]
    fn the_room_a_frame_takes_is_at_least_what_any_frame_takes_in_an_answer() {
        let id = "0123abcd";
        for of in 1..=MOST_PARTS {
            for part in 1..=of {
                let frame =
                    hook_output(&format!("{}\n\n{}", head(id, part, of), tail(id, part, of))).len()
                        - hook_output("").len();
                assert!(
                    frame <= frame_most(),
                    "part {part} of {of}: {frame} bytes of a frame, room for {}",
                    frame_most()
                );
            }
        }
        // And so the fullest part of the hardest role is under the budget, to the byte.
        let role = format!("{}\n", "a".repeat(98)).repeat(2_000);
        for block in blocks(&role) {
            assert!(hook_output(&block).len() <= PART_OUTPUT);
        }
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
    /// spaces, text indented under it, a line longer than a part and a half, two-, three- and
    /// four-byte characters, quotes and backslashes (two bytes each in the answer), tabs and
    /// carriage returns, and blank lines, each many times over, `shift` characters in.
    fn hard_role(shift: usize) -> String {
        let mut role = format!("{}\n", "s".repeat(shift));
        for section in 0..2 {
            role += &format!("## Section {section}\n\n");
            role += "```text\n";
            for line in 0..120 {
                role += &format!("rule {line} of the fence, and a trailing space pair  \n");
            }
            role += "```\n\n";
            for line in 0..60 {
                role += &format!("    indented {line}: code that Muse would cut the indent of\n");
            }
            role += &format!("\n{}\n\n", "a".repeat(30_000));
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

    /// Whether the cut that begins a part at `at` of `role` is the one a part is given when a
    /// paragraph or a heading is there to begin it: after a blank line, at a line of the
    /// role under test that is a heading or a paragraph (it begins with `#` or a letter: its
    /// list items, table rows, quotations, numbered items, fences and indented lines do not),
    /// not after a heading or a line that ends in a colon, and outside a fence. It does not ask
    /// [`begins_a_block`], which it checks.
    fn at_a_block(role: &str, at: usize) -> Result<(), String> {
        let before = &role[..at];
        let line = role[at..].split('\n').next().unwrap_or("");
        if !before.ends_with("\n\n") {
            return Err(format!("no blank line before {line:?}"));
        }
        if !line.starts_with(|first: char| first == '#' || first.is_alphabetic()) {
            return Err(format!("{line:?} goes on with what is before it"));
        }
        let last = before.trim_end().lines().last().unwrap_or("");
        if last.ends_with(':') || last.starts_with('#') {
            return Err(format!("{last:?} introduces {line:?}"));
        }
        let fences = before
            .lines()
            .filter(|line| line.starts_with("```"))
            .count();
        if fences % 2 != 0 {
            return Err(format!("{line:?} is inside a fence"));
        }
        Ok(())
    }

    #[test]
    fn a_line_does_not_end_a_part_before_the_list_it_introduces_where_a_block_begins_in_the_room() {
        // A paragraph of 40 bytes, a line that introduces a list of three, a paragraph. The room
        // ends inside the list: the part ends before the line that introduces it (42), where it
        // used to end between two items of the list (65).
        let role = format!(
            "{}\n\nThe tiers:\n- one\n- two\n- three\n\nThen the next paragraph.\n",
            "p".repeat(40)
        );
        assert_eq!(cut(&role, 70), [&role[..42], &role[42..]]);
        // A room of the first half of it or less is not given up: a part is not made shorter
        // than half its room for a block's sake (the parts of a role stay few), and ends at the
        // last line start, as before.
        let short = format!(
            "{}\n\nThe tiers:\n- one\n- two\n- three\n- four\n- five\n- six\n- seven\n",
            "p".repeat(10)
        );
        let at = short.find("- six").unwrap();
        assert_eq!(cut(&short, 68), [&short[..at], &short[at..]]);
        // A paragraph after a line that ends in a colon is that line's, blank line or not.
        let colon = format!(
            "{}\n\nAs follows:\n\nthe paragraph it introduces, which\ncontinues here\n",
            "p".repeat(40)
        );
        let cuts = cut(&colon, 80);
        assert_eq!(cuts.concat(), colon);
        assert_eq!(cuts[0], &colon[..42], "before the line that introduces");
        // A blank line inside a fence, and a line that looks like a paragraph after it, is
        // no place to end a part; the fence is kept whole where the room has the room.
        let fence = format!(
            "{}\n\n```text\nline one\n\nlooks like a paragraph\n```\n\nafter the fence\n",
            "p".repeat(40)
        );
        let at = fence.find("```text").unwrap();
        assert_eq!(cut(&fence, 78), [&fence[..at], &fence[at..]]);
    }

    #[test]
    fn a_heading_is_not_left_at_the_end_of_a_part_from_the_paragraph_under_it() {
        // The room ends inside the paragraph under a heading: the part ends before the heading
        // (42), not between the heading and its paragraph (56).
        let role = format!(
            "{}\n\n## Heading\n\nA paragraph that goes on and on and on and on.\n",
            "p".repeat(40)
        );
        assert_eq!(cut(&role, 70), [&role[..42], &role[42..]]);
        // And a heading after a heading, with nothing between, is no place either: the first
        // would be left alone at the end of a part.
        let two = format!(
            "{}\n\n## One\n\n## Two\n\nA paragraph that goes on and on and on and on.\n",
            "p".repeat(40)
        );
        let at = two.find("## One").unwrap();
        assert_eq!(cut(&two, 76), [&two[..at], &two[at..]]);
    }

    #[test]
    fn every_part_of_a_role_of_blocks_ends_before_a_paragraph_or_a_heading() {
        // Blocks of every kind a role holds, each shorter than half the smallest room: a
        // paragraph, a heading, a line that introduces a list and its items (one with a line
        // of its own indented), a table, a quotation, a numbered list, a fence with a blank
        // line in it and a line after that looks like a paragraph.
        let mut role = String::new();
        for at in 0..24 {
            role += &format!("## Heading {at}\n\n");
            role += &format!("A paragraph of the role, number {at}, of one line.\n\n");
            role +=
                &format!("The list {at}:\n- first item\n  and its second line\n- second item\n\n");
            role += "| a | b |\n|---|---|\n| 1 | 2 |\n\n";
            role += &format!("> a quotation {at}\n\n");
            role += "1. one\n2. two\n\n";
            role += "```text\nrule\n\nlooks like a paragraph\n```\n\n";
            role += &format!("A last paragraph {at}.\nTwo lines of it.\n\n");
        }
        let mut cuts = 0;
        for room in 400..700 {
            let parts = cut(&role, room);
            assert_eq!(parts.concat(), role, "room {room}");
            let mut at = 0;
            for part in &parts[..parts.len() - 1] {
                at += part.len();
                cuts += 1;
                if let Err(why) = at_a_block(&role, at) {
                    panic!("room {room}, the cut at {at}: {why}");
                }
                assert!(
                    part.chars().map(encoded).sum::<usize>() >= room / 2,
                    "room {room}: a part of less than half the room"
                );
            }
        }
        assert!(cuts > 2_000, "{cuts} cuts were looked at");
    }

    #[test]
    fn the_lines_that_go_on_with_what_is_before_them_do_not_begin_a_block() {
        for goes_on in [
            "",
            "  indented",
            "\tindented",
            "- item",
            "* item",
            "+ item",
            "-",
            "1. item",
            "12) item",
            "7.",
            "| row |",
            "> quoted",
            "```text",
            "~~~",
            "   ```",
        ] {
            assert!(!begins_a_block(goes_on), "{goes_on:?}");
        }
        for begins in [
            "## Heading",
            "# Title",
            "A paragraph.",
            "The tiers:",
            "-not an item",
            "1.5 is a number",
            "2026 was the year",
            "**bold** text",
            "<<consensflow-role: empty>>",
            "日本語",
        ] {
            assert!(begins_a_block(begins), "{begins:?}");
        }
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
        let said = |part| {
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
        };
        for (at, block) in blocks.iter().enumerate().skip(1) {
            assert_eq!(&said(at), block);
        }
        assert!(
            !launch.said.exists(),
            "only the first writes the session down"
        );
        assert_eq!(said(0), blocks[0]);
        // Every hook past the role's parts, up to the last there is, says the same short text.
        for part in [last, last + 3, MOST_PARTS - 1] {
            assert_eq!(said(part), FILLER, "block {part}");
        }
        assert!(FILLER.starts_with(MARK) && FILLER.len() < 40, "{FILLER}");
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
