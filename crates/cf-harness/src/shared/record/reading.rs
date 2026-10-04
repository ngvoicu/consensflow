//! What a look at a harness's own record of a conversation reads
//! (`resultBase`, `emit`, `unreadable`, `nativeId` and `visibleText`,
//! `hosts/lib/completion/shared.js`): the record's items in its order, and
//! what they say of the turn, written as Node wrote a reading.

use std::sync::Arc;

use cf_base::js;
use serde::ser::{SerializeMap, Serializer};
use serde::Serialize;
use serde_json::Value;

use crate::shared::quota::Quota;

/// A look's reading: what the record says, or that it could not say, with
/// the reason (`{unknown: true, reason}`).
#[derive(Debug, Clone, PartialEq)]
pub enum Reading {
    Unknown(String),
    Known(Record),
}

impl Reading {
    /// `unreadable`: a failure to read, said after `unreadable: `.
    pub(crate) fn unreadable(reason: &str) -> Self {
        Reading::Unknown(format!("unreadable: {reason}"))
    }
}

impl Serialize for Reading {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Reading::Unknown(reason) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("unknown", &true)?;
                map.serialize_entry("reason", reason)?;
                map.end()
            }
            Reading::Known(record) => record.serialize(serializer),
        }
    }
}

/// What a record says (`resultBase`): its items, and what they say of the
/// turn. The quota is shared, as Node shared the object: the scheduler
/// tells an old refusal from a new one by the object it holds.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub items: Vec<Item>,
    pub in_flight: bool,
    /// Its own question dialog is open: the window waits for the human's answer.
    pub asking: bool,
    pub failed: bool,
    pub quota: Option<Arc<Quota>>,
    pub settlement: Settlement,
}

impl Record {
    /// `resultBase()`: no items, nothing in flight, settlement unknown.
    pub(crate) fn new() -> Self {
        Self {
            items: Vec::new(),
            in_flight: false,
            asking: false,
            failed: false,
            quota: None,
            settlement: Settlement::Unknown,
        }
    }
}

/// Whether the turn is over: `{state: 'unknown' | 'in-flight' | 'settled'}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settlement {
    Unknown,
    InFlight,
    Settled,
}

impl Serialize for Settlement {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let state = match self {
            Settlement::Unknown => "unknown",
            Settlement::InFlight => "in-flight",
            Settlement::Settled => "settled",
        };
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("state", state)?;
        map.end()
    }
}

/// Who an item is from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    Tool,
    /// What the harness put in the conversation itself: an extension's
    /// message, a hook's context.
    Custom,
}

/// An item as a reading returns it (`emit`): its native id, who it is from,
/// its text, whether it is complete, and when, as the record had it (a
/// timestamp's text, or a number). Its id and text are shared, so a later
/// reading that keeps the item copies neither.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: Arc<str>,
    pub role: Role,
    pub text: Arc<str>,
    pub complete: bool,
    pub at: Value,
    /// A progress note, as Codex marks one: said only when true.
    pub commentary: bool,
}

impl Serialize for Item {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("id", &*self.id)?;
        map.serialize_entry("role", &self.role)?;
        map.serialize_entry("text", &*self.text)?;
        map.serialize_entry("complete", &self.complete)?;
        map.serialize_entry("at", &self.at)?;
        if self.commentary {
            map.serialize_entry("commentary", &true)?;
        }
        map.end()
    }
}

/// `nativeId`: an item's id as the record gave it, or the failure of a record
/// that gave none: `missing native <kind> id at record <seq>`.
pub(crate) fn native_id(
    value: Option<&Value>,
    kind: &str,
    seq: &Value,
) -> Result<Arc<str>, String> {
    match value {
        Some(Value::String(id)) if !id.is_empty() => Ok(Arc::from(id.as_str())),
        _ => Err(format!(
            "missing native {kind} id at record {}",
            js::text(Some(seq))
        )),
    }
}

/// Why a record that is JSON's `null` fails its look: Node read a field of
/// it, and V8 threw.
pub(crate) fn null_record(index: usize) -> String {
    format!("record {index} is null, where an object was read")
}

/// `visibleText`: what a value shows as text. Text is itself, nothing is
/// nothing, a list is its parts on lines of their own (a part's own `text`
/// when it has one), and anything else is its JSON.
pub(crate) fn visible_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        None | Some(Value::Null) => String::new(),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|part| match part {
                Value::String(text) => text.clone(),
                Value::Object(fields) => match fields.get("text") {
                    Some(Value::String(text)) => text.clone(),
                    _ => js::stringify(part),
                },
                other => js::stringify(other),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some(other) => js::stringify(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_reading_is_written_as_node_wrote_one() {
        let mut record = Record::new();
        record.items.push(Item {
            id: Arc::from("msg_1"),
            role: Role::Assistant,
            text: Arc::from("Done."),
            complete: true,
            at: json!("2026-09-06T17:27:06.000Z"),
            commentary: false,
        });
        record.items.push(Item {
            id: Arc::from("msg_2"),
            role: Role::Assistant,
            text: Arc::from("Reading the diff"),
            complete: false,
            at: json!(7),
            commentary: true,
        });
        record.settlement = Settlement::InFlight;
        assert_eq!(
            serde_json::to_string(&Reading::Known(record)).unwrap(),
            r#"{"items":[{"id":"msg_1","role":"assistant","text":"Done.","complete":true,"at":"2026-09-06T17:27:06.000Z"},{"id":"msg_2","role":"assistant","text":"Reading the diff","complete":false,"at":7,"commentary":true}],"inFlight":false,"asking":false,"failed":false,"quota":null,"settlement":{"state":"in-flight"}}"#
        );
        assert_eq!(
            serde_json::to_string(&Reading::unreadable("no codex rollout for s")).unwrap(),
            r#"{"unknown":true,"reason":"unreadable: no codex rollout for s"}"#
        );
    }

    #[test]
    fn an_item_with_no_native_id_fails_its_record() {
        assert_eq!(
            native_id(Some(&json!("a")), "codex item", &json!(3))
                .unwrap()
                .as_ref(),
            "a"
        );
        for missing in [None, Some(json!("")), Some(json!(5)), Some(json!(null))] {
            assert_eq!(
                native_id(missing.as_ref(), "codex item", &json!(3)).unwrap_err(),
                "missing native codex item id at record 3"
            );
        }
    }

    #[test]
    fn visible_text_is_what_node_showed() {
        assert_eq!(visible_text(Some(&json!("x"))), "x");
        assert_eq!(visible_text(None), "");
        assert_eq!(visible_text(Some(&json!(null))), "");
        assert_eq!(
            visible_text(Some(
                &json!(["a", { "text": "b" }, { "type": "image" }, 2.0, null, ["c"]])
            )),
            "a\nb\n{\"type\":\"image\"}\n2\nnull\n[\"c\"]"
        );
        assert_eq!(visible_text(Some(&json!({ "a": 1.5 }))), r#"{"a":1.5}"#);
        assert_eq!(visible_text(Some(&json!(true))), "true");
    }
}
