//! How a message reads in its recipient's pane. Its header doubles as the proof
//! that it arrived: a delivery counts once the window's own record shows the
//! header's start ([`marker_of`]), so the two are written together. A message
//! that carries others (what its window kept for it) reads as one paste: their
//! words and its own, in the order of their ids, under the one header, which is
//! the one marker that proves the paste arrived.
//!
//! Every command such a text teaches starts with the `cf` of the window it is for, by its
//! whole path ([`CfWord`]): a shell that reads the user's profile can find another `cf`
//! first (T-972), and a member told to run `cf answer` would run it. The footers and the
//! commands that read what is cut are written with it; what ConsensFlow itself wrote of a
//! message's words, a note's (`sender` none: the engine's and the ledger's own, whose words
//! the board shows as they are, bare) has each command it names by the word `cf` started by
//! it instead ([`CfWord::in_text`]). What a participant wrote is never changed; the words the
//! human gives the page to resume a task, which the ledger writes with no sender, are read as
//! ConsensFlow's own, and a command in them is no worse for starting with the window's `cf`.

use std::borrow::Cow;

use cf_base::js;
use cf_base::text::{utf16_len, window_text};
use cf_proto::cf_word::CfWord;
use cf_proto::ledger::MessageView;

/// A body longer than this, counted in UTF-16 code units as JavaScript counted
/// it, goes as its opening and the command that reads the rest. Measured with
/// npm run live:paste --long (2026-10-03): Claude and Devin take 16,000
/// characters whole on macOS and Windows; at 32,000, Devin on Windows was still
/// taking the paste when its Enter came. 4,000, the first guess, cut every long
/// brief.
const INLINE_LIMIT: usize = 16_000;
const OPENING: usize = 15_000;

/// How a message reads in the recipient's pane, and the rows it `carried`
/// with it, still waiting when its delivery began (a message that carries
/// none reads as it always did). The header doubles as the arrival marker.
///
/// A cut that falls inside a surrogate pair leaves half of it in Node, which
/// `windowText` drops before the text reaches a window. Here the half is
/// dropped at the cut, so what a window is given is the same.
pub fn delivery_text(message: &MessageView, carried: &[MessageView], cf: &CfWord) -> String {
    text_of(message, carried, cf, "", None)
}

/// What a header says, after who the message is from, of a message a start gave back
/// though its window's harness said the window took it (`in_flight`): it may be in the
/// conversation already, and reads again as one the reader may have read.
pub const SENT_AGAIN: &str = " · sent again after a restart, it may be there already";

/// [`delivery_text`] of a message sent again after a restart ([`SENT_AGAIN`]): the
/// word is in its header, after its marker, so its record proves it as any paste's.
pub fn delivery_text_sent_again(
    message: &MessageView,
    carried: &[MessageView],
    cf: &CfWord,
) -> String {
    delivery_text_beside(message, carried, cf, true, None)
}

/// [`delivery_text`] of a result whose task's requester was given, or is to be given, the
/// answer of its window's turn on a tell in the note `told_in` (`cf-ledger`'s
/// `paused_answers`): its footer says that this result is the one to decide (T-958). Sent
/// again after a restart where `again`.
pub fn delivery_text_beside(
    message: &MessageView,
    carried: &[MessageView],
    cf: &CfWord,
    again: bool,
    told_in: Option<i64>,
) -> String {
    text_of(
        message,
        carried,
        cf,
        if again { SENT_AGAIN } else { "" },
        told_in,
    )
}

/// [`delivery_text`], `again` said at the end of its header, and `told_in` in its footer.
fn text_of(
    message: &MessageView,
    carried: &[MessageView],
    cf: &CfWord,
    again: &str,
    told_in: Option<i64>,
) -> String {
    let id = message.id;
    let from = from_of(message);
    // A chain's notes are about several tasks: its header names none.
    let task = message
        .task_number
        .filter(|_| !is_chain(message, carried))
        .map_or_else(String::new, |number| format!(" · T-{number}"));
    let (whole, read_all) = words_of(message, carried, cf);
    let length = utf16_len(&whole);
    let body = if length <= INLINE_LIMIT {
        whole
    } else {
        Cow::Owned(format!(
            "{}\n… ({length} characters; read all of it with: {read_all})",
            opening(&whole)
        ))
    };
    format!(
        "[ConsensFlow m-{id}{task} · {} from {from}{again}]\n{body}{}",
        message.kind,
        footer(message, cf, told_in)
    )
}

/// The words a message has whole, and the command that reads them where they are
/// cut: its own body, or, where it carries rows, theirs and its own together.
fn words_of<'a>(
    message: &'a MessageView,
    carried: &[MessageView],
    cf: &CfWord,
) -> (Cow<'a, str>, String) {
    let id = message.id;
    if is_chain(message, carried) {
        return (
            Cow::Owned(chain(message, carried, cf)),
            format!("{cf} inbox"),
        );
    }
    match message.task_number.filter(|_| !carried.is_empty()) {
        None => (body_of(message, cf), format!("{cf} inbox read m-{id}")),
        Some(number) => (
            Cow::Owned(composite(message, carried, cf)),
            format!("{cf} task get T-{number}"),
        ),
    }
}

/// Whether `message` is the first note of a chain (RS-25): a note of ConsensFlow's own that
/// carries others, which only a chain's note does (`cf-ledger`'s `gather_chain`).
fn is_chain(message: &MessageView, carried: &[MessageView]) -> bool {
    message.kind == "note" && message.sender.is_none() && !carried.is_empty()
}

/// What a chain's paste says: how many notes it holds, and each, in the order written, on a
/// line of its own that begins with its id and its task; a note's further lines are indented
/// under it.
fn chain(first: &MessageView, carried: &[MessageView], cf: &CfWord) -> String {
    let mut notes: Vec<&MessageView> = [first].into_iter().chain(carried).collect();
    notes.sort_by_key(|note| note.id);
    let lines: Vec<String> = notes
        .iter()
        .map(|note| {
            let task = note
                .task_number
                .map_or_else(String::new, |number| format!(" · T-{number}"));
            let body = body_of(note, cf).replace('\n', "\n  ");
            format!("m-{}{task}: {body}", note.id)
        })
        .collect();
    format!(
        "{} notes at once, a line each:\n{}",
        notes.len(),
        lines.join("\n")
    )
}

/// The words of `row`, its commands started by `cf` where ConsensFlow wrote them
/// (`sender` none), as they are where a participant did.
fn body_of<'a>(row: &'a MessageView, cf: &CfWord) -> Cow<'a, str> {
    if row.sender.is_none() {
        cf.in_text(&row.body)
    } else {
        Cow::Borrowed(row.body.as_str())
    }
}

/// Whether a message goes into its recipient's pane in whole: its words are no
/// longer than a paste is given, and are not cut to their opening and the command
/// that reads the rest.
pub fn goes_whole(message: &MessageView, carried: &[MessageView], cf: &CfWord) -> bool {
    fits_whole(&words_of(message, carried, cf).0)
}

/// Whether `words`, a message's own, go into a pane whole: no longer than a
/// paste is given.
pub fn fits_whole(words: &str) -> bool {
    utf16_len(words) <= INLINE_LIMIT
}

/// Who a message is from: its sender's handle, or ConsensFlow itself.
fn from_of(message: &MessageView) -> String {
    message
        .sender
        .as_ref()
        .map_or_else(|| "ConsensFlow".to_owned(), |sender| format!("@{sender}"))
}

