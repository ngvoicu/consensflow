//! Devin's own word that its TUI took a paste: the row its store's `prompt_history`
//! gains when the paste's Enter submits it, whole, a row of the session it was typed
//! into. Devin 3000.11.92 writes it at the Enter, a paste queued behind a running turn
//! too, seconds or minutes before its record shows it (that waits for the turn to take
//! the paste in); Devin 3000.11.3, 3000.10.31 and 3000.6.14 write it only when the turn
//! takes it in, as late as the record (probed live, T-102, 2026-10-21). So it is read
//! from 3000.11.92 on ([`AT_ENTER`]), and an older Devin's window says nothing of it.
//!
//! The store is read read-only, as the record reader reads it, and never waited on: a
//! look that meets its writer's lock answers what the last look that could read it
//! answered (`Seen`).

use std::cell::Cell;
use std::path::Path;

use cf_base::env::Env;
use cf_base::js;
use cf_base::text::window_text;

use super::paths;
use crate::records::{Item, Role};
use crate::shared::record::sqlite::{self, Cell as Value};

/// The first Devin that writes a prompt's history row at its Enter.
pub(super) const AT_ENTER: [u32; 3] = [3000, 11, 92];

/// What the store said of a paste at the last look that could read it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Seen {
    /// The session has a history row of the paste, written since it was handed over.
    pub(super) took: bool,
    /// How many history rows the session has: one more since a look is a prompt the
    /// human (or anyone) submitted since it.
    pub(super) prompts: u64,
}

/// The paste a window was handed last, and what its store said of it.
#[derive(Default)]
pub(super) struct History {
    /// The paste, as the window was given it, and the second (Devin's clock: the
    /// machine's) it was handed over in.
    pasted: std::cell::RefCell<Option<(String, i64)>>,
    seen: Cell<Seen>,
}

impl History {
    /// `text` is about to be pasted, at `at_ms` on the machine's clock: a history row
    /// of the session whose content begins with it whole, written in that second or
    /// later, is its own. A row a second earlier counts too, for a clock that turns
    /// between the two: an earlier attempt of the same message went at least an arrival
    /// window before.
    pub(super) fn expect(&self, text: &str, at_ms: i64) {
        let text = js::trim(text).to_owned();
        *self.pasted.borrow_mut() = (!text.is_empty()).then(|| (text, at_ms.div_euclid(1000) - 1));
        let seen = self.seen.get();
        self.seen.set(Seen {
            took: false,
            ..seen
        });
    }

    /// Whether `items`, a record's, hold the paste: a user's item that begins with it.
    pub(super) fn recorded_in(&self, items: &[Item]) -> bool {
        self.pasted.borrow().as_ref().is_some_and(|(text, _)| {
            items.iter().any(|item| {
                item.role == Role::User
                    && window_text(&item.text)
                        .trim_start()
                        .starts_with(text.as_str())
            })
        })
    }

    /// What the store of `env` says now of the paste, in `session`; where it cannot
    /// be read, what it said last.
    pub(super) fn look(&self, env: &Env, session: &str) -> Seen {
        let pasted = self.pasted.borrow().clone();
        match read(env, session, pasted.as_ref()) {
            Ok(seen) => {
                // The store's word that it took the paste is never taken back.
                let took = seen.took || (pasted.is_some() && self.seen.get().took);
                let seen = Seen { took, ..seen };
                self.seen.set(seen);
                seen
            }
            Err(_) => self.seen.get(),
        }
    }
}

/// What Devin's store of `env` says of `session`: how many history rows it has, and
/// whether one is `pasted`'s.
fn read(env: &Env, session: &str, pasted: Option<&(String, i64)>) -> Result<Seen, String> {
    let file = paths::store(env)?;
    let store = sqlite::Store::open(Path::new(&file))?;
    store.read(|reads| {
        let rows = reads.all(
            "select content, timestamp from prompt_history where session_id = ? and is_shell = 0",
            [session],
        )?;
        let took = pasted.is_some_and(|(text, since)| {
            rows.iter().any(|row| {
                let after =
                    matches!(row.get("timestamp"), Some(Value::Number(at)) if *at >= *since as f64);
                let whole = matches!(row.get("content"), Some(Value::Text(content))
                    if window_text(content).trim_start().starts_with(text.as_str()));
                after && whole
            })
        });
        Ok(Seen {
            took,
            prompts: rows.len() as u64,
        })
    })
}

/// Whether Devin of `version`, what it says to `--version`, writes a prompt's history
/// row at its Enter ([`AT_ENTER`]).
pub(super) fn at_enter(version: &str) -> bool {
    super::install::at_least(version, AT_ENTER)
}
