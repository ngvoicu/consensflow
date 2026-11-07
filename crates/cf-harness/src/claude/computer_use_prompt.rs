//! Claude Code's Computer Use permission prompt in a member's window (T-1300).
//!
//! A member that calls the built-in `computer-use` server's `request_access` while the app that
//! runs its window lacks macOS Accessibility or Screen Recording is shown this, and the turn waits
//! for a key (2.1.319; the field, 2026-11-07, T-1291's window, ConsensFlow 3.0.7-alpha.2):
//!
//! ```text
//! Computer Use needs macOS permissions
//! Accessibility: ✔ granted
//! Screen Recording: ✘ not granted
//! Grant the missing permissions in System Settings, then select "Try again". macOS may require
//! you to restart Claude Code after granting Screen Recording.
//! ❯ Open System Settings → Screen Recording
//!   Try again
//! ```
//!
//! Nobody answers it in an unwatched window, and the chief's `cf tell` waits behind it for ever.
//! Neither listed choice can be pressed: "Open System Settings → …" asks macOS for the permission
//! in ConsensFlow's name (`tcc.requestScreenRecording()`) and opens System Settings, and "Try
//! again" cannot succeed, since a Screen Recording grant takes effect only when the app restarts,
//! which stops every window. Claude Code's Select takes a digit for the choice of that number
//! whether or not it draws the numbers (`hideIndexes` hides them, the `/^[0-9]$/` branch of its
//! key handler stays), so a digit here is one of those two choices as well.
//!
//! **Escape is the one key.** The Select binds Escape to `select:cancel`, which calls the dialog's
//! `onCancel`, which resolves the request with nothing granted and nothing denied (`granted:[]`,
//! `denied:[]`): no configuration is written. The tool then answers the model that "The user saw
//! the permission prompt but macOS … permission(s) are still not granted. Do not retry in this
//! turn. Let the user know these permissions need to be granted in the Claude desktop app …", an
//! ordinary tool result: the turn goes on by itself (nothing aborts it, unlike the "No" of a hook's
//! ask). Claude Code ignores a key within 150 ms of the prompt opening and of a refused key; the
//! engine presses seconds after it drew it.
//!
//! The model's words name the Claude desktop app, which is not what ConsensFlow's member needs
//! to do, so the member is also told what is so here ([`Decline::then_tell`]), once its window
//! rests.
//!
//! The prompt is known by what Claude Code draws of it and nothing else: its title, in the rows
//! above its choices, and its choices, which are some of the three it can offer, in its order,
//! "Try again" last. A dialog that is not drawn with those exact choices is another dialog, and a
//! screen that only quotes the words (a command's output, a file shown) has no cursor on its
//! choices or has the harness's own prompt drawn below them.

use crate::contract::{Decline, Dialog};

/// The title the prompt opens with, a row of its own.
const TITLE: &str = "Computer Use needs macOS permissions";

/// The choices the prompt can offer (the first two only for a permission that is missing), in its
/// order, and the one it always ends with.
const OPEN_ACCESSIBILITY: &str = "Open System Settings \u{2192} Accessibility";
const OPEN_SCREEN_RECORDING: &str = "Open System Settings \u{2192} Screen Recording";
const TRY_AGAIN: &str = "Try again";

/// The key that closes it: Escape.
const ESCAPE: u8 = 27;

/// Which permissions the prompt says are missing, from the choices it offers to open System
/// Settings at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Missing {
    accessibility: bool,
    screen_recording: bool,
}

/// What `dialog`'s choices say is missing: none unless they are exactly some of the three the
/// prompt offers, in its order, "Try again" last and at least one permission before it.
fn missing(dialog: &Dialog) -> Option<Missing> {
    let options: Vec<&str> = dialog.options.iter().map(String::as_str).collect();
    match options.as_slice() {
        [OPEN_ACCESSIBILITY, TRY_AGAIN] => Some(Missing {
            accessibility: true,
            screen_recording: false,
        }),
        [OPEN_SCREEN_RECORDING, TRY_AGAIN] => Some(Missing {
            accessibility: false,
            screen_recording: true,
        }),
        [OPEN_ACCESSIBILITY, OPEN_SCREEN_RECORDING, TRY_AGAIN] => Some(Missing {
            accessibility: true,
            screen_recording: true,
        }),
        _ => None,
    }
}

/// The row of `detail` that is the prompt's title, the nearest to its question, where `dialog` is
/// the prompt: its choices are the prompt's, and a row says its title and nothing else.
pub(crate) fn top(dialog: &Dialog) -> Option<usize> {
    missing(dialog)?;
    dialog.detail.iter().rposition(|row| row == TITLE)
}

/// Whether `dialog` is the prompt.
pub(crate) fn is_prompt(dialog: &Dialog) -> bool {
    top(dialog).is_some()
}

/// What the member is told once the prompt is closed: that computer use is not available in a
/// ConsensFlow window, what it lacks, and to go on without it.
fn then_tell(missing: Missing) -> String {
    let lacks = match (missing.accessibility, missing.screen_recording) {
        (true, true) => "the Accessibility and Screen Recording permissions and has been restarted",
        (false, true) => "the Screen Recording permission and has been restarted",
        _ => "the Accessibility permission",
    };
    format!(
        "Computer use isn't available in ConsensFlow's windows until ConsensFlow has {lacks}. Do the task without it."
    )
}

/// The decline of `dialog` where it is the prompt and the window is a member's: Escape, in a
/// window nobody watches, once the note of it has gone. Not in the chief's window, which is the
/// human's ([`Decline::members_only`]). Not where the harness's own prompt is drawn below the
/// choices: they are then words of the conversation, not a dialog that waits.
pub(crate) fn decline(dialog: &Dialog) -> Option<Decline> {
    if dialog.prompt_below {
        return None;
    }
    let lacking = missing(dialog)?;
    top(dialog)?;
    Some(Decline {
        choice: "Esc".to_owned(),
        key: vec![ESCAPE],
        after_ms: None,
        then_tell: Some(then_tell(lacking)),
        what: "Computer Use permission prompt".to_owned(),
        listed: false,
        members_only: true,
    })
}

#[cfg(test)]
mod tests;
