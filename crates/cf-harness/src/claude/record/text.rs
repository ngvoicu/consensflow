//! What Claude Code's records say as text: a message's, a tool's, the
//! interrupt that ends a turn, and whether a removal names a queued message.

use std::borrow::Cow;

use cf_base::js;
use serde_json::Value;

use super::patterns::ENVELOPE;
use super::{field, kind};
use crate::shared::record::reading::visible_text;

/// What Claude Code writes as the user's turn when its own prompt is cut short.
const INTERRUPT_MARKERS: [&str; 2] = [
    "[Request interrupted by user]",
    "[Request interrupted by user for tool use]",
];

/// `claudeText`: a message's text, from text, or from a list of blocks whose
/// text blocks are joined a line each; none for content that is no list.
pub(super) fn claude_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|block| kind(block) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// `claudeToolText`: what a tool returned. Text is itself; a list of text
/// blocks is each block's text (`String(block.text ?? '')`, which V8 fails
/// on an object with a `toString` of its own) a line each; anything else is
/// what `visibleText` shows of it.
pub(super) fn claude_tool_text(content: Option<&Value>) -> Result<String, String> {
    match content {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Array(blocks)) if blocks.iter().all(|block| kind(block) == Some("text")) => {
            let texts = blocks
                .iter()
                .map(|block| match block.get("text") {
                    None | Some(Value::Null) => Ok(String::new()),
                    text => js::string(text).map(Cow::into_owned),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(texts.join("\n"))
        }
        other => Ok(visible_text(other)),
    }
}

/// `isClaudeInterrupt`: whether a user record is Claude Code's own record of
/// an interrupt: a list of one text block, which says one of its two markers
/// and nothing else. What the user types is text, never such a list.
///
/// Departs from Node's reader on purpose (Node's daemon keeps its rules, by
/// the owner's decision of 2026-10-07): Node asked the record to name the
/// message it interrupted (`interruptedMessageId`) as well. Claude Code
/// 2.1.292 names none when the interrupt comes while a Stop hook runs, after
/// its answer was written and no message is left to name; read as the user's
/// turn, that record began a turn which never ended, and the window never
/// read at rest.
pub(super) fn is_interrupt(record: &Value) -> bool {
    let message = record.get("message");
    if field(message, "role").and_then(Value::as_str) != Some("user") {
        return false;
    }
    let Some(Value::Array(content)) = field(message, "content") else {
        return false;
    };
    let [only] = content.as_slice() else {
        return false;
    };
    kind(only) == Some("text")
        && only
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| INTERRUPT_MARKERS.contains(&text))
}

/// `sameQueuedContent`: whether a queue removal names the same queued
/// message. Claude Code wraps cross-session messages in an envelope tag whose
/// attributes differ between the enqueue and the remove record (2.1.275 adds
/// `hop-chain` to one only), so the envelope's attributes are set aside after
/// an exact match fails.
pub(super) fn same_queued_content(queued: &str, removed: &str) -> bool {
    queued == removed || envelope(queued) == envelope(removed)
}

/// `content` with the attributes of its opening tag left out, when it opens
/// on an envelope.
fn envelope(content: &str) -> Cow<'_, str> {
    ENVELOPE.replace(content, "<${1}>")
}
