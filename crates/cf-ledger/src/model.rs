//! The words the ledger's records hold: harnesses, roles, tiers, pools,
//! purposes and the task states it counts by, the limits on text, and the
//! checks that refuse anything else with a stable code.
//!
//! A check comes in two halves where JavaScript had one: the value's own
//! rule (blank, too long, not a tier), which every operation applies to what
//! it is given; and its shape (text at all, a list at all), which only a
//! caller reading JSON has to ask, through the `parse_*` parsers, with the
//! same message JavaScript gave and the offender printed as it printed it.

use cf_base::js;
use cf_base::refusal::Refusal;
use cf_base::text::{utf16_len, utf16_prefix};
use cf_proto::agents::Harness;
use serde_json::Value;

use crate::views::ParticipantRow;

pub const MEMBER_ROLES: [&str; 5] = ["worker", "advisor", "reviewer", "tester", "designer"];
/// Who hands out work and hears when the staff changes: the human and the chief.
pub const COORDINATOR_HANDLES: [&str; 2] = ["human", "chief"];
pub const COORDINATOR_ROLES: [&str; 2] = ["human", "chief"];
pub const TIERS: [&str; 4] = ["critical", "complex", "standard", "light"];
/// Who takes a task on the board: a worker, an advisor (advice), a reviewer, a tester, or an image designer (no tier).
pub const POOLS: [&str; 5] = ["worker", "advisor", "reviewer", "tester", "designer"];
pub const PURPOSES: [&str; 4] = [
    "critical-review",
    "architecture",
    "hard-problem",
    "important-question",
];
pub const ACTIVE_TASK_STATES: [&str; 2] = ["working", "waiting"];
/// A task on a member's hands: from assignment until its result.
pub const HELD_TASK_STATES: [&str; 3] = ["queued", "working", "waiting"];
/// A task that is over, accepted or not: the board's last column, and what the human may delete.
pub const FINISHED_TASK_STATES: [&str; 3] = ["accepted", "cancelled", "failed"];
pub const MAX_BODY: usize = 1_000_000;
pub const MAX_TITLE: usize = 120;

/// Why the ledger did not do what it was asked: a refusal it makes itself,
/// with its stable code, or what SQLite or a stored JSON text said, as they
/// said it.
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("{0}")]
    Refused(Refusal),
    #[error("{0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
}

impl LedgerError {
    /// A refusal with status 400, what a request got wrong.
    pub fn refused(code: &'static str, message: impl Into<String>) -> Self {
        LedgerError::Refused(Refusal::new(code, message))
    }

    /// A refusal with its own status.
    pub fn refused_with(code: &'static str, message: impl Into<String>, status: u16) -> Self {
        LedgerError::Refused(Refusal::with_status(code, message, status))
    }

    /// The refusal's code, when it is one.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            LedgerError::Refused(refusal) => Some(refusal.code),
            _ => None,
        }
    }
}

/// Words as SQL lists them: `'queued', 'working', 'waiting'`.
pub(crate) fn sql_list(words: &[&str]) -> String {
    words
        .iter()
        .map(|word| format!("'{word}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A participant that is still in the project; a member who left is refused.
pub(crate) fn require_active(row: &ParticipantRow) -> Result<(), LedgerError> {
    if row.left_at.is_some() {
        return Err(LedgerError::refused_with(
            "member-left",
            format!("@{} left the staff", row.handle),
            409,
        ));
    }
    Ok(())
}

/// `value` as `JSON.stringify` printed it in a refusal: "undefined" for a value not given.
pub(crate) fn printed(value: Option<&Value>) -> String {
    value.map_or_else(
        || "undefined".to_string(),
        |value| cf_base::json::js_order(value.clone()).to_string(),
    )
}

/// Text that says something and fits: `field` names it in the refusal.
pub fn require_text(value: &str, field: &str, max: usize) -> Result<(), LedgerError> {
    if js::trim(value).is_empty() || utf16_len(value) > max {
        return Err(LedgerError::refused(
            "invalid-text",
            format!("{field} must be text, not empty, at most {max} characters"),
        ));
    }
    Ok(())
}

/// `value` read as text that says something and fits.
pub fn parse_text(value: Option<&Value>, field: &str, max: usize) -> Result<String, LedgerError> {
    let Some(Value::String(text)) = value else {
        return Err(LedgerError::refused(
            "invalid-text",
            format!("{field} must be text, not empty, at most {max} characters"),
        ));
    };
    require_text(text, field, max)?;
    Ok(text.clone())
}

/// One of the harnesses, by its kind (`claude-code`).
pub fn require_harness(harness: &str) -> Result<&'static str, LedgerError> {
    Harness::from_kind(harness)
        .map(Harness::kind)
        .ok_or_else(|| {
            LedgerError::refused(
                "invalid-harness",
                format!("unknown harness {}", quoted(harness)),
            )
        })
}

/// `value` read as one of the harnesses.
pub fn parse_harness(value: Option<&Value>) -> Result<&'static str, LedgerError> {
    match value {
        Some(Value::String(harness)) => require_harness(harness),
        _ => Err(LedgerError::refused(
            "invalid-harness",
            format!("unknown harness {}", printed(value)),
        )),
    }
}

/// A tier.
pub fn require_tier(tier: &str) -> Result<&'static str, LedgerError> {
    TIERS
        .into_iter()
        .find(|known| *known == tier)
        .ok_or_else(|| tier_refused(&quoted(tier)))
}

