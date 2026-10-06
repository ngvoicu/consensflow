//! What a screen reads of its request's body (`agents-server.js:98`): the one
//! JSON value it holds, as JavaScript's `JSON.parse` hands it to the route, and
//! what each route reads of that value as JavaScript reads it.
//!
//! A screen's body is any JSON value, not an object: `null` is a body, and so
//! is a number. A route reads the properties it needs of it, and JavaScript
//! gave `undefined` for those of everything but `null`, which it threw for. The
//! words of that throw are V8's, and the same here ([`property`]).
//!
//! **The daemon's own words.** Node answered a body that is no JSON in V8's
//! (`Expected property name or '}' in JSON at position 1 (line 1 column 2)`),
//! which no Rust parser says. The daemon says that the body is no JSON and
//! where, in `serde_json`'s words.

use cf_base::json::from_slice_lossy;
use serde_json::{Map, Value};

use crate::api::request::Request;

/// The body as `JSON.parse((await readBody(request)) || '{}')` reads it: `{}`
/// for no body at all (an empty text is none), else what the text is as JSON.
/// Its error is the words the answer says: `body too large` (over 64 K UTF-16
/// units), what the connection said when it broke, or that the text is no JSON.
pub(super) async fn read(request: &mut Request) -> Result<Value, String> {
    let text = request
        .text()
        .await
        .map_err(|unread| unread.message().to_owned())?;
    if text.is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    from_slice_lossy(text.as_bytes())
        .map_err(|failed| format!("the request body is not valid JSON: {failed}"))
}

/// `body[name]`: the property of an object, none for a property it has not, and
/// for any other value but `null` too (a text, a number, a flag and a list have
/// none of the ones a route reads). `null` has no properties to read: V8 threw
/// `Cannot read properties of null (reading 'id')`, and that is the error.
pub(super) fn property<'a>(body: &'a Value, name: &str) -> Result<Option<&'a Value>, String> {
    match body {
        Value::Object(fields) => Ok(fields.get(name)),
        Value::Null => Err(format!("Cannot read properties of null (reading '{name}')")),
        _ => Ok(None),
    }
}

/// The fields of `body` as a roster operation, which reads its input's
/// properties one by one, reads them: those of an object, none for any other
/// value, and for `null` the error of the first property it reads, `first`.
pub(super) fn fields(body: &Value, first: &str) -> Result<Map<String, Value>, String> {
    property(body, first)?;
    Ok(match body {
        Value::Object(fields) => fields.clone(),
        _ => Map::new(),
    })
}

#[cfg(test)]
mod tests;
