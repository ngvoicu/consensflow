//! What a handler answers with, and how a failure is said. **Frozen**: the
//! routes of the three landings that follow answer with these and nothing of
//! their own.
//!
//! An answer is a status and what goes with it: a JSON body, a page of HTML,
//! or nothing. A failure is a [`Refusal`] (or the ledger's), said with its
//! status as `{error: <code>, message}`, or anything else, said as 500
//! `{error: "internal", message}`.

use cf_base::refusal::Refusal;
use cf_engine::seams::EngineError;
use cf_ledger::LedgerError;
use serde_json::{json, Value};

/// A handler's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub status: u16,
    pub content: Content,
}

/// What an answer carries.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// A JSON body, sent as `application/json`, its keys in the order they
    /// were put in.
    Json(Value),
    /// A page, sent as `text/html; charset=utf-8`.
    Html(String),
    /// Nothing, and no content type.
    Nothing,
}

impl Answer {
    /// `body` as JSON, with `status`.
    pub fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            content: Content::Json(body),
        }
    }

    /// 200 and `body` (`ok`).
    pub fn ok(body: Value) -> Self {
        Self::json(200, body)
    }

    /// 201 and `body`: a row was made.
    pub fn created(body: Value) -> Self {
        Self::json(201, body)
    }

    /// 200 and a page of HTML.
    pub fn html(page: impl Into<String>) -> Self {
        Self {
            status: 200,
            content: Content::Html(page.into()),
        }
    }

    /// `status` and nothing else (204 for a row deleted).
    pub fn nothing(status: u16) -> Self {
        Self {
            status,
            content: Content::Nothing,
        }
    }
}

/// Why a handler did not answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    /// A refusal, the engine's or the ledger's: its code, its words, its
    /// status.
    Refused(Refusal),
    /// Anything else (a database that failed, a body that broke off): 500,
    /// with what it said.
    Internal(String),
}

impl Failure {
    /// A refusal of `code`, said as `message`, with `status`.
    pub fn refuse(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self::Refused(Refusal::with_status(code, message, status))
    }

    /// The failure as the API answers it: `{error, message}`, with the
    /// refusal's status, or 500 and `internal`.
    pub fn answer(&self) -> Answer {
        match self {
            Self::Refused(refusal) => Answer::json(
                refusal.status,
                json!({ "error": refusal.code, "message": refusal.message }),
            ),
            Self::Internal(message) => {
                Answer::json(500, json!({ "error": "internal", "message": message }))
            }
        }
    }
}

impl From<Refusal> for Failure {
    fn from(refusal: Refusal) -> Self {
        Self::Refused(refusal)
    }
}

/// A ledger's refusal is a refusal; what SQLite or a stored text said is not.
impl From<LedgerError> for Failure {
    fn from(error: LedgerError) -> Self {
        match error {
            LedgerError::Refused(refusal) => Self::Refused(refusal),
            other => Self::Internal(other.to_string()),
        }
    }
}

impl From<EngineError> for Failure {
    fn from(error: EngineError) -> Self {
        match error {
            EngineError::Refused(refusal) | EngineError::Ledger(LedgerError::Refused(refusal)) => {
                Self::Refused(refusal)
            }
            EngineError::Ledger(other) => Self::Internal(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_is_said_as_its_code_and_words_with_its_own_status() {
        let failure = Failure::refuse(
            403,
            "not-the-chief",
            "the chief's history is the chief's to read",
        );
        assert_eq!(
            failure.answer(),
            Answer::json(
                403,
                json!({ "error": "not-the-chief", "message": "the chief's history is the chief's to read" })
            )
        );
        let written = match failure.answer().content {
            Content::Json(body) => serde_json::to_string(&body).unwrap(),
            other => panic!("not JSON: {other:?}"),
        };
        assert_eq!(
            written,
            r#"{"error":"not-the-chief","message":"the chief's history is the chief's to read"}"#,
            "the code first, then the words"
        );
    }

    #[test]
    fn anything_else_is_a_500_that_says_internal_and_what_it_said() {
        let failure = Failure::Internal("disk I/O error".to_owned());
        assert_eq!(
            failure.answer(),
            Answer::json(
                500,
                json!({ "error": "internal", "message": "disk I/O error" })
            )
        );
    }

    #[test]
    fn the_ledger_s_refusal_keeps_its_status_and_its_other_failures_are_internal() {
        let refused = LedgerError::refused_with("unknown-task", "no task T-9 in this project", 404);
        assert_eq!(
            Failure::from(refused).answer(),
            Answer::json(
                404,
                json!({ "error": "unknown-task", "message": "no task T-9 in this project" })
            )
        );
        let broke = LedgerError::Json(serde_json::from_str::<Value>("{").unwrap_err());
        let Failure::Internal(message) = Failure::from(broke) else {
            panic!("a stored text that does not read is no refusal");
        };
        assert!(message.contains("EOF while parsing"), "{message}");
    }

    #[test]
    fn the_engine_s_refusals_are_refusals_whichever_way_they_came() {
        let words = Refusal::new("project-closed", "app is closed: resume it first");
        let engine = EngineError::Refused(words.clone());
        let ledger = EngineError::Ledger(LedgerError::Refused(words.clone()));
        assert_eq!(Failure::from(engine), Failure::Refused(words.clone()));
        assert_eq!(Failure::from(ledger), Failure::Refused(words));
    }

    #[test]
    fn the_helpers_name_their_status_and_their_content() {
        assert_eq!(Answer::ok(json!({})).status, 200);
        assert_eq!(Answer::created(json!({})).status, 201);
        assert_eq!(
            Answer::html("<p>hi</p>").content,
            Content::Html("<p>hi</p>".to_owned())
        );
        assert_eq!(
            Answer::nothing(204),
            Answer {
                status: 204,
                content: Content::Nothing
            }
        );
    }
}