/// `value` read as a tier.
pub fn parse_tier(value: Option<&Value>) -> Result<&'static str, LedgerError> {
    match value {
        Some(Value::String(tier)) => require_tier(tier),
        _ => Err(tier_refused(&printed(value))),
    }
}

fn tier_refused(offender: &str) -> LedgerError {
    LedgerError::refused(
        "invalid-tier",
        format!("a tier is {}, not {offender}", TIERS.join(", ")),
    )
}

/// Text as `JSON.stringify` printed it in a refusal: quoted and escaped.
pub(crate) fn quoted(text: &str) -> String {
    Value::from(text).to_string()
}

/// `value` read as task numbers a task needs or comes before: distinct
/// positive integers, in the order first given.
pub fn parse_numbers(value: Option<&Value>, field: &str) -> Result<Vec<u64>, LedgerError> {
    let refused = || {
        LedgerError::refused(
            "invalid-needs",
            format!("{field} is a list of task numbers (T-3, T-4)"),
        )
    };
    let Some(Value::Array(items)) = value else {
        return Err(refused());
    };
    let mut numbers = Vec::new();
    for item in items {
        // JavaScript's numbers: 3.0 is the integer 3.
        let number = item
            .as_f64()
            .filter(|n| n.fract() == 0.0 && *n > 0.0 && *n <= 9_007_199_254_740_991.0);
        let number = number.ok_or_else(refused)? as u64;
        if !numbers.contains(&number) {
            numbers.push(number);
        }
    }
    Ok(numbers)
}

/// `value` read as human approval required (true) or not (false).
pub fn parse_gate(value: Option<&Value>) -> Result<bool, LedgerError> {
    value.and_then(Value::as_bool).ok_or_else(|| {
        LedgerError::refused(
            "invalid-gate",
            "human approval is required (true) or not (false)",
        )
    })
}

/// Whether a meeting with the chief is on: a flag, as the page sends it.
pub fn parse_meeting(value: Option<&Value>) -> Result<bool, LedgerError> {
    value.and_then(Value::as_bool).ok_or_else(|| {
        LedgerError::refused(
            "invalid-meeting",
            "a meeting with the chief is on (true) or off (false)",
        )
    })
}

/// A member's roles: one or more of worker, advisor, reviewer, tester and
/// designer, distinct, the first one leading; `offender` is how the
/// refusal prints what was given.
pub fn require_roles<S: AsRef<str>>(
    roles: &[S],
    offender: impl FnOnce() -> String,
) -> Result<Vec<&'static str>, LedgerError> {
    let mut known = Vec::new();
    for role in roles {
        match MEMBER_ROLES
            .into_iter()
            .find(|member| *member == role.as_ref())
        {
            Some(role) if !known.contains(&role) => known.push(role),
            Some(_) => {}
            None => return Err(roles_refused(&offender())),
        }
    }
    if known.is_empty() {
        return Err(roles_refused(&offender()));
    }
    Ok(known)
}

/// `value` read as a member's roles.
pub fn parse_roles(value: Option<&Value>) -> Result<Vec<&'static str>, LedgerError> {
    let Some(Value::Array(items)) = value else {
        return Err(roles_refused(&printed(value)));
    };
    let names: Option<Vec<&str>> = items.iter().map(Value::as_str).collect();
    let names = names.ok_or_else(|| roles_refused(&printed(value)))?;
    require_roles(&names, || printed(value))
}

fn roles_refused(offender: &str) -> LedgerError {
    LedgerError::refused(
        "invalid-role",
        format!(
            "a member is one or more of {}, not {offender}",
            MEMBER_ROLES.join(", ")
        ),
    )
}

/// A model's id as a harness names it, what its `--model` takes: the model a chief runs
/// on (CHF-04). Not an agent's id: Pi's holds its provider and may hold a `/` of
/// OpenRouter's own (`openrouter/anthropic/claude-fable-5`), Claude Code's a context
/// suffix (`[1m]`), OpenRouter's a `~`. In ASCII, 256 bytes at most, no space or control,
/// and never begun by a `-`, which a command line would read as a flag.
pub fn require_model_id(model: &str) -> Result<(), LedgerError> {
    if is_model_id(model) {
        return Ok(());
    }
    Err(LedgerError::refused(
        "invalid-model",
        format!("not a model id: {}", quoted(model)),
    ))
}

