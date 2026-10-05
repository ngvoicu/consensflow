//! What the chief's earlier conversations say, as the entries a page is made
//! of: each item rendered, a delivery as one line with its outcome.

use cf_base::js;
use cf_base::refusal::Refusal;
use cf_proto::ledger::{ChiefConversation, HistoryItem};

use super::text::{defuse, delivered, first_line, name_of, Shown};
use super::{is_handoff, Lookup};

/// Past this a number is no id the ledger could hold: 2^63.
const BEYOND_IDS: f64 = 9_223_372_036_854_775_808.0;

/// Entries in the order they were said: a heading for each conversation, and
/// each of its items that is shown. With `find`, no headings, and only the
/// entries that contain it, each after a line saying where it is from.
pub(super) fn entries_of(
    conversations: &[ChiefConversation],
    message: Lookup<'_>,
    tools: bool,
    find: Option<&str>,
) -> Result<Vec<String>, Refusal> {
    let mut entries = Vec::new();
    let needle = find.map(str::to_lowercase);
    for conversation in conversations {
        let view = &conversation.conversation;
        let harness = name_of(&view.harness);
        let when = format!("{} to {}", view.started_at, Shown(&view.ended_at));
        let left = if tools {
            0
        } else {
            conversation
                .items
                .iter()
                .filter(|item| item.role == "tool")
                .count()
        };
        if needle.is_none() {
            let outputs = if left == 1 { "output" } else { "outputs" };
            let left_out = if left > 0 {
                format!("; {left} tool {outputs} left out (--tools)")
            } else {
                String::new()
            };
            entries.push(format!("── The chief on {harness}, {when}{left_out} ──"));
        }
        for item in &conversation.items {
            let Some(shown) = render(item, &view.harness, message, tools)? else {
                continue;
            };
            match &needle {
                None => entries.push(shown),
                Some(needle) if shown.to_lowercase().contains(needle.as_str()) => {
                    let at = item.at.as_deref().unwrap_or(&when);
                    entries.push(format!("({harness}, {at})\n{shown}"));
                }
                Some(_) => {}
            }
        }
    }
    Ok(entries)
}

/// One item of a conversation as a page shows it, or none when it is left out.
fn render(
    item: &HistoryItem,
    harness: &str,
    message: Lookup<'_>,
    tools: bool,
) -> Result<Option<String>, Refusal> {
    let text = &item.text;
    if item.role == "tool" {
        return Ok(tools.then(|| format!("Tool output:\n{}", defuse(text))));
    }
    if item.role == "assistant" {
        return Ok(Some(format!(
            "{} chief: {}",
            name_of(harness),
            defuse(text)
        )));
    }
    let Some(header) = delivered(text) else {
        let who = if item.role == "user" {
            "Human"
        } else {
            "In the window"
        };
        return Ok(Some(format!("{who}: {}", defuse(text))));
    };
    // What the human had typed before a delivery went in with it.
    let before = js::trim(&text[..header.at]);
    let outcome = delivery_line(header.id, message)?;
    Ok(Some(if before.is_empty() {
        outcome
    } else {
        format!("Human: {}\n{outcome}", defuse(before))
    }))
}

/// A delivery as one line: what it was, and what came of it. `digits` are the
/// id the header names, read as `Number(digits)` was.
fn delivery_line(digits: &str, message: Lookup<'_>) -> Result<String, Refusal> {
    let number = js::number(digits);
    let id = js::number_text(number);
    // No other number is the id of a message, and none of them has a line.
    let ledger_id = (number.fract() == 0.0 && number < BEYOND_IDS).then_some(number as i64);
    let found = match ledger_id {
        Some(ledger_id) => message(ledger_id)?,
        None => None,
    };
    let Some(m) = found else {
        return Ok(format!(
            "· m-{id}: a message ConsensFlow delivered (no longer on record)"
        ));
    };
    let from = m
        .sender
        .as_ref()
        .map_or_else(|| "ConsensFlow".to_owned(), |sender| format!("@{sender}"));
    let on = m
        .task_number
        .map_or_else(String::new, |number| format!(" on T-{number}"));
    let task = Shown(&m.task_number);
    let gist = || defuse(&first_line(&m.body, 160));
    Ok(match m.kind.as_str() {
        "result" => format!("· m-{id}: {from}'s result{on} (cf task get T-{task})"),
        "question" => {
            format!(
                "· m-{id}: {from} asked{on}: \"{}\" (cf inbox read m-{id})",
                gist()
            )
        }
        "task" => format!("· m-{id}: {from} gave the chief T-{task}: \"{}\"", gist()),
        "answer" => format!("· m-{id}: {from} answered{on}: \"{}\"", gist()),
        _ if is_handoff(&m) => format!("· m-{id}: the handoff that brought this chief in"),
        _ => format!(
            "· m-{id}: a note from {from}{on}: \"{}\" (cf inbox read m-{id})",
            gist()
        ),
    })
}

#[cfg(test)]
mod tests {
    use cf_proto::ledger::MessageView;

    use super::*;

    #[test]
    fn an_id_no_message_can_have_names_no_message() {
        let nothing =
            |_: i64| -> Result<Option<MessageView>, Refusal> { panic!("no id to ask for") };
        for digits in ["99999999999999999999", "9223372036854775808"] {
            let line = delivery_line(digits, &nothing).unwrap();
            assert!(line.ends_with("(no longer on record)"), "{line}");
        }
        assert_eq!(
            delivery_line("1000000000000000000000", &nothing).unwrap(),
            "· m-1e+21: a message ConsensFlow delivered (no longer on record)"
        );
        assert_eq!(
            delivery_line(&"9".repeat(400), &nothing).unwrap(),
            "· m-Infinity: a message ConsensFlow delivered (no longer on record)"
        );
    }
}