/// What a carrier's paste says: its own words and the rows it carries, in the
/// order of their ids, a blank line between. The carrier's words are its
/// own; each row it carries is under a line that says what it is and whose.
/// None of them holds a header, so only the carrier's marker is in the paste.
fn composite(carrier: &MessageView, carried: &[MessageView], cf: &CfWord) -> String {
    let mut rows: Vec<&MessageView> = carried.iter().chain([carrier]).collect();
    rows.sort_by_key(|row| row.id);
    rows.iter()
        .map(|row| {
            if row.id == carrier.id {
                body_of(row, cf).into_owned()
            } else {
                format!("{}\n{}", label(row), body_of(row, cf))
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The line over a row a paste carries: what it is, and whose, and for an
/// answer, which of the reader's questions it is for.
fn label(row: &MessageView) -> String {
    let (id, from) = (row.id, from_of(row));
    match (row.kind.as_str(), row.reply_to) {
        ("answer", Some(question)) => {
            format!("(kept for you: answer m-{id} from {from}, to your m-{question})")
        }
        (kind, _) => format!("(kept for you: {kind} m-{id} from {from})"),
    }
}

/// Where every message's header begins, whatever the message is.
pub(crate) const HEADER_START: &str = "[ConsensFlow m-";

/// The start of a message's header, as a window's record shows it once the
/// message arrived. The space after the id keeps m-1 from matching m-12; what
/// follows it is not part of the marker, since a window may not keep the ·
/// (Devin on Windows takes it as |, see `console_text`).
pub fn marker_of(message_id: i64) -> String {
    format!("{HEADER_START}{message_id} ")
}

/// What a member's fresh window is given ahead of its first message where that
/// is not its task's brief (a reopening, an answer): the brief it received, as
/// its paste said it, and a blank line (`launch_text`).
pub fn brief_first(brief: &MessageView, carried: &[MessageView], cf: &CfWord) -> String {
    format!("{}\n\n", delivery_text(brief, carried, cf))
}

/// The tag Claude Code wraps a paste in, in its record (2.1.296 on 2026-10-16, and
/// the eval of 2026-10-08): `<pasted_content id="b118">` on a line of its own, the
/// paste, and the same tag closed after it, `</pasted_content id="b118">`.
const PASTED: &str = "pasted_content";

/// What Claude Code puts ahead of a paste whose lines are paths of images it can read
/// (2.1.308; the field bug of 2026-10-21 in 2.1.306 and the live probe of 2026-10-22,
/// `tests/live_image_paste.rs`): a placeholder for each image, `[Image #4]`, one space
/// between them, and the paste after them with those lines gone, at once
/// (`[Image #4][ConsensFlow m-…]`) or, where Claude wrapped it ([`PASTED`]), after a blank
/// line. The attachments' sources are the next item of the record, which holds no header.
const IMAGE: &str = "[Image #";

/// The item a message taken back to be given again must come after to have arrived: the one
/// its window's record held the copy that never began at (its receipt's `before`, written by
/// a stall's relaunch). None for any other message.
pub(crate) fn arrival_boundary(message: &MessageView) -> Option<&str> {
    message
        .receipt
        .get("before")
        .and_then(serde_json::Value::as_str)
}

/// The first of `rows`, in their order, that is the paste of a message (`is_paste`) and comes
/// after its boundary (`before`, [`arrival_boundary`]) where `rows` hold that item (`id`);
/// where they do not (a conversation opened afresh), any. The one rule every reader of an
/// arrival keeps, the window's look, the record a switched window left, ConsensFlow's copy
/// of the conversation and its harness's own record (a restart, a window's end, a Close):
/// the copy that never began proves nothing of the delivery after it (T-1223's P2-2,
/// T-1229's P2), and nor would the last copy that matches.
pub(crate) fn arrived_among<'a, T>(
    rows: &'a [T],
    before: Option<&str>,
    id: impl Fn(&T) -> &str,
    is_paste: impl Fn(&T) -> bool,
) -> Option<&'a T> {
    let from = before
        .and_then(|before| rows.iter().rposition(|row| id(row) == before))
        .map_or(0, |at| at + 1);
    rows[from..].iter().find(|row| is_paste(row))
}

/// Whether `text`, an item of what a window was given, is the paste of the
/// message `message_id`, which proves it arrived: the message's own header begins
/// it, as ConsensFlow pastes it, the lines of the window's text
/// (`window_text`). So it does where the record holds the paste as its harness
/// writes one:
/// - inside Claude Code's wrapper ([`PASTED`]), the tag opened with its attributes
///   on a line of its own and closed with the same after the header (the reviews of
///   2026-10-16: a Claude chief's record of every result was taken for lacking, and
///   the result pasted again), at the end or not: a paste Claude was given in pieces
///   has them each wrapped and its last, short piece bare after the last closing tag
///   (T-763); the four others record the paste as it is given;
/// - behind the placeholders of the images Claude Code attached ([`IMAGE`]: a result
///   that starts with the path of a picture, the field bug of 2026-10-21, pasted three
///   times and failed), the header at once, or a blank line and then the wrapper (the
///   shape Claude writes; the header after the blank line with no wrapper is taken for
///   the paste too, as a wrapper is read where it is and none is needed, a shape Claude
///   does not write and no wider than the limit below);
/// - after `first`, the paste a member's fresh window is given ahead of its first
///   message ([`brief_first`]), there and nowhere else: as it was given, but for what
///   Claude Code takes out of a paste it attaches pictures of. The brief is pasted with
///   the message (T-681), so a picture path that stands on a line of its own in the
///   brief is taken out of the record as it would be of any paste's, and so are the
///   blank lines of the whole paste (the paste handler of 2.1.314 to 2.1.317, read in the
///   binary: it drops a line with nothing in it, and keeps whole a line of words that has
///   a path in it) (T-812, Calliope's P3-1 of T-803). The brief is then matched line by
///   line, and a line of it may be missing, if it is blank or ends with the name of an
///   image file ([`names_an_image`], once for each placeholder the item begins with).
///   A line the record keeps (a picture Claude could not read, a file that is no image)
///   is matched where it is, and a line of words is never missing.
///
/// A header quoted in other words, the human's (`Why is "[ConsensFlow m-42 …]"
/// queued?`) or a message's (`Investigate this:\n\n[ConsensFlow m-42 …]`), is no
/// paste, and proves nothing arrived (the reviews of 2026-10-16), whether words come
/// before it or only placeholders do. Limits: words that begin with a header as a paste
/// does are taken for one, and a paste Devin glued behind words the human typed and never
/// sent (it pastes behind what its input box holds, a record of 2026-10-02) is none: the
/// window waits for those words to be sent first (`Held::Unsent`), so only a typing
/// between that look and the paste can make one. Limit: the item must begin with the
/// placeholders for any line of the brief to be missing; a record of an attachment that
/// leaves none is not read.
pub fn is_paste_of(text: &str, message_id: i64, first: Option<&str>) -> bool {
    let marker = marker_of(message_id);
    if !text.contains(&marker) {
        return false;
    }
    let text = window_text(text);
    let text = text.trim_start();
    // Behind the placeholders of the images Claude attached, the header at once or a blank
    // line and then the wrapper (or the header itself, which no wrapper is needed for).
    let paste = match after_placeholders(text) {
        Some(head) => head.strip_prefix("\n\n").map_or(head, unwrapped),
        None => unwrapped(text),
    };
    // One placeholder for each picture Claude took out of the paste.
    let attached = after_placeholders(text).map_or(0, |head| {
        text[..text.len() - head.len()].matches(IMAGE).count()
    });
    let behind = first.and_then(|first| after_brief(paste, &window_text(first), attached));
    paste.starts_with(&marker) || behind.is_some_and(|rest| rest.starts_with(&marker))
}

/// What follows the brief `first` a fresh window was given ahead of its first message, in
/// `paste`, as its record shows it: the brief line by line, but for the lines Claude Code
/// took out when it attached pictures. A line of the brief that `paste` does not begin with
/// may be missing if it is blank (Claude drops them), or if it ends with the name of an
/// image file ([`names_an_image`]) and no more such lines are missing than the `attached`
/// placeholders the record begins with. A line that is there is matched where it is, so a
/// picture Claude could not read, which stays in the words, is no line to skip. None where
/// `paste` does not begin with the brief so.
fn after_brief<'a>(paste: &'a str, first: &str, mut attached: usize) -> Option<&'a str> {
    let mut rest = paste;
    // Pictures were attached: the record is that of a paste Claude took lines out of.
    let handled = attached > 0;
    for line in first.split_inclusive('\n') {
        if let Some(after) = rest.strip_prefix(line) {
            rest = after;
        } else if handled && line.trim().is_empty() {
            // A blank line, dropped from a paste whose pictures were attached.
        } else if attached > 0 && names_an_image(line) {
            attached -= 1;
        } else {
            return None;
        }
    }
    Some(rest)
}

