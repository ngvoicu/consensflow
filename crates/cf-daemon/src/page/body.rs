//! What an operation reads of the body the page sent, and how it says no
//! (`async ({ project, task }) => …` in `src/core/page.js`).
//!
//! The page sends numbers for ids and text for names, and the ledger turned
//! anything else away in the words of SQLite's bindings, which nobody reads: a
//! field that is no whole number is refused here by name. Where JavaScript
//! wrote a value into a sentence (`no agent named ${agent} in your agents`) it
//! is written as JavaScript wrote it ([`js::text`]): `99` is `99`, a field
//! that was not sent is `undefined`.

use std::borrow::Cow;

use cf_base::js;
use cf_base::refusal::Refusal;
use cf_engine::seams::EngineError;
use cf_ledger::LedgerError;
use serde::Serialize;
use serde_json::{Map, Value};

/// What an operation answers: the fields that follow `ok: true`.
pub(super) type Fields = Map<String, Value>;

/// Why an operation is refused: the words the page shows.
#[derive(Debug)]
pub(super) struct Said(pub(super) String);

impl From<String> for Said {
    fn from(words: String) -> Self {
        Self(words)
    }
}

impl From<&str> for Said {
    fn from(words: &str) -> Self {
        Self(words.to_owned())
    }
}

impl From<LedgerError> for Said {
    fn from(refused: LedgerError) -> Self {
        Self(refused.to_string())
    }
}

impl From<EngineError> for Said {
    fn from(refused: EngineError) -> Self {
        Self(refused.to_string())
    }
}

impl From<Refusal> for Said {
    fn from(refused: Refusal) -> Self {
        Self(refused.message)
    }
}

impl From<serde_json::Error> for Said {
    fn from(refused: serde_json::Error) -> Self {
        Self(refused.to_string())
    }
}

/// The body of a request, an object the page sent: each field is what
/// destructuring it gives, so one the page left out is none, and one it sent
/// as `null` is `null`.
#[derive(Clone, Copy)]
pub(super) struct Body<'a>(&'a Value);

impl<'a> Body<'a> {
    pub(super) fn new(value: &'a Value) -> Self {
        Self(value)
    }

    /// The field as it was sent; none when it was not sent.
    pub(super) fn get(self, name: &str) -> Option<&'a Value> {
        self.0.get(name)
    }

    /// The field as `${value}` writes it into a sentence.
    pub(super) fn text(self, name: &str) -> Cow<'a, str> {
        js::text(self.get(name))
    }

    /// The field, which names a row of the ledger: a whole number.
    pub(super) fn whole(self, name: &str) -> Result<i64, Said> {
        let value = self.get(name);
        value
            .and_then(Value::as_f64)
            .filter(|number| number.fract() == 0.0 && number.abs() <= 9_007_199_254_740_991.0)
            // The bound is the largest whole number a double holds exactly.
            .map(|number| number as i64)
            .ok_or_else(|| Said(format!("{name} is a whole number, not {}", js::text(value))))
    }
}

/// `{name: value}`: the one field an operation answers.
pub(super) fn one(name: &str, value: impl Serialize) -> Result<Fields, Said> {
    let mut fields = Fields::new();
    fields.insert(name.to_owned(), serde_json::to_value(value)?);
    Ok(fields)
}

/// An answer whose fields are those of `value`, which serializes as an object:
/// what the ledger or the engine answered, as it answered it.
pub(super) fn merged(value: impl Serialize) -> Result<Fields, Said> {
    match serde_json::to_value(value)? {
        Value::Object(fields) => Ok(fields),
        other => Err(Said(format!("an answer of fields, not {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_field_the_page_left_out_is_none_and_one_it_sent_null_is_null() {
        let sent = json!({ "project": 3, "agent": null });
        let body = Body::new(&sent);
        assert_eq!(body.get("project"), Some(&json!(3)));
        assert_eq!(body.get("agent"), Some(&Value::Null));
        assert_eq!(body.get("task"), None);
        assert_eq!(body.text("project"), "3");
        assert_eq!(body.text("agent"), "null");
        assert_eq!(body.text("task"), "undefined");
    }

    #[test]
    fn an_id_is_a_whole_number_and_says_what_it_was_when_it_is_not() {
        let sent = json!({ "a": 7, "b": 7.0, "c": 7.5, "d": "7", "e": null, "f": true });
        let body = Body::new(&sent);
        assert_eq!(body.whole("a").unwrap(), 7);
        assert_eq!(body.whole("b").unwrap(), 7, "JavaScript's 7.0 is 7");
        for (field, words) in [
            ("c", "c is a whole number, not 7.5"),
            ("d", "d is a whole number, not 7"),
            ("e", "e is a whole number, not null"),
            ("f", "f is a whole number, not true"),
            ("g", "g is a whole number, not undefined"),
        ] {
            assert_eq!(body.whole(field).unwrap_err().0, words);
        }
    }

    #[test]
    fn a_body_that_is_no_object_has_no_fields() {
        for sent in [json!(5), json!("project"), json!([1]), Value::Null] {
            assert_eq!(Body::new(&sent).get("project"), None);
        }
    }

    #[test]
    fn what_is_answered_is_an_object_of_its_fields_or_the_one_field_named() {
        assert_eq!(
            one("project", json!({ "id": 1 })).unwrap(),
            json!({ "project": { "id": 1 } })
                .as_object()
                .unwrap()
                .clone()
        );
        assert_eq!(
            merged(json!({ "total": 2, "shown": 1 })).unwrap(),
            json!({ "total": 2, "shown": 1 })
                .as_object()
                .unwrap()
                .clone()
        );
        assert!(merged(json!([1])).is_err());
    }
}
