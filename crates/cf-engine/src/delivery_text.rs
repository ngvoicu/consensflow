//! How a message reads in its recipient's pane. Its header doubles as the proof
//! that it arrived: a delivery counts once the window's own record shows the
//! header's start ([`marker_of`]), so the two are written together. A message
//! that carries others (what its window kept for it) reads as one paste: their
//! words and its own, in the order of their ids, under the one header, which is
//! the one marker that proves the paste arrived.

use std::borrow::Cow;

use cf_base::js;
use cf_base::text::{utf16_len, window_text};
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

/// What a member's fresh window is given ahead of its first message where that
/// is not its task's brief (a reopening, an answer): the brief it received, as
/// its paste said it, and a blank line (`launch_text`).
pub fn brief_first(brief: &MessageView, carried: &[MessageView]) -> String {
    format!("{}\n\n", delivery_text(brief, carried))
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

/// Whether `text`, an item of what a window was given, is the paste of the
/// message `message_id`, which proves it arrived: the message's own header begins
/// it, as ConsensFlow pastes it, the lines of the window's text
/// (`window_text`). So it does where the record holds the paste as its harness
/// writes one:
/// - inside Claude Code's wrapper ([`PASTED`]), the tag opened with its attributes
///   on a line of its own and closed with the same at the end (the reviews of
///   2026-10-16: a Claude chief's record of every result was taken for lacking, and
///   the result pasted again); the four others record the paste as it is given;
/// - behind the placeholders of the images Claude Code attached ([`IMAGE`]: a result
///   that starts with the path of a picture, the field bug of 2026-10-21, pasted three
///   times and failed), the header at once, or the wrapper after a blank line;
/// - after `first`, the exact paste a member's fresh window is given ahead of its
///   first message ([`brief_first`]), there and nowhere else.
///
/// A header quoted in other words, the human's (`Why is "[ConsensFlow m-42 …]"
/// queued?`) or a message's (`Investigate this:\n\n[ConsensFlow m-42 …]`), is no
/// paste, and proves nothing arrived (the reviews of 2026-10-16), whether words come
/// before it or only placeholders do. Limits: words that begin with a header as a paste
/// does are taken for one, and a paste Devin glued behind words the human typed and never
/// sent (it pastes behind what its input box holds, a record of 2026-10-02) is none: the
/// window waits for those words to be sent first (`Held::Unsent`), so only a typing
/// between that look and the paste can make one. And a brief that `first` holds, with
/// lines that are paths of images, is not what the record has behind its placeholders (those
/// lines are gone from it), so a second message behind it is not found.
pub fn is_paste_of(text: &str, message_id: i64, first: Option<&str>) -> bool {
    let marker = marker_of(message_id);
    if !text.contains(&marker) {
        return false;
    }
    let text = window_text(text);
    let text = text.trim_start();
    // Behind the placeholders of the images Claude attached, the header at once or the
    // wrapper after a blank line, as Claude writes them and no other way.
    let paste = match after_placeholders(text) {
        Some(head) => head.strip_prefix("\n\n").map_or(head, unwrapped),
        None => unwrapped(text),
    };
    let behind = first.and_then(|first| paste.strip_prefix(window_text(first).as_str()));
    paste.starts_with(&marker) || behind.is_some_and(|rest| rest.starts_with(&marker))
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

/// The paste `text` holds where Claude Code wrapped it ([`PASTED`]), else `text`.
fn unwrapped(text: &str) -> &str {
    let Some((attributes, inner)) = after_wrapper_opening(text) else {
        return text;
    };
    inner
        .trim_end()
        .strip_suffix(&format!("</{PASTED}{attributes}>"))
        .unwrap_or(text)
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
    fn a_paste_is_an_item_its_header_begins_and_no_words_that_quote_it() {
        let said = message(12, Some(3), "Look at what m-5 said".to_owned());
        let text = delivery_text(&said, &[]);
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
        let quoting = |id: i64, body: String| delivery_text(&message(id, Some(3), body), &[]);
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
        let text = delivery_text(&message(1, Some(1), "The page is done".to_owned()), &[]);
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
        // And behind the brief a fresh window was given first, the placeholders ahead of it.
        let first = brief_first(&message(5, Some(3), "Write the parser".to_owned()), &[]);
        let answer = delivery_text(&message(12, Some(3), "Use JSON".to_owned()), &[]);
        assert!(is_paste_of(
            &format!("[Image #1]{first}{answer}"),
            12,
            Some(&first)
        ));
        assert!(!is_paste_of(
            &format!("[Image #1]{first}{answer}"),
            12,
            None
        ));
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
        let first = brief_first(&brief, &[]);
        let answer = delivery_text(&message(12, Some(3), "Use JSON".to_owned()), &[]);
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
        let other = brief_first(&message(6, Some(3), "Write the lexer".to_owned()), &[]);
        assert!(!is_paste_of(&launched, 12, Some(&other)));
        // Nor in a paragraph of the brief's own words that quotes it.
        let quoting = message(5, Some(3), format!("Write the parser\n\n{answer}"));
        let only_the_brief = delivery_text(&quoting, &[]);
        assert!(!is_paste_of(
            &only_the_brief,
            12,
            Some(&brief_first(&quoting, &[]))
        ));
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