/// Whether `line`, a line of a paste, ends with the name of an image file of the kinds
/// Claude Code reads and attaches: a path on a line of its own, whatever the machine's
/// separators (`/work/desk.png`, `C:\work\desk.png`), quoted or not, or several of them, so
/// the line ends with the last.
fn names_an_image(line: &str) -> bool {
    let line = line.trim();
    let line = line.strip_suffix(['"', '\'']).unwrap_or(line);
    line.rsplit_once('.').is_some_and(|(_, kind)| {
        ["png", "jpg", "jpeg", "gif", "webp"]
            .iter()
            .any(|image| kind.eq_ignore_ascii_case(image))
    })
}

/// The shape of `text`, an item that holds message `message_id`'s header where
/// [`is_paste_of`] takes it for no paste (3.0.7 chief-meeting, CMT-23), for the
/// issue it is filed in: how many times the header is in it, where the first is (in
/// characters of the text as the window shows it), what comes before it (`before`:
/// nothing but space, Claude Code's image placeholders, the opening of its wrapper,
/// or words), and how long the text is. Nothing of its words is kept.
pub fn header_shape(text: &str, message_id: i64) -> serde_json::Value {
    let marker = marker_of(message_id);
    let shown = window_text(text);
    let at = shown.find(&marker);
    let head = at.map_or("", |at| &shown[..at]);
    let before = if at.is_none() {
        "none"
    } else if head.trim().is_empty() {
        "space"
    } else if after_placeholders(head.trim_start()).is_some_and(|rest| rest.trim().is_empty()) {
        "image placeholders"
    } else if after_wrapper_opening(head.trim_start())
        .is_some_and(|(_, rest)| rest.trim().is_empty())
    {
        "wrapper"
    } else {
        "words"
    };
    serde_json::json!({
        "markers": shown.matches(&marker).count(),
        "offset": head.chars().count(),
        "before": before,
        "length": shown.chars().count(),
    })
}

/// What follows the image placeholders ([`IMAGE`]) that begin `text`: a run of them, one
/// space between, each with a number; none where `text` does not begin with one.
fn after_placeholders(text: &str) -> Option<&str> {
    let mut rest = text;
    let mut seen = false;
    while let Some(after) = rest.strip_prefix(IMAGE) {
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        let Some(next) = after[digits..].strip_prefix(']').filter(|_| digits > 0) else {
            break;
        };
        (rest, seen) = (next, true);
        match rest.strip_prefix(' ') {
            Some(more) if more.starts_with(IMAGE) => rest = more,
            _ => break,
        }
    }
    seen.then_some(rest)
}

/// What follows the opening of Claude Code's wrapper ([`PASTED`]) that begins `text`: the
/// tag's attributes, and what is after its line; none where `text` does not begin with one.
fn after_wrapper_opening(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix('<')?.strip_prefix(PASTED)?;
    let (attributes, inner) = rest.split_once(">\n")?;
    let whole =
        (attributes.is_empty() || attributes.starts_with(' ')) && !attributes.contains(['>', '\n']);
    whole.then_some((attributes, inner))
}

/// The paste `text` holds where Claude Code wrapped it ([`PASTED`]), else `text`: what follows
/// the opening, where the same tag closes after it. The closing tag need not end the text:
/// Claude Code, given a long paste in pieces (under load), wraps each piece of about a thousand
/// characters and leaves the last, shorter one bare after the last closing tag (the field bug of
/// 2026-10-25, T-763: five deliveries retried), and the header begins the first piece whatever
/// comes after it.
fn unwrapped(text: &str) -> &str {
    let Some((attributes, inner)) = after_wrapper_opening(text) else {
        return text;
    };
    if inner.contains(&format!("</{PASTED}{attributes}>")) {
        inner
    } else {
        text
    }
}

/// Whether `before`, the text ahead of the header of a delivery in a record's item, is
/// only what Claude Code put there itself and no word of the human's: the placeholders
/// of the images it attached ([`IMAGE`]) and, where it wrapped the paste ([`PASTED`]),
/// the line that opens the wrapper, after a blank line when placeholders come first.
pub fn put_there_by_claude(before: &str) -> bool {
    let before = before.trim_start();
    let after = after_placeholders(before).map_or(before, str::trim_start);
    after.is_empty() || after_wrapper_opening(after).is_some_and(|(_, inner)| inner.is_empty())
}

/// The message a window's record item, text of what it was given, is: the one
/// whose header comes first in it. A paste begins with its own header (behind the
/// placeholders of the images Claude Code attached to it, which are no header),
/// and carries the rows it keeps for the window under no header of their own, so
/// what comes first is the message; a header that the words of a message quote
/// comes after it, and is the message's no more than any other words are. None
/// where the text holds no header.
pub fn header_id(text: &str) -> Option<i64> {
    let after = &text[text.find(HEADER_START)? + HEADER_START.len()..];
    let digits = after.bytes().take_while(u8::is_ascii_digit).count();
    after[..digits].parse().ok()
}

/// The first `OPENING` UTF-16 code units of `body`, and not the half of a
/// pair that the cut would leave.
fn opening(body: &str) -> &str {
    let mut units = 0;
    for (at, character) in body.char_indices() {
        units += character.len_utf16();
        if units > OPENING {
            return &body[..at];
        }
    }
    body
}

