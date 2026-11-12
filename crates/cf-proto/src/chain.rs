//! What a chain's paste says (RS-25): ConsensFlow's own notes that wait for a chief together
//! go into its window as one message, a line each. Written here, once, for the two that need
//! it: the engine's paste (`cf-engine`'s `delivery_text`) and the ledger's gathering of the
//! chain (`cf-ledger`'s `gather_chain`), which measures each line as the paste will say it, so
//! that a chain it gathers is never longer than a window is given whole (T-1500's finding 1).

use crate::cf_word::CfWord;
use crate::ledger::MessageView;

/// The line a chain's paste begins with, for `count` notes.
pub fn head(count: usize) -> String {
    format!("{count} notes at once, a line each:")
}

/// The line of `note` in a chain's paste: its id and its task, then its words, each command
/// ConsensFlow wrote started by `cf`, the window's own (a note no participant wrote; any
/// other's words as they are); its further lines indented under it.
pub fn line(note: &MessageView, cf: &CfWord) -> String {
    let task = note
        .task_number
        .map_or_else(String::new, |number| format!(" · T-{number}"));
    let body = if note.sender.is_none() {
        cf.in_text(&note.body)
    } else {
        std::borrow::Cow::Borrowed(note.body.as_str())
    };
    format!("m-{}{task}: {}", note.id, body.replace('\n', "\n  "))
}

/// What a chain's paste says of `notes`, in the order given: [`head`], then a [`line`] each.
pub fn words(notes: &[&MessageView], cf: &CfWord) -> String {
    let lines: Vec<String> = notes.iter().map(|note| line(note, cf)).collect();
    format!("{}\n{}", head(notes.len()), lines.join("\n"))
}