/// `value` read as a model's id.
pub fn parse_model_id(value: Option<&Value>) -> Result<String, LedgerError> {
    match value {
        Some(Value::String(model)) => require_model_id(model).map(|()| model.clone()),
        _ => Err(LedgerError::refused(
            "invalid-model",
            format!("not a model id: {}", printed(value)),
        )),
    }
}

/// `value` read as the effort a model runs at, as its harness words it (`high`, `xhigh`):
/// none where it names none (absent or `null`), else 1 to 32 ASCII letters, digits and
/// `-`.
pub fn parse_effort(value: Option<&Value>) -> Result<Option<String>, LedgerError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(effort))
            if (1..=32).contains(&effort.len())
                && effort
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-') =>
        {
            Ok(Some(effort.clone()))
        }
        _ => Err(LedgerError::refused(
            "invalid-effort",
            format!("not an effort: {}", printed(value)),
        )),
    }
}

/// The most characters a model's name holds: the most a model id may have ([`is_model_id`]),
/// which is what the field holds now (`resolve` stores the id its harness's list names the model
/// by, HMD-12), and so more than the 128 of the display names it held before. A row from before
/// holds one of those, which this still takes.
pub const MODEL_NAME_MAX: usize = 256;

/// `value` read as the name a harness's list gives a model, which is its id (`claude-opus-5-5`,
/// Pi's `provider/id`), or, in a row from before, what the list called it (`Opus`): none where
/// it names none, else text of 1 to [`MODEL_NAME_MAX`] characters, no control among them.
pub fn parse_model_name(value: Option<&Value>) -> Result<Option<String>, LedgerError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(name))
            if (1..=MODEL_NAME_MAX).contains(&name.chars().count())
                && !name.trim().is_empty()
                && !name.chars().any(char::is_control) =>
        {
            Ok(Some(name.clone()))
        }
        _ => Err(LedgerError::refused(
            "invalid-model-name",
            format!("not a model's name: {}", printed(value)),
        )),
    }
}

/// `^[A-Za-z0-9~][A-Za-z0-9._~/:@+\[\]-]{0,255}$`, in ASCII.
fn is_model_id(model: &str) -> bool {
    let bytes = model.as_bytes();
    (1..=256).contains(&bytes.len())
        && (bytes[0].is_ascii_alphanumeric() || bytes[0] == b'~')
        && bytes[1..].iter().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'.' | b'_' | b'-' | b'~' | b'/' | b':' | b'@' | b'+' | b'[' | b']'
                )
        })
}

/// Whether a role fits an agent, by its designer flag: an image designer is
/// an image agent (a Codex agent that designs), and an image agent is nothing
/// else, the chief included.
pub fn fits_role(designer: bool, role: &str) -> bool {
    (role == "designer") == designer
}

/// Refuses roles that do not fit the member named `agent`, a designer or not, saying
/// which way.
pub fn require_fitting_roles(
    agent: &str,
    designer: bool,
    roles: &[&str],
) -> Result<(), LedgerError> {
    if roles.iter().all(|role| fits_role(designer, role)) {
        return Ok(());
    }
    Err(LedgerError::refused(
        "invalid-role",
        if designer {
            format!("{agent} is an image agent, which can only be an image designer")
        } else {
            format!("only an image agent can be an image designer, and {agent} is not one")
        },
    ))
}