/// A question says how to answer it; a result says what to do with it, so the
/// reader decides on the board even when its harness frames the message as a
/// request. A result's footer is not checked against its task here: the
/// decision on the task withdraws the result in the ledger, in its own
/// transaction, so a result that is pasted has not been decided on (the
/// dispatcher asks the ledger for the head of the queue again and begins the
/// delivery before it writes this text, awaiting nothing between), and one
/// already being pasted is in the window, where no check can take it back.
fn footer(message: &MessageView, cf: &CfWord, told_in: Option<i64>) -> String {
    let id = message.id;
    match (message.kind.as_str(), message.task_number) {
        ("question", task) => {
            if js::truthy(Some(&message.questions)) {
                // The ledger stores a list of questions or nothing.
                let several = message
                    .questions
                    .as_array()
                    .is_some_and(|questions| questions.len() > 1);
                let lines = if several {
                    "; one line per question"
                } else {
                    ""
                };
                format!(
                    "\n\nRun in your shell: {cf} answer m-{id} \"…\" (a label or your own words{lines})"
                )
            } else if let (true, Some(task)) = (message.urgent, task) {
                format!(
                    "\n\nT-{task} is paused for this. Run in your shell: {cf} answer m-{id} \"…\"; the chief resumes the task."
                )
            } else {
                format!("\n\nRun in your shell: {cf} answer m-{id} \"…\"")
            }
        }
        ("result", Some(task)) => {
            // The answer of the window's turn on a tell, given in a note before (T-958).
            let beside = told_in.map_or_else(String::new, |note| {
                format!("The turn on your words while T-{task} was paused ended with a report too, given in m-{note}: this result is the one to decide. ")
            });
            format!(
                "\n\n{beside}Decide with: {cf} task accept T-{task} · {cf} task reopen T-{task} \"…\""
            )
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_opening_is_fifteen_thousand_units_and_never_half_a_pair() {
        let long = |before: usize, middle: &str| {
            format!("{}{middle}{}", "x".repeat(before), "y".repeat(3_000))
        };
        assert_eq!(opening(&"x".repeat(16_001)), "x".repeat(15_000));
        // The emoji's halves are units 14,999 and 15,000: the cut is between them.
        assert_eq!(opening(&long(14_999, "🙂")), "x".repeat(14_999));
        // It ends at unit 15,000 exactly: whole.
        assert_eq!(
            opening(&long(14_998, "🙂")),
            format!("{}🙂", "x".repeat(14_998))
        );
        // It starts at unit 15,000: none of it.
        assert_eq!(opening(&long(15_000, "🙂")), "x".repeat(15_000));
        // A character of three bytes counts one unit.
        assert_eq!(opening(&"漢".repeat(16_001)), "漢".repeat(15_000));
    }

    /// A message to the chief saying `body`, from ConsensFlow.
    fn message(id: i64, task: Option<i64>, body: String) -> MessageView {
        MessageView {
            id,
            project_id: 1,
            recipient: "chief".to_owned(),
            recipient_id: 2,
            recipient_role: "chief".to_owned(),
            sender: None,
            kind: "note".to_owned(),
            task_number: task,
            reply_to: None,
            body,
            state: "queued".to_owned(),
            attempts: 0,
            reason: None,
            receipt: serde_json::Value::Null,
            questions: serde_json::Value::Null,
            choices: serde_json::Value::Null,
            urgent: false,
            created_at: "2026-10-13T12:00:00.000Z".to_owned(),
            delivered_at: None,
        }
    }

    #[test]
    fn a_message_goes_whole_to_sixteen_thousand_units_and_is_cut_past_them() {
        for (body, whole) in [
            ("x".repeat(16_000), true),
            ("x".repeat(16_001), false),
            // The pair counts two units: a body of 15,999 and one emoji is 16,001.
            (format!("{}🙂", "x".repeat(15_998)), true),
            (format!("{}🙂", "x".repeat(15_999)), false),
            ("漢".repeat(16_000), true),
        ] {
            let said = message(7, None, body);
            assert_eq!(goes_whole(&said, &[], &CfWord::bare()), whole);
            // What reads as cut in the pane is what `goes_whole` says is cut.
            assert_eq!(
                delivery_text(&said, &[], &CfWord::bare()).contains("read all of it with"),
                !whole
            );
        }
    }

    /// A message a start gave back though its window took it says so in its header,
    /// after its marker, and its record proves it as any paste's (T-280).
    #[test]
    fn a_message_sent_again_after_a_restart_says_so_in_its_header_and_its_record_proves_it() {
        let said = message(12, Some(3), "Look at this".to_owned());
        let again = delivery_text_sent_again(&said, &[], &CfWord::bare());
        assert_eq!(
            again.lines().next(),
            Some("[ConsensFlow m-12 \u{b7} T-3 \u{b7} note from ConsensFlow \u{b7} sent again after a restart, it may be there already]")
        );
        assert_eq!(
            again.replacen(SENT_AGAIN, "", 1),
            delivery_text(&said, &[], &CfWord::bare()),
            "the word, and nothing else"
        );
        assert!(is_paste_of(&again, 12, None));
        assert_eq!(header_id(&again), Some(12));
    }

    #[test]
    fn an_item_is_the_message_whose_header_comes_first_in_it_and_no_other_it_quotes() {
        let said = message(12, Some(3), "Look at what m-5 said".to_owned());
        let text = delivery_text(&said, &[], &CfWord::bare());
        assert_eq!(header_id(&text), Some(12));
        // A window that frames what it was given still shows the header first.
        assert_eq!(header_id(&format!("> {text}")), Some(12));
        // The header another message has, quoted in the words of this one, is not this one's.
        let quoting = message(13, Some(3), format!("As {} said: be brief", marker_of(5)));
        assert_eq!(
            header_id(&delivery_text(&quoting, &[], &CfWord::bare())),
            Some(13)
        );
        // No header, or one cut short of its id: no message.
        assert_eq!(header_id("Understood."), None);
        assert_eq!(header_id("see [ConsensFlow m-x"), None);
        assert_eq!(header_id(&marker_of(7)), Some(7));
        // What the marker proves arrived is the same header.
        assert!(text.contains(&marker_of(12)));
    }

    #[test]
    fn a_paste_is_an_item_its_header_begins_and_no_words_that_quote_it() {
        let said = message(12, Some(3), "Look at what m-5 said".to_owned());
        let text = delivery_text(&said, &[], &CfWord::bare());
        assert!(is_paste_of(&text, 12, None));
        assert!(!is_paste_of(&text, 1, None), "m-1 is not m-12");
        // As a window keeps it: lines ended otherwise, or a blank line first.
        let kept = format!("\n{}", text.replace('\n', "\r\n"));
        assert!(is_paste_of(&kept, 12, None));
        // A header quoted in the human's words is no paste of it.
        let asked = format!(
            "Why is \"{}· note from @zeus]\" still queued?",
            marker_of(12)
        );
        assert!(!is_paste_of(&asked, 12, None));
        let shown = format!("Look at this:\n\n{}· note from @zeus]", marker_of(12));
        assert!(!is_paste_of(&shown, 12, None));
        // Nor is one in a message's: m-13 quoting m-12 after a blank line (Astra), a
        // note that says "Investigate this:" and the header after it (Sol), or one on
        // a line of its own.
        let quoting = |id: i64, body: String| {
            delivery_text(&message(id, Some(3), body), &[], &CfWord::bare())
        };
        let astra = quoting(13, format!("Before it:\n\n{text}"));
        let sol = quoting(43, format!("Investigate this:\n\n{}· T-3]", marker_of(42)));
        let lined = quoting(14, format!("They said:\n{}", marker_of(5)));
        assert!(is_paste_of(&astra, 13, None) && !is_paste_of(&astra, 12, None));
        assert!(!is_paste_of(&sol, 42, None) && !is_paste_of(&lined, 5, None));
        let mid_line = quoting(15, format!("As {} said: be brief", marker_of(5)));
        assert!(!is_paste_of(&mid_line, 5, None));
    }

    #[test]
    fn a_paste_claude_code_wrapped_in_its_record_is_the_paste_it_wraps() {
        let text = delivery_text(
            &message(1, Some(1), "The page is done".to_owned()),
            &[],
            &CfWord::bare(),
        );
        // As Claude Code 2.1.296 records it (the live paste suite's records).
        let wrapped =
            format!("\n\n<pasted_content id=\"b118\">\n{text}\n</pasted_content id=\"b118\">\n");
        assert!(is_paste_of(&wrapped, 1, None));
        assert!(is_paste_of(&wrapped.replace('\n', "\r\n"), 1, None));
        // Only the tag as Claude writes it: opened on a line of its own, closed the same.
        let wrapped_as = |open: &str, close: &str| format!("{open}\n{text}\n{close}\n");
        for (open, close) in [
            (
                "<pasted_content id=\"b118\">",
                "</pasted_content id=\"4f1a\">",
            ),
            ("<pasted_content id=\"b118\">", ""),
            (
                "<pasted_contents id=\"b118\">",
                "</pasted_contents id=\"b118\">",
            ),
            ("<quote>", "</quote>"),
        ] {
            assert!(!is_paste_of(&wrapped_as(open, close), 1, None), "{open}");
        }
        // And a header quoted inside the wrapper is no more a paste than outside it.
        let quoted = format!(
            "<pasted_content id=\"b118\">\nSee {}\n</pasted_content id=\"b118\">",
            marker_of(1)
        );
        assert!(!is_paste_of(&quoted, 1, None));
    }

    /// `length` characters of words, none of them a header.
    fn filler(length: usize) -> String {
        "words "
            .repeat(length / 6 + 1)
            .chars()
            .take(length)
            .collect()
    }

    /// A paste as Claude Code records one that reached it in pieces under load (the field's
    /// items of 2026-10-25 and 26, T-763): each piece in the wrapper, the tag closed on a line
    /// of its own, three line breaks between two; the first holds `first` and then words up to
    /// `blocks[0]` characters, and a last piece of `tail` characters is bare after a blank
    /// line: none where `tail` is 0, the record ending on the closing tag and a line break.
    fn in_pieces(first: &str, blocks: &[usize], tail: usize) -> String {
        let piece = |at: usize, length: usize| {
            let words = if at == 0 {
                format!(
                    "{first}\n{}",
                    filler(length.saturating_sub(first.len() + 1))
                )
            } else {
                filler(length)
            };
            format!("<pasted_content id=\"aead\">\n{words}\n</pasted_content id=\"aead\">")
        };
        let wrapped: Vec<String> = blocks
            .iter()
            .enumerate()
            .map(|(at, length)| piece(at, *length))
            .collect();
        let tail = if tail == 0 {
            "\n".to_owned()
        } else {
            format!("\n\n{}", filler(tail))
        };
        format!("\n\n{}{tail}", wrapped.join("\n\n\n"))
    }

    /// The five first items of the field's retried deliveries that held a loose tail
    /// (m-4272, m-4273, m-4367, m-4382, m-4518): the sizes of their pieces and of the tail
    /// after the last closing tag, as the ledger's copy of the record measured them.
    const LOOSE: [(i64, &[usize], usize); 5] = [
        (4272, &[4_078, 1_018, 1_012, 1_024], 746),
        (4273, &[3_060, 1_024, 1_024, 1_022, 1_022, 1_022], 231),
        (4367, &[1_016, 1_024, 1_024, 1_022], 326),
        (4382, &[1_004, 1_020, 1_020, 1_016], 767),
        (4518, &[10_210], 791),
    ];

    /// The header the field's paste of message `id` began with.
    fn field_header(id: i64) -> String {
        format!("[ConsensFlow m-{id} \u{b7} T-45 \u{b7} result from @pygmalion-jolly-brook]")
    }

    #[test]
    fn a_paste_claude_code_recorded_in_pieces_with_the_last_left_bare_is_the_paste() {
        for (id, blocks, tail) in LOOSE {
            let item = in_pieces(&field_header(id), blocks, tail);
            assert!(is_paste_of(&item, id, None), "m-{id}");
            assert!(is_paste_of(&item.replace('\n', "\r\n"), id, None), "m-{id}");
            assert!(!is_paste_of(&item, id + 1, None), "not m-{}", id + 1);
            assert_eq!(header_id(&item), Some(id), "m-{id}");
            // What is ahead of the header is the wrapper's opening and no word of the human's.
            let ahead = &item[..item.find(HEADER_START).unwrap()];
            assert!(put_there_by_claude(ahead), "m-{id}: {ahead:?}");
        }
        // The two the field confirmed at once, whose last piece closed the record (m-4109, seven
        // pieces; m-4519, four), stay the paste.
        for (id, blocks) in [
            (4109, &[1_014, 1_024, 1_024, 1_024, 1_022, 1_012, 822][..]),
            (4519, &[1_016, 1_023, 1_022, 805][..]),
        ] {
            let item = in_pieces(&field_header(id), blocks, 0);
            assert!(is_paste_of(&item, id, None), "m-{id}");
        }
        // The tail is a loose piece of any size, and the pieces are any number: one, of a
        // paste with the images' placeholders ahead of it, or of one wrapped alone.
        let header = field_header(7);
        for (blocks, tail) in [
            (&[900][..], 1),
            (&[1_000, 1_000][..], 999),
            (&[50][..], 5_000),
        ] {
            assert!(is_paste_of(&in_pieces(&header, blocks, tail), 7, None));
        }
        let behind_images = format!(
            "[Image #7] [Image #8]{}",
            in_pieces(&header, &[1_016, 1_024], 300)
        );
        assert!(
            is_paste_of(&behind_images, 7, None),
            "behind the placeholders of its images"
        );
    }

    #[test]
    fn a_header_the_pieces_of_a_paste_only_quote_or_hold_after_the_first_is_no_paste_for_a_loose_tail(
    ) {
        let header = field_header(42);
        let quoting = format!("Why is \"{header}\" still queued?");
        let blocks = [1_016, 1_024, 1_022];
        // The first piece is words, and the header is quoted in them, or begins the second piece,
        // or the tail: the human pasted something and asked about the message.
        let second = in_pieces("Look at what they pasted", &blocks, 326).replacen(
            &filler(1_024),
            &format!("{header}\n{}", filler(1_000)),
            1,
        );
        assert!(
            second.contains(&header),
            "the header is in the second piece"
        );
        let in_tail = format!(
            "{}\n\n{header}",
            in_pieces("Look at what they pasted", &blocks, 0).trim_end()
        );
        for item in [
            in_pieces(&quoting, &blocks, 326),
            format!("See {}", in_pieces(&header, &blocks, 326)),
            second,
            in_tail,
            // Another message's header begins it, and this message's is quoted behind it.
            in_pieces(
                &format!("{}\nAs {header} said", field_header(43)),
                &blocks,
                326,
            ),
        ] {
            assert!(!is_paste_of(&item, 42, None), "{item:?}");
        }
        // A tail with no wrapper the first piece was closed in proves nothing: the tag opened and
        // never closed, closed with another id, or the tag of another name.
        let loose = format!("\n\n{}", filler(300));
        for wrapped in [
            format!("<pasted_content id=\"b118\">\n{header}\n{}", filler(900)),
            format!("<pasted_content id=\"b118\">\n{header}\n</pasted_content id=\"4f1a\">"),
            format!("<pasted_contents id=\"b118\">\n{header}\n</pasted_contents id=\"b118\">"),
            format!("<quote>\n{header}\n</quote>"),
        ] {
            assert!(
                !is_paste_of(&format!("\n\n{wrapped}{loose}"), 42, None),
                "{wrapped}"
            );
        }
        // Closed, with a tail, it is the paste.
        let closed = format!(
            "\n\n<pasted_content id=\"b118\">\n{header}\n</pasted_content id=\"b118\">{loose}"
        );
        assert!(is_paste_of(&closed, 42, None));
    }

    /// A result as the live probe of 2026-10-22 recorded it in Claude Code 2.1.308 once the
    /// paths of its pictures were turned into attachments (`tests/live_image_paste.rs`).
    const FIELD: &str = "[Image #4][ConsensFlow m-2619 · T-45 · result from @pygmalion-jolly-brook]\nWarm editorial desk scene.\n\nDecide with: cf task accept T-45 · cf task reopen T-45 \"…\"";

    #[test]
    fn a_paste_behind_the_placeholders_of_the_images_claude_attached_is_the_paste() {
        // The field's shape: a space between placeholders, none before the header.
        assert!(is_paste_of(FIELD, 2619, None));
        assert!(is_paste_of(&FIELD.replace('\n', "\r\n"), 2619, None));
        let header = "[ConsensFlow m-3 · T-1 · result from @pygmalion-jolly-brook]\nWarm.";
        for placeholders in [
            "[Image #1]",
            "[Image #2] [Image #3]",
            "[Image #14] [Image #15] [Image #16]",
        ] {
            assert!(
                is_paste_of(&format!("{placeholders}{header}"), 3, None),
                "{placeholders}"
            );
        }
        // Wrapped, a blank line between them and the wrapper (the probe's rows of
        // Claude Code 2.1.308), as a wrapped paste with no image has its blank lines first.
        let wrapped = format!("[Image #7] [Image #8]\n\n<pasted_content id=\"f920\">\n{header}\nMore.\nAnd more.\n</pasted_content id=\"f920\">\n");
        assert!(is_paste_of(&wrapped, 3, None));
        assert!(is_paste_of(&wrapped.replace('\n', "\r\n"), 3, None));
        let unattached = format!("\n\n<pasted_content id=\"f920\">\n{header}\n/tmp/gone.png\nMore.\n</pasted_content id=\"f920\">\n");
        assert!(
            is_paste_of(&unattached, 3, None),
            "a path to a file Claude could not read"
        );
        assert!(
            !is_paste_of(&wrapped, 33, None) && !is_paste_of(FIELD, 261, None),
            "m-261 is not m-2619"
        );
        // The next item, where the attachments came from, is no message's paste.
        assert!(!is_paste_of(
            "[Image: source: /tmp/desk.png]\n[Image: source: /tmp/hero.png]",
            3,
            None
        ));
        // The header after a blank line with no wrapper is taken for the paste too: a shape
        // Claude does not write (it wraps what it moves behind a blank line), and no wider
        // than a header that begins words a paste does.
        assert!(is_paste_of(&format!("[Image #5]\n\n{header}"), 3, None));
        assert!(!is_paste_of(&format!("[Image #5]\n{header}"), 3, None));
    }

    #[test]
    fn a_header_in_other_words_is_no_paste_for_the_placeholders_before_it() {
        let header = "[ConsensFlow m-42 · T-1 · note from @zeus]";
        for text in [
            // Words between the placeholders and the header, on the line or after a blank one.
            format!("[Image #5] Investigate this: {header}"),
            format!("[Image #5]\n\nInvestigate this:\n\n{header}"),
            format!("[Image #5]\nSee {header}"),
            // Placeholders as Claude does not write them.
            format!("[Image]{header}"),
            format!("[Image #]{header}"),
            format!("[Image #x]{header}"),
            format!("[Image #5]  [Image #6]{header}"),
            format!("[Image #5],[Image #6]{header}"),
            format!("[Image #5] {header}"),
            format!("[Image #5]\n{header}"),
            // Placeholders after words.
            format!("Look: [Image #5]{header}"),
            // A header the paste quotes, behind its own.
            format!("[Image #5][ConsensFlow m-43 · T-1 · note from @zeus]\nAs {header}"),
        ] {
            assert!(!is_paste_of(&text, 42, None), "{text}");
        }
        assert!(is_paste_of(
            &format!("[Image #5][ConsensFlow m-43 · T-1 · note from @zeus]\n{header}"),
            43,
            None
        ));
        // A wrapper that is none of Claude's is the same behind placeholders as without.
        assert!(!is_paste_of(
            &format!("[Image #5]\n\n<quote>\n{header}\n</quote>\n"),
            42,
            None
        ));
    }

    #[test]
    fn the_message_of_an_item_is_found_behind_its_placeholders_too() {
        assert_eq!(header_id(FIELD), Some(2619));
        assert_eq!(header_id("[Image #2] [Image #3]\n\n<pasted_content id=\"f920\">\n[ConsensFlow m-6 · T-1 · result from @x]\nSaved.\n</pasted_content id=\"f920\">\n"), Some(6));
        // The attachments' sources hold no header.
        assert_eq!(header_id("[Image: source: /tmp/desk.png]"), None);
    }

    #[test]
    fn what_claude_code_puts_ahead_of_a_paste_is_no_word_of_the_humans_and_nothing_else_is_that() {
        let wrapper = "<pasted_content id=\"f920\">\n";
        for ahead in [
            "".to_owned(),
            "[Image #4]".to_owned(),
            "[Image #1] [Image #2]".to_owned(),
            wrapper.to_owned(),
            format!("\n\n{wrapper}"),
            format!("[Image #7] [Image #8]\n\n{wrapper}"),
            "<pasted_content>\n".to_owned(),
        ] {
            assert!(put_there_by_claude(&ahead), "{ahead:?}");
        }
        for ahead in [
            "look".to_owned(),
            "[Image #4]look".to_owned(),
            "look [Image #4]".to_owned(),
            "[Image #4] look".to_owned(),
            "[Image #]".to_owned(),
            "[Image #4][Image #5]".to_owned(),
            "[Image #4]\n\nlook".to_owned(),
            format!("[Image #4]\n\n{wrapper}look\n"),
            format!("typed {wrapper}"),
            "<pasted_content id=\"f\">look\n".to_owned(),
            "<pasted_contentious>\n".to_owned(),
        ] {
            assert!(!put_there_by_claude(&ahead), "{ahead:?}");
        }
    }

    #[test]
    fn a_second_header_is_a_paste_only_behind_the_brief_a_fresh_window_was_given_first() {
        let brief = message(5, Some(3), "Write the parser".to_owned());
        let first = brief_first(&brief, &[], &CfWord::bare());
        let answer = delivery_text(
            &message(12, Some(3), "Use JSON".to_owned()),
            &[],
            &CfWord::bare(),
        );
        let launched = format!("{first}{answer}");
        assert!(is_paste_of(&launched, 12, Some(&first)));
        assert!(is_paste_of(
            &launched.replace('\n', "\r\n"),
            12,
            Some(&first)
        ));
        assert!(
            is_paste_of(&launched, 5, Some(&first)),
            "and the brief's own"
        );
        // Not where nothing says it was given first, nor behind another brief.
        assert!(!is_paste_of(&launched, 12, None));
        let other = brief_first(
            &message(6, Some(3), "Write the lexer".to_owned()),
            &[],
            &CfWord::bare(),
        );
        assert!(!is_paste_of(&launched, 12, Some(&other)));
        // Nor in a paragraph of the brief's own words that quotes it.
        let quoting = message(5, Some(3), format!("Write the parser\n\n{answer}"));
        let only_the_brief = delivery_text(&quoting, &[], &CfWord::bare());
        assert!(!is_paste_of(
            &only_the_brief,
            12,
            Some(&brief_first(&quoting, &[], &CfWord::bare()))
        ));
    }

    /// What Claude Code records of `paste` when it attaches the pictures whose paths stand
    /// on a line of their own in it (`tests/live_image_paste.rs`): a placeholder for each, a
    /// space between, and the words with those lines gone, no break before them. `taken`
    /// says which of the paste's lines it attached, by their numbers. The blank lines are
    /// kept, as the record of the field showed them.
    fn claude_records(paste: &str, taken: &[usize]) -> String {
        let kept: Vec<&str> = paste
            .lines()
            .enumerate()
            .filter(|(at, _)| !taken.contains(at))
            .map(|(_, line)| line)
            .collect();
        let marks: Vec<String> = (1..=taken.len()).map(|n| format!("[Image #{n}]")).collect();
        format!("{}{}", marks.join(" "), kept.join("\n"))
    }

    /// How a window records the paste it was given: the item's words.
    type Records = fn(&str) -> String;

    /// The first message of a fresh window whose brief says `brief_words`, a message m-12
    /// ("Use JSON") behind it: the brief it is given first, and the whole of its paste.
    fn launched_behind(brief_words: &str) -> (String, String) {
        launched_behind_saying(brief_words, "Use JSON")
    }

    /// The same, the message m-12 saying `message_words`.
    fn launched_behind_saying(brief_words: &str, message_words: &str) -> (String, String) {
        let brief = message(5, Some(3), brief_words.to_owned());
        let first = brief_first(&brief, &[], &CfWord::bare());
        let answer = delivery_text(
            &message(12, Some(3), message_words.to_owned()),
            &[],
            &CfWord::bare(),
        );
        let paste = format!("{first}{answer}");
        (first, paste)
    }

    /// The picture the message names and not the brief, or both: the placeholders are the
    /// paste's, whoever's pictures they stand for, and the brief is matched as before with
    /// the blank line it had.
    #[test]
    fn a_picture_the_message_names_leaves_the_brief_before_it_matched_as_it_was() {
        // (the brief, the message, the lines Claude attached)
        let rows: [(&str, &str, &[usize]); 2] = [
            ("Write the parser", "Look\n/work/images/shot.png", &[5]),
            (
                "Write the parser\n/work/images/desk.png",
                "Look\n/work/images/shot.png",
                &[2, 6],
            ),
        ];
        for (at, (brief_words, message_words, taken)) in rows.into_iter().enumerate() {
            let (first, paste) = launched_behind_saying(brief_words, message_words);
            let record = claude_records(&paste, taken);
            let dropped: Vec<&str> = record
                .lines()
                .filter(|line| !line.trim().is_empty())
                .collect();
            for (record, blanks) in [(record.clone(), "kept"), (dropped.join("\n"), "dropped")] {
                assert!(
                    is_paste_of(&record, 12, Some(&first)),
                    "row {at}, blank lines {blanks}: {record:?}"
                );
                assert!(
                    !is_paste_of(&record, 12, None),
                    "row {at}, blank lines {blanks}: no brief given first"
                );
            }
        }
    }

    /// The first message of a fresh window is its paste behind the brief it was given
    /// first, where Claude Code attached the pictures the brief names by their paths and
    /// left those lines out of its record (Calliope's P3-1 of T-803; T-812): the proof
    /// takes the brief with the lines Claude dropped gone, as many as it has placeholders
    /// for. One row for each shape: the brief's words, how the record shows the paste, and
    /// whether that proves m-12 arrived.
    #[test]
    fn the_first_message_of_a_fresh_window_is_its_paste_behind_a_brief_whose_pictures_claude_attached(
    ) {
        // Line 0 of a paste is the brief's header, lines 1 on its words.
        let rows: [(&str, &str, Records, bool); 16] = [
            (
                "a path on a line of its own, mid-brief",
                "Write the parser\n/work/images/desk.png\nSee the picture.",
                |paste| claude_records(paste, &[2]),
                true,
            ),
            (
                "a path on the brief's last line, the blank line after it",
                "Write the parser\n/work/images/desk.png",
                |paste| claude_records(paste, &[2]),
                true,
            ),
            (
                "two paths on the lines after the brief's first",
                "Write the parser\n/work/images/desk.png\n/work/images/hero.png\nSee them.",
                |paste| claude_records(paste, &[2, 3]),
                true,
            ),
            (
                "two paths on one line",
                "Write the parser\n/work/images/desk.png /work/images/hero.png\nSee them.",
                |paste| {
                    claude_records(paste, &[2]).replacen("[Image #1]", "[Image #1] [Image #2]", 1)
                },
                true,
            ),
            (
                "an upper-case extension",
                "Write the parser\n/work/images/DESK.PNG\nSee it.",
                |paste| claude_records(paste, &[2]),
                true,
            ),
            (
                "a path of Windows, a drive letter and backslashes",
                "Write the parser\nC:\\work\\images\\desk.png\nSee it.",
                |paste| claude_records(paste, &[2]),
                true,
            ),
            (
                "a picture Claude could not read stays in the words, another is attached",
                "Write the parser\n/work/images/gone.png\n/work/images/desk.png\nSee it.",
                |paste| claude_records(paste, &[3]),
                true,
            ),
            (
                "a brief of paragraphs, a picture between them",
                "Write the parser\n\nUse the grammar.\n/work/images/desk.png\n\nSee the picture.",
                |paste| claude_records(paste, &[4]),
                true,
            ),
            (
                "a path in quotes, as a terminal pastes one with a space in it",
                "Write the parser\n\"/work/my images/desk.png\"\nSee it.",
                |paste| claude_records(paste, &[2]),
                true,
            ),
            (
                "the blank lines gone and no placeholder: nothing was attached",
                "Write the parser\n\nSee it.",
                |paste| paste.replace("\n\n", "\n"),
                false,
            ),
            (
                "no picture in the brief, no placeholder",
                "Write the parser\nSee the picture.",
                |paste| paste.to_owned(),
                true,
            ),
            (
                "the line is gone and no placeholder says it was attached",
                "Write the parser\n/work/images/desk.png\nSee the picture.",
                |paste| claude_records(paste, &[2]).replacen("[Image #1]", "", 1),
                false,
            ),
            (
                "two lines gone, one placeholder",
                "Write the parser\n/work/images/desk.png\n/work/images/hero.png\nSee them.",
                |paste| {
                    claude_records(paste, &[2, 3]).replacen(
                        "[Image #1] [Image #2]",
                        "[Image #1]",
                        1,
                    )
                },
                false,
            ),
            (
                "a line that names no picture, gone from the record behind a placeholder",
                "Write the parser\n/work/notes.txt\nSee it.",
                |paste| claude_records(paste, &[2]),
                false,
            ),
            (
                "other words in the place of the line a picture's path was",
                "Write the parser\n/work/images/desk.png\nSee the picture.",
                |paste| {
                    format!(
                        "[Image #1]{}",
                        paste.replace("/work/images/desk.png", "(a picture)")
                    )
                },
                false,
            ),
            (
                "a word of the brief's own gone beside the picture's line",
                "Write the parser\n/work/images/desk.png\nSee the picture.",
                |paste| claude_records(paste, &[2]).replacen("See the picture.\n", "", 1),
                false,
            ),
        ];
        for (shape, words, records, proves) in rows {
            let (first, paste) = launched_behind(words);
            let kept = records(&paste);
            // The record of a paste Claude attached pictures of has no blank line in it
            // (its paste handler drops them), and one that it did not has them all.
            let dropped = kept.starts_with("[Image #").then(|| {
                let lines: Vec<&str> = kept
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .collect();
                lines.join("\n")
            });
            for (record, blanks) in std::iter::once((kept, "blank lines kept"))
                .chain(dropped.map(|record| (record, "blank lines dropped")))
            {
                assert_eq!(
                    is_paste_of(&record, 12, Some(&first)),
                    proves,
                    "{shape}, {blanks}: {record:?}"
                );
                // The window's own line breaks, a CR before each.
                assert_eq!(
                    is_paste_of(&record.replace('\n', "\r\n"), 12, Some(&first)),
                    proves,
                    "{shape}, {blanks}, with CRLF"
                );
                // Not where nothing says the brief was given first.
                assert!(
                    !is_paste_of(&record, 12, None),
                    "{shape}, {blanks}: no brief given first"
                );
            }
        }
    }

    /// The same shape written out, as the field would show it: the brief's header behind
    /// its placeholder, its words without the picture's line, a blank line, the message.
    #[test]
    fn a_fresh_windows_record_behind_a_brief_with_a_picture_is_the_message_as_claude_writes_it() {
        let (first, _) = launched_behind("Write the parser\n/work/images/desk.png\nSee it.");
        let record = "[Image #1][ConsensFlow m-5 \u{b7} T-3 \u{b7} note from ConsensFlow]\nWrite the parser\nSee it.\n\n[ConsensFlow m-12 \u{b7} T-3 \u{b7} note from ConsensFlow]\nUse JSON";
        assert!(is_paste_of(record, 12, Some(&first)));
        assert!(is_paste_of(record, 5, Some(&first)), "and the brief's own");
        assert!(!is_paste_of(record, 13, Some(&first)));
        // As Claude Code 2.1.314 to 2.1.317 writes it: its paste handler drops the blank line.
        let handled = record.replace("\n\n", "\n");
        assert!(is_paste_of(&handled, 12, Some(&first)), "{handled}");
        assert!(!is_paste_of(&handled, 13, Some(&first)));
        // In Claude's wrapper too, where the paste was long: the placeholder, a blank line.
        let wrapped = format!(
            "[Image #1]\n\n<pasted_content id=\"f920\">\n{}\n</pasted_content id=\"f920\">\n",
            record.strip_prefix("[Image #1]").unwrap()
        );
        assert!(is_paste_of(&wrapped, 12, Some(&first)), "{wrapped}");
        // A header the brief's own words quote, behind the same placeholder, is no paste.
        let quoting = message(
            5,
            Some(3),
            "Write the parser\n/work/images/desk.png\n\n[ConsensFlow m-12 \u{b7} T-3 \u{b7} note from ConsensFlow]\nUse JSON".to_owned(),
        );
        let only_the_brief = delivery_text(&quoting, &[], &CfWord::bare());
        let quoted = format!(
            "[Image #1]{}",
            only_the_brief.replacen("/work/images/desk.png\n", "", 1)
        );
        assert!(
            !is_paste_of(
                &quoted,
                12,
                Some(&brief_first(&quoting, &[], &CfWord::bare()))
            ),
            "{quoted}"
        );
    }

    #[test]
    fn a_chain_of_consensflows_notes_reads_as_one_message_a_line_each() {
        // RS-25: the notes that wait for a chief together go in the first one's paste.
        let first = message(9, Some(1), "T-1's window closed.".to_owned());
        let carried = [
            message(
                10,
                Some(2),
                "T-2's window closed;\nits lock is free.".to_owned(),
            ),
            message(11, None, "The human changed the staff's rules.".to_owned()),
        ];
        assert_eq!(
            delivery_text(&first, &carried, &CfWord::bare()),
            "[ConsensFlow m-9 · note from ConsensFlow]\n3 notes at once, a line each:\nm-9 · T-1: T-1's window closed.\nm-10 · T-2: T-2's window closed;\n  its lock is free.\nm-11: The human changed the staff's rules."
        );
        assert!(is_paste_of(
            &delivery_text(&first, &carried, &CfWord::bare()),
            9,
            None
        ));
    }

    #[test]
    fn a_message_that_carries_others_goes_whole_by_the_words_of_all_of_them() {
        let mut carrier = message(9, Some(1), "x".repeat(8_000));
        carrier.kind = "task".to_owned();
        let carried = [message(8, Some(1), "y".repeat(8_000))];
        // Its own words are short of the limit; with the row it carries, and the
        // line over the row, they are not.
        assert!(goes_whole(&carrier, &[], &CfWord::bare()));
        assert!(!goes_whole(&carrier, &carried, &CfWord::bare()));
        assert!(delivery_text(&carrier, &carried, &CfWord::bare()).contains("cf task get T-1"));
    }

    /// The harness reads the delivery a cut turn began with (`Opening::began_with`, T-1328)
    /// from a record's item as this crate's proof of an arrival does (`is_paste_of`): the
    /// header begins the item, behind only what Claude Code put there itself. A header that
    /// words quote, or that follows other words, names none in either.
    #[test]
    fn the_harness_reads_the_delivery_an_item_began_with_as_the_proof_of_an_arrival_does() {
        use cf_harness::contract::Opening;
        let delivery = message(12, Some(3), "Fix the parser".to_owned());
        let paste = delivery_text(&delivery, &[], &CfWord::bare());
        let wrapped = |inner: &str| {
            format!("<pasted_content id=\"b118\">\n{inner}\n</pasted_content id=\"b118\">")
        };
        let mut failed = Vec::new();
        for (text, named) in [
            (paste.clone(), true),
            (wrapped(&paste), true),
            (format!("[Image #4]{paste}"), true),
            (format!("[Image #4]\n\n{}", wrapped(&paste)), true),
            (format!("[Image #4] [Image #5]{paste}"), true),
            (format!("  \n{paste}"), true),
            (format!("Why is \"{paste}\" queued?"), false),
            (format!("Investigate this:\n\n{paste}"), false),
            (format!("key: abc {paste}"), false),
            (format!("<pasted_content id=\"b1\"> {paste}"), false),
            (paste.replace("m-12", "m-12x"), false),
            ("pass\nword\nhunter22".to_owned(), false),
        ] {
            let theirs = Opening::began_with(&text, None).delivery;
            let ours = header_id(&text).filter(|id| is_paste_of(&text, *id, None));
            if theirs != ours || theirs.is_some() != named {
                failed.push(format!("{text:?}: harness {theirs:?}, proof {ours:?}"));
            }
        }
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
