//! A window's permission mode, and what it does to a tool call, as Devin 3000.11.92
//! showed it (`cargo xtask live devin-resume`, 2026-10-15; the field report B1 of the
//! same day). A window opened with `--permission-mode dangerous` is in full permission
//! (`bypass`), and one opened without in the `normal` mode, except a session resumed
//! after the app's quit cut its first turn, which comes back in the `normal` mode
//! whatever the flag says. In that mode every call asks: the turn of a prompt file has
//! each refused at once, "Tool execution was rejected: User skipped this tool call",
//! and a pasted turn's opens a dialog that waits for a key, Enter for its first option
//! (approve once) and Escape for none ("Canceled due to user interrupt"). Devin's own
//! command `/bypass` switches the window to full permission, and runs the prompt after
//! it, if it has one, under it: on its own it makes no turn and nothing in the store.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// Devin's command that switches a window to full permission.
pub const BYPASS: &str = "/bypass";

/// What a call refused in the turn of a prompt file comes back with.
pub const SKIPPED: &str = "Tool execution was rejected: User skipped this tool call";

/// What a call whose dialog was left with Escape comes back with.
pub const CANCELED: &str = "Canceled due to user interrupt";

/// What the window shows when it switched.
pub const SWITCHED: &str = "✓ Switched to Bypass mode";

/// The dialog of a call, as Devin draws it under the call.
pub const DIALOG: &str = "❭ 1 Yes  (Approve once)\n2 Yes, switch to accept edits mode\n3 Yes, switch to bypass mode\n4 Edit command\n5 Describe change to command\n6 No\n↑↓ select · ↵ confirm · esc cancel\n";

/// What a submitted text asks of `/bypass`: none where it is no such command, and the
/// prompt after it (empty where it has none) where it is.
pub fn command(text: &str) -> Option<&str> {
    let rest = text.trim_start().strip_prefix(BYPASS)?;
    if rest.is_empty() {
        return Some("");
    }
    rest.strip_prefix(char::is_whitespace).map(str::trim_start)
}

/// How a dialog was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Enter: its first option, approve once.
    Approved,
    /// Escape.
    Cancelled,
}

/// What became of a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    Runs,
    /// Refused at once, in the turn of a prompt file.
    Skipped,
    /// Its dialog was left with Escape.
    Cancelled,
}

#[derive(Debug, Default)]
struct State {
    bypass: bool,
    asking: bool,
    answer: Option<Answer>,
}

/// The mode of a window, and the dialog of the call that waits on one: written by the
/// turn's thread and answered by the one that reads the terminal.
#[derive(Debug, Default)]
pub struct Permission {
    state: Mutex<State>,
    answered: Condvar,
}

impl Permission {
    /// A window in full permission where `bypass`, else in the normal mode.
    pub fn new(bypass: bool) -> Self {
        Self {
            state: Mutex::new(State {
                bypass,
                ..State::default()
            }),
            answered: Condvar::new(),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The mode, as Devin's wire names it.
    pub fn mode(&self) -> &'static str {
        if self.state().bypass {
            "bypass"
        } else {
            "normal"
        }
    }

    /// Switches to full permission: whether that was a change.
    pub fn to_bypass(&self) -> bool {
        let mut state = self.state();
        let changed = !state.bypass;
        state.bypass = true;
        changed
    }

    /// Whether a dialog waits for a key: for the cases that press one.
    #[cfg(test)]
    pub fn asking(&self) -> bool {
        self.state().asking
    }

    /// What becomes of a call of the turn, a prompt file's where `prompt_file`: it runs in
    /// full permission; in the normal mode it is refused in a prompt file's turn, and asks
    /// in any other, waiting for the key that answers its dialog (`ask` draws it first).
    pub fn call(&self, prompt_file: bool, ask: impl FnOnce()) -> Call {
        if self.state().bypass {
            return Call::Runs;
        }
        if prompt_file {
            return Call::Skipped;
        }
        {
            let mut state = self.state();
            state.asking = true;
            state.answer = None;
        }
        ask();
        let mut state = self
            .answered
            .wait_while(self.state(), |state| state.answer.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        state.asking = false;
        match state.answer.take() {
            Some(Answer::Approved) => Call::Runs,
            _ => Call::Cancelled,
        }
    }

    /// A key the window was given: whether a dialog took it as its answer.
    pub fn answer(&self, answer: Answer) -> bool {
        let mut state = self.state();
        if !state.asking || state.answer.is_some() {
            return false;
        }
        state.answer = Some(answer);
        self.answered.notify_all();
        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn bypass_alone_or_before_a_prompt_is_the_command_and_nothing_else_is() {
        assert_eq!(command("/bypass"), Some(""));
        assert_eq!(command("/bypass  go on\nnow"), Some("go on\nnow"));
        assert_eq!(
            command("  /bypass\n[ConsensFlow m-2]"),
            Some("[ConsensFlow m-2]")
        );
        assert_eq!(command("/bypassed"), None);
        assert_eq!(command("say /bypass"), None);
        assert_eq!(command("Reply with exactly: OK"), None);
    }

    #[test]
    fn a_call_runs_in_full_permission_and_is_refused_in_a_prompt_files_turn_in_the_normal_mode() {
        let normal = Permission::new(false);
        assert_eq!(normal.mode(), "normal");
        assert_eq!(normal.call(true, || panic!("no dialog")), Call::Skipped);
        assert!(normal.to_bypass());
        assert!(!normal.to_bypass(), "no change the second time");
        assert_eq!(normal.mode(), "bypass");
        assert_eq!(normal.call(false, || panic!("no dialog")), Call::Runs);
    }

    #[test]
    fn a_pasted_turns_call_waits_at_its_dialog_for_enter_or_escape() {
        let permission = Arc::new(Permission::new(false));
        assert!(!permission.answer(Answer::Approved), "no dialog waits");
        for (key, call) in [
            (Answer::Approved, Call::Runs),
            (Answer::Cancelled, Call::Cancelled),
        ] {
            let presser = {
                let permission = Arc::clone(&permission);
                thread::spawn(move || {
                    while !permission.asking() {
                        thread::sleep(Duration::from_millis(5));
                    }
                    assert!(permission.answer(key));
                })
            };
            let mut drawn = false;
            assert_eq!(permission.call(false, || drawn = true), call);
            assert!(drawn, "the dialog is drawn before it waits");
            presser.join().unwrap();
            assert!(!permission.asking());
        }
    }
}
