//! How a message reads in its recipient's pane. Its header doubles as the proof
//! that it arrived: a delivery counts once the window's own record shows the
//! header's start ([`marker_of`]), so the two are written together. A message
//! that carries others (what its window kept for it) reads as one paste: their
//! words and its own, in the order of their ids, under the one header, which is
//! the one marker that proves the paste arrived.

use std::borrow::Cow;

use cf_base::js;
use cf_base::text::utf16_len;
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
pub fn delivery_text(message: &MessageView, carried: &[MessageView]) -> String {
    let id = message.id;
    let from = from_of(message);
    let task = message
        .task_number
        .map_or_else(String::new, |number| format!(" · T-{number}"));
    let (whole, read_all) = words_of(message, carried);
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
        "[ConsensFlow m-{id}{task} · {} from {from}]\n{body}{}",
        message.kind,
        footer(message)
    )
}

/// The words a message has whole, and the command that reads them where they are
/// cut: its own body, or, where it carries rows, theirs and its own together.
fn words_of<'a>(message: &'a MessageView, carried: &[MessageView]) -> (Cow<'a, str>, String) {
    let id = message.id;
    match message.task_number.filter(|_| !carried.is_empty()) {
        None => (
            Cow::Borrowed(message.body.as_str()),
            format!("cf inbox read m-{id}"),
        ),
        Some(number) => (
            Cow::Owned(composite(message, carried)),
            format!("cf task get T-{number}"),
        ),
    }
}

/// Whether a message goes into its recipient's pane in whole: its words are no
/// longer than a paste is given, and are not cut to their opening and the command
/// that reads the rest.
pub fn goes_whole(message: &MessageView, carried: &[MessageView]) -> bool {
    utf16_len(&words_of(message, carried).0) <= INLINE_LIMIT
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
fn composite(carrier: &MessageView, carried: &[MessageView]) -> String {
    let mut rows: Vec<&MessageView> = carried.iter().chain([carrier]).collect();
    rows.sort_by_key(|row| row.id);
    rows.iter()
        .map(|row| {
            if row.id == carrier.id {
                row.body.clone()
            } else {
                format!("{}\n{}", label(row), row.body)
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
const HEADER_START: &str = "[ConsensFlow m-";

/// The start of a message's header, as a window's record shows it once the
/// message arrived. The space after the id keeps m-1 from matching m-12; what
/// follows it is not part of the marker, since a window may not keep the ·
/// (Devin on Windows takes it as |, see `console_text`).
pub fn marker_of(message_id: i64) -> String {
    format!("{HEADER_START}{message_id} ")
}

/// The message a window's record item, text of what it was given, is: the one
/// whose header comes first in it. A paste begins with its own header, and
/// carries the rows it keeps for the window under no header of their own, so
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
fn footer(message: &MessageView) -> String {
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
                    "\n\nRun in your shell: cf answer m-{id} \"…\" (a label or your own words{lines})"
                )
            } else if let (true, Some(task)) = (message.urgent, task) {
                format!(
                    "\n\nT-{task} is paused for this. Run in your shell: cf answer m-{id} \"…\"; the chief resumes the task."
                )
            } else {
                format!("\n\nRun in your shell: cf answer m-{id} \"…\"")
            }
        }
        ("result", Some(task)) => {
            format!("\n\nDecide with: cf task accept T-{task} · cf task reopen T-{task} \"…\"")
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
            assert_eq!(goes_whole(&said, &[]), whole);
            // What reads as cut in the pane is what `goes_whole` says is cut.
            assert_eq!(
                delivery_text(&said, &[]).contains("read all of it with"),
                !whole
            );
        }
    }

    #[test]
    fn an_item_is_the_message_whose_header_comes_first_in_it_and_no_other_it_quotes() {
        let said = message(12, Some(3), "Look at what m-5 said".to_owned());
        let text = delivery_text(&said, &[]);
        assert_eq!(header_id(&text), Some(12));
        // A window that frames what it was given still shows the header first.
        assert_eq!(header_id(&format!("> {text}")), Some(12));
        // The header another message has, quoted in the words of this one, is not this one's.
        let quoting = message(13, Some(3), format!("As {} said: be brief", marker_of(5)));
        assert_eq!(header_id(&delivery_text(&quoting, &[])), Some(13));
        // No header, or one cut short of its id: no message.
        assert_eq!(header_id("Understood."), None);
        assert_eq!(header_id("see [ConsensFlow m-x"), None);
        assert_eq!(header_id(&marker_of(7)), Some(7));
        // What the marker proves arrived is the same header.
        assert!(text.contains(&marker_of(12)));
    }

    #[test]
    fn a_message_that_carries_others_goes_whole_by_the_words_of_all_of_them() {
        let carrier = message(9, Some(1), "x".repeat(8_000));
        let carried = [message(8, Some(1), "y".repeat(8_000))];
        // Its own words are short of the limit; with the row it carries, and the
        // line over the row, they are not.
        assert!(goes_whole(&carrier, &[]));
        assert!(!goes_whole(&carrier, &carried));
        assert!(delivery_text(&carrier, &carried).contains("cf task get T-1"));
    }
}
