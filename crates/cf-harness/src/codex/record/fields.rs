//! What a rollout's records say, read as Node read them: a field of something
//! that may be none, `??`, `Number.isInteger`, `${value ?? ''}`, and a message's
//! text.

use cf_base::js;
use serde_json::{Number, Value};

use crate::shared::record::jsonl::Stop;

/// A field of a value that may be none (`value?.name`): none of anything
/// that is no object.
pub(super) fn field<'a>(value: Option<&'a Value>, name: &str) -> Option<&'a Value> {
    value.and_then(|value| value.get(name))
}

/// `first ?? second`.
pub(super) fn nullish_or<'a>(
    first: Option<&'a Value>,
    second: Option<&'a Value>,
) -> Option<&'a Value> {
    match first {
        None | Some(Value::Null) => second,
        first => first,
    }
}

/// `${value ?? ''}`, or the failure V8 threw.
pub(super) fn or_empty(value: Option<&Value>) -> Result<String, Stop> {
    match value {
        None | Some(Value::Null) => Ok(String::new()),
        value => Ok(js::string(value).map_err(Stop::Failed)?.into_owned()),
    }
}

/// `Number.isInteger`: a number with no fraction.
pub(super) fn is_whole(number: &Number) -> bool {
    number.is_i64()
        || number.is_u64()
        || number.as_f64().is_some_and(|double| double.fract() == 0.0)
}

/// `contentText`: a message's text, from text, or from a list of parts
/// whose own text (`text`, else `Text`) is joined a line each.
pub(super) fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| nullish_or(part.get("text"), part.get("Text"))?.as_str())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}
