//! What a Muse log may say that no live log has shown. **Provisional (the binary's
//! strings only):** each shape below is read from a name in Muse 1.4.13's binary, never
//! from a record of a real session, and is replaced when a recording shows it. The quota
//! rule is here; the others are read where their live neighbours are, and listed here:
//! - a refusal by the account (MUS-14, MUS-23): a failed turn whose reason is an API
//!   error 429 or Muse's own "usage limit reached" (a limit that resets), a 402 or 401
//!   or the words every harness shares for billing and the login (the account
//!   unavailable, with the plain words of its fix), or the wire's own `quota` beside the
//!   error (its reset);
//! - `task_backgrounded` (`record`), the binary's word for a task put in the background,
//!   which no live log has written (Muse 1.4.13 says `execution_state:
//!   background_running` in the task's output: `work`);
//! - a child's status that says it ended (`work`'s `CHILD_ENDED`): live, a child ends by
//!   its result;
//! - a goal still open when its run ends, and one the user paused or cleared (`work`):
//!   live, the goal closed within its run;
//! - a message handed to the session of another kind or surface than a chat of the main
//!   one (`record`'s `intent`);
//! - a `/model` the human typed (`record`'s `asks_for_a_model`): the commands seen live
//!   are `/clear` and `/compact`.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use super::super::usage::wire_quota;
use super::field;
use crate::shared::quota::{account_refusal, Quota, Refusal};

/// What the provisional shapes said so far.
#[derive(Default)]
pub(super) struct Provisional {
    /// The refusal for quota each run ended with.
    refusals: HashMap<Arc<str>, Arc<Quota>>,
}

impl Provisional {
    /// A run that failed for `reason`, at `at`: what the account refused for, where its
    /// words say it, and the plain words for the failure it is. A usage limit is a quota
    /// that resets (provisional, MUS-14: only the 429 shape and Muse's own "usage limit
    /// reached" are known, a subscribed account has not shown one); a refusal for billing
    /// or for the login is the account unavailable, with no reset, only the human's fix
    /// (MUS-23; the 402 is the refusal seen, T-705).
    pub(super) fn failed(
        &mut self,
        run: &Arc<str>,
        event: Option<&Value>,
        reason: Option<&str>,
        at: &Value,
    ) -> Option<&'static str> {
        let at = at.as_str().map(str::to_owned);
        // The wire's own word on a limit (`error.quota`, `{limitKind, resetsAtMs}`) names its
        // reset, which no words do. Provisional: never seen in a log, only in the schema.
        let detail = field(event, "quota").or_else(|| field(field(event, "error"), "quota"));
        if let Some(refusal) =
            detail.and_then(|detail| wire_quota(detail, at.as_deref()).ok().flatten())
        {
            self.refusals.insert(Arc::clone(run), Arc::new(refusal));
            return None;
        }
        let (refusal, plain) = match refused_for(reason?)? {
            Refused::Limit => (
                Quota::Exhausted {
                    at,
                    resets_at: None,
                },
                None,
            ),
            Refused::Account(Refusal::Billing) => {
                (unavailable(at, NO_ALLOWANCE), Some(NO_ALLOWANCE))
            }
            Refused::Account(Refusal::Credentials) => {
                (unavailable(at, LOGIN_REFUSED), Some(LOGIN_REFUSED))
            }
        };
        self.refusals.insert(Arc::clone(run), Arc::new(refusal));
        plain
    }

    /// The refusal for quota the run `run` ended with.
    pub(super) fn quota(&self, run: &str) -> Option<Arc<Quota>> {
        self.refusals.get(run).cloned()
    }
}

/// A failure of a turn that the account refused: by a limit that resets, or for what
/// only its human cures.
enum Refused {
    Limit,
    Account(Refusal),
}

/// The plain words of a failure for billing (MUS-23): what `muse usage` calls
/// `no_allowance`, where the human fixes it.
const NO_ALLOWANCE: &str = "Muse refused: the account has no allowance; set up billing at accountscenter.meta.com/muse_code";

/// The plain words of a failure for the login.
const LOGIN_REFUSED: &str = "Muse refused: its login is not valid; run `muse login`";

fn unavailable(at: Option<String>, because: &str) -> Quota {
    Quota::Unavailable {
        at,
        because: because.to_owned(),
    }
}

/// What Muse's words for a failed request say the account refused for. A turn's reason
/// is `API error <status> [request_id=…]: <message> (<kind>)` for a refused request
/// (`billing_error`, T-705), or Muse's own words for no login. A 429 or Muse's "usage
/// limit reached" is a limit that resets; a 402 or 401 and the words the other harnesses
/// share (`account_refusal`) are the account's own. Provisional (MUS-14): a spent
/// allowance's words were never seen.
fn refused_for(reason: &str) -> Option<Refused> {
    let lower = reason.to_ascii_lowercase();
    if lower.contains("no meta credentials") || lower.contains("not logged in") {
        return Some(Refused::Account(Refusal::Credentials));
    }
    if let Some(refusal) = account_refusal(reason, api_status(&lower)) {
        return Some(Refused::Account(refusal));
    }
    [
        "api error 429",
        "usage limit reached",
        "usage limit was reached",
    ]
    .iter()
    .any(|words| lower.contains(words))
    .then_some(Refused::Limit)
}

/// The status of `API error <status>`, in a lowercased reason.
fn api_status(lower: &str) -> Option<f64> {
    let after = lower.split("api error ").nth(1)?;
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    (digits.len() == 3).then(|| digits.parse().ok()).flatten()
}