/// A card title: the first line that says something, shortened to fit.
pub fn title_of(body: &str) -> String {
    let line = body
        .split('\n')
        .map(js::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    if utf16_len(line) <= MAX_TITLE {
        line.to_string()
    } else {
        format!("{}…", utf16_prefix(line, MAX_TITLE - 1))
    }
}

/// A title the chief gives a task: read as a brief's first line is ([`title_of`]), and
/// it must say something.
pub fn checked_title(title: &str) -> Result<String, LedgerError> {
    require_text(title, "title", MAX_BODY)?;
    Ok(title_of(title))
}

/// `text` cut at `max` characters, then a line saying how long it was (at 0,
/// only that line); shorter, as it is.
pub fn cut(text: &str, max: usize) -> String {
    let length = utf16_len(text);
    if length <= max {
        return text.to_string();
    }
    let gap = if max == 0 { "" } else { "\n" };
    format!(
        "{}{gap}… ({length} characters; cut here)",
        utf16_prefix(text, max)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn refusal(
        result: Result<impl std::fmt::Debug, LedgerError>,
    ) -> (Option<&'static str>, String) {
        let error = result.unwrap_err();
        (error.code(), error.to_string())
    }

    #[test]
    fn refuses_blank_or_long_text_counting_as_javascript_counts() {
        assert!(require_text("Fix it", "body", 10).is_ok());
        assert_eq!(
            refusal(require_text(" \n\t", "body", 10)),
            (
                Some("invalid-text"),
                "body must be text, not empty, at most 10 characters".into()
            )
        );
        // An emoji is two of JavaScript's characters.
        assert!(require_text("😀😀", "body", 4).is_ok());
        assert!(require_text("😀😀😀", "body", 5).is_err());
        assert_eq!(
            refusal(parse_text(None, "body", 10)).0,
            Some("invalid-text")
        );
        assert_eq!(
            refusal(parse_text(Some(&json!(3)), "body", 10)).0,
            Some("invalid-text")
        );
    }

    #[test]
    fn prints_the_offender_as_json_stringify_did() {
        assert_eq!(refusal(parse_harness(None)).1, "unknown harness undefined");
        assert_eq!(
            refusal(parse_harness(Some(&json!(null)))).1,
            "unknown harness null"
        );
        assert_eq!(
            refusal(parse_harness(Some(&json!("kimi")))).1,
            r#"unknown harness "kimi""#
        );
        assert_eq!(
            refusal(parse_tier(Some(&json!("huge")))).1,
            r#"a tier is critical, complex, standard, light, not "huge""#
        );
        assert_eq!(
            refusal(parse_roles(Some(&json!(["chief"])))).1,
            r#"a member is one or more of worker, advisor, reviewer, tester, designer, not ["chief"]"#
        );
    }

    #[test]
    fn reads_numbers_and_roles_distinct_in_the_order_first_given() {
        assert_eq!(
            parse_numbers(Some(&json!([4, 3, 4.0])), "needs").unwrap(),
            [4, 3]
        );
        for wrong in [json!(["T-1"]), json!([0]), json!([1.5]), json!(3)] {
            assert_eq!(
                refusal(parse_numbers(Some(&wrong), "needs")),
                (
                    Some("invalid-needs"),
                    "needs is a list of task numbers (T-3, T-4)".into()
                )
            );
        }
        assert_eq!(
            parse_roles(Some(&json!(["reviewer", "worker", "reviewer"]))).unwrap(),
            ["reviewer", "worker"]
        );
        assert_eq!(
            parse_roles(Some(&json!(["tester", "worker", "tester"]))).unwrap(),
            ["tester", "worker"]
        );
        assert!(parse_roles(Some(&json!([]))).is_err());
        assert!(parse_gate(Some(&json!(true))).unwrap());
        assert_eq!(
            refusal(parse_gate(Some(&json!("yes")))).0,
            Some("invalid-gate")
        );
    }

    #[test]
    fn takes_an_image_designer_for_an_image_model_alone() {
        assert!(require_fitting_roles("pygmalion", true, &["designer"]).is_ok());
        assert_eq!(
            refusal(require_fitting_roles("hera", false, &["designer"])).1,
            "only an image agent can be an image designer, and hera is not one"
        );
        assert_eq!(
            refusal(require_fitting_roles("pygmalion", true, &["worker"])).1,
            "pygmalion is an image agent, which can only be an image designer"
        );
    }

    #[test]
    fn a_tester_is_a_role_a_member_holds_and_a_pool_a_task_is_for_next_to_the_reviewer() {
        for roles in [&MEMBER_ROLES[..], &POOLS[..]] {
            let at = |word: &str| roles.iter().position(|role| *role == word).unwrap();
            assert_eq!(at("tester"), at("reviewer") + 1, "{roles:?}");
            assert_eq!(roles.last(), Some(&"designer"), "{roles:?}");
        }
        assert_eq!(require_roles(&["tester"], String::new).unwrap(), ["tester"]);
        // Any agent may test but an image agent, which is nothing but a designer.
        assert!(require_fitting_roles("hera", false, &["tester"]).is_ok());
        assert_eq!(
            refusal(require_fitting_roles("pygmalion", true, &["tester"])).1,
            "pygmalion is an image agent, which can only be an image designer"
        );
    }

    #[test]
    fn titles_a_task_by_its_first_line_that_says_something() {
        assert_eq!(title_of("\n  \n  Fix the parser  \nmore"), "Fix the parser");
        let long = format!("{}tail", "x".repeat(120));
        assert_eq!(title_of(&long), format!("{}…", "x".repeat(119)));
        assert_eq!(utf16_len(&title_of(&long)), 120);
    }

    #[test]
    fn cuts_text_saying_how_long_it_was() {
        assert_eq!(cut("short", 10), "short");
        assert_eq!(cut("abcdef", 3), "abc\n… (6 characters; cut here)");
        assert_eq!(cut("abcdef", 0), "… (6 characters; cut here)");
    }
}
