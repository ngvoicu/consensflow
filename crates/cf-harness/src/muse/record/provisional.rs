//! What a Muse log may say that no live log has shown. **Provisional (the binary's
//! strings only):** each shape below is read from a name in Muse 1.4.13's binary, never
//! from a record of a real session, and is replaced when a recording shows it:
//! - a refusal for quota (MUS-14, MUS-23: T-770's): a failed turn whose reason is an
//!   API error 429 or 402, or Muse's own "usage limit reached";
//! - `task_backgrounded`, the binary's word for a task put in the background, which no
//!   live log has written (Muse 1.4.13 says `execution_state: background_running` in the
//!   task's output instead: `work`).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use crate::shared::quota::Quota;

/// What the provisional shapes said so far.
#[derive(Default)]
pub(super) struct Provisional {
    /// The refusal for quota each run ended with.
    refusals: HashMap<Arc<str>, Arc<Quota>>,
}

impl Provisional {
    /// A run that failed for `reason`, at `at`: a refusal for quota where its words are
    /// one of Muse's limits. Provisional (MUS-14): the 402 below is the one refusal
    /// seen (the Mac's billing, T-700); a spent allowance's words were never seen.
    pub(super) fn failed(&mut self, run: &Arc<str>, reason: Option<&str>, at: &Value) {
        if reason.is_some_and(refused_for_quota) {
            let refusal = Quota::Exhausted {
                at: at.as_str().map(str::to_owned),
                resets_at: None,
            };
            self.refusals.insert(Arc::clone(run), Arc::new(refusal));
        }
    }

    /// The refusal for quota the run `run` ended with.
    pub(super) fn quota(&self, run: &str) -> Option<Arc<Quota>> {
        self.refusals.get(run).cloned()
    }
}

/// Muse's words for a refused request that its account's limits refused: an API error
/// 429 (a rate or usage limit) or 402 (credit or billing), or its own "usage limit
/// reached". Provisional (MUS-14).
fn refused_for_quota(reason: &str) -> bool {
    let reason = reason.to_ascii_lowercase();
    ["api error 429", "api error 402", "usage limit reached"]
        .iter()
        .any(|words| reason.contains(words))
}
