//! A row of the roster in the shape `agents.json` keeps it (`kind`,
//! `thinking`, everything it carries): what the launcher runs and what the
//! views are made from.

use cf_base::js;
use serde::{Serialize, Serializer};
use serde_json::{Map, Value};

/// A row of the roster: an ordered JSON object, never a struct, because the
/// rows and fields this build does not understand are not its own to drop.
/// The typed readings below say what a field holds when it holds what the
/// build reads there; anything else they take for absent.
///
/// A row read from a file has the shape the roster checks as it loads the
/// file: JavaScript took a row of another shape for what it was and failed,
/// or passed it through, in whichever function met it first.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentRow {
    fields: Map<String, Value>,
}

/// The fields that name the agent, what it runs on and what it runs: text
/// when they are there, `null` included not.
const TEXT_FIELDS: [&str; 3] = ["id", "kind", "model"];
/// The fields the profile reads as text when they are set: `null` is as good
/// as absent.
const OPTIONAL_TEXT_FIELDS: [&str; 4] = ["preset", "harness", "effort", "thinking"];

impl AgentRow {
    /// A row the build itself makes, whose shape is its own.
    pub(crate) fn new(fields: Map<String, Value>) -> Self {
        Self { fields }
    }

    /// A row as a file holds it; none when its shape is wrong. A row is an
    /// object; its `id`, `kind` and `model` are text when present; its
    /// `preset`, `harness`, `effort` and `thinking` are text or `null` when
    /// present. Every other field is kept, whatever it holds.
    pub(crate) fn from_value(value: Value) -> Option<Self> {
        let Value::Object(fields) = value else {
            return None;
        };
        let texts = TEXT_FIELDS
            .into_iter()
            .all(|key| matches!(fields.get(key), None | Some(Value::String(_))));
        let optional = OPTIONAL_TEXT_FIELDS
            .into_iter()
            .all(|key| matches!(fields.get(key), None | Some(Value::Null | Value::String(_))));
        (texts && optional).then_some(Self { fields })
    }

    /// Every field the row carries, in the order JavaScript enumerates them.
    pub fn fields(&self) -> &Map<String, Value> {
        &self.fields
    }

    /// A field as the row holds it; none when it is not there.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.fields.get(key)
    }

    /// The agent's handle.
    pub fn id(&self) -> Option<&str> {
        self.text("id")
    }

    /// What the agent runs on, in the payload's word (`claude-code`).
    pub fn kind(&self) -> Option<&str> {
        self.text("kind")
    }

    /// The model the agent runs, by the id its harness accepts.
    pub fn model(&self) -> Option<&str> {
        self.text("model")
    }

    /// The CLI by name (`claude`), when the row says it in the app's own
    /// vocabulary: the profile reads it before the kind.
    pub fn harness(&self) -> Option<&str> {
        self.text("harness")
    }

    /// Provenance, as a row an older build saved names its entry.
    pub fn preset(&self) -> Option<&str> {
        self.text("preset")
    }

    /// The effort level, in the word every runner but Pi's reads.
    pub fn effort(&self) -> Option<&str> {
        self.text("effort")
    }

    /// Pi's word for the effort.
    pub fn thinking(&self) -> Option<&str> {
        self.text("thinking")
    }

    /// An image agent as the profile and the launcher read the row: any
    /// truthy `designer`, `"no"` and `[]` included. The view says it for
    /// `true` alone.
    pub fn is_designer(&self) -> bool {
        js::truthy(self.get("designer"))
    }

    /// `field` set to `value`: a field the row has keeps its place, a new
    /// one comes last.
    pub(crate) fn set(&mut self, field: &str, value: Value) {
        self.fields.insert(field.to_owned(), value);
    }

    /// The row as the JSON object it is.
    pub(crate) fn into_value(self) -> Value {
        Value::Object(self.fields)
    }

    fn text(&self, key: &str) -> Option<&str> {
        match self.fields.get(key) {
            Some(Value::String(text)) => Some(text),
            _ => None,
        }
    }
}

impl Serialize for AgentRow {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.fields.serialize(serializer)
    }
}

/// The key a kind's effort is kept under (`effortKey`): the Pi runner reads
/// `thinking`, every other runner reads `effort`.
pub(crate) fn effort_key(kind: Option<&str>) -> &'static str {
    if kind == Some("pi") {
        "thinking"
    } else {
        "effort"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn read(value: Value) -> Option<AgentRow> {
        AgentRow::from_value(value)
    }

    #[test]
    fn a_row_that_is_no_object_is_of_the_wrong_shape() {
        for value in [
            json!(null),
            json!(5),
            json!(true),
            json!("nova"),
            json!([]),
            json!([{ "id": "nova" }]),
        ] {
            assert_eq!(read(value.clone()), None, "{value}");
        }
    }

    #[test]
    fn an_id_a_kind_or_a_model_that_is_present_and_no_text_is_of_the_wrong_shape() {
        for field in TEXT_FIELDS {
            for value in [json!(null), json!(5), json!(false), json!([]), json!({})] {
                assert_eq!(
                    read(json!({ field: value.clone() })),
                    None,
                    "{field}: {value}"
                );
            }
            assert!(read(json!({ field: "text" })).is_some(), "{field}");
            assert!(read(json!({ field: "" })).is_some(), "{field} empty");
        }
    }

    #[test]
    fn a_preset_a_harness_an_effort_or_a_thinking_that_is_set_and_no_text_is_of_the_wrong_shape() {
        for field in OPTIONAL_TEXT_FIELDS {
            for value in [json!(3), json!(true), json!([]), json!({})] {
                assert_eq!(
                    read(json!({ field: value.clone() })),
                    None,
                    "{field}: {value}"
                );
            }
            assert!(read(json!({ field: "text" })).is_some(), "{field}");
            assert!(read(json!({ field: null })).is_some(), "{field} null");
        }
    }

    #[test]
    fn every_other_field_is_kept_whatever_it_holds() {
        let row = read(json!({
            "id": "nova",
            "designer": "no",
            "workTier": "huge",
            "description": { "any": [1, null] },
            "name": 7,
            "custom": [],
            "createdAt": null,
            "colour": "green",
        }))
        .unwrap();
        assert_eq!(row.fields().len(), 8);
        assert_eq!(row.get("workTier"), Some(&json!("huge")));
        assert_eq!(row.get("name"), Some(&json!(7)));
        assert_eq!(row.get("colour"), Some(&json!("green")));
        assert_eq!(row.get("createdAt"), Some(&Value::Null));
        assert_eq!(row.get("no-such-field"), None);
    }

    #[test]
    fn the_typed_readings_take_text_and_call_null_and_absent_none() {
        let row = read(json!({
            "id": "nova",
            "kind": "pi",
            "model": "m",
            "harness": "codex",
            "preset": null,
            "effort": "",
            "thinking": "low",
        }))
        .unwrap();
        assert_eq!(row.id(), Some("nova"));
        assert_eq!(row.kind(), Some("pi"));
        assert_eq!(row.model(), Some("m"));
        assert_eq!(row.harness(), Some("codex"));
        assert_eq!(row.preset(), None, "null");
        assert_eq!(row.effort(), Some(""), "empty text is text");
        assert_eq!(row.thinking(), Some("low"));
        let bare = read(json!({})).unwrap();
        assert_eq!(
            (bare.id(), bare.kind(), bare.model(), bare.harness()),
            (None, None, None, None)
        );
    }

    #[test]
    fn a_row_is_a_designer_when_its_flag_is_truthy_the_way_javascript_reads_it() {
        for truthy in [
            json!(true),
            json!("no"),
            json!("0"),
            json!([]),
            json!({}),
            json!(1),
        ] {
            let row = read(json!({ "designer": truthy.clone() })).unwrap();
            assert!(row.is_designer(), "{truthy}");
        }
        for falsy in [json!(false), json!(0), json!(""), json!(null)] {
            let row = read(json!({ "designer": falsy.clone() })).unwrap();
            assert!(!row.is_designer(), "{falsy}");
        }
        assert!(!read(json!({})).unwrap().is_designer(), "absent");
    }

    #[test]
    fn setting_a_field_keeps_its_place_and_a_new_one_comes_last() {
        let mut row = read(json!({ "id": "a", "kind": "image", "model": "m" })).unwrap();
        row.set("kind", json!("codex"));
        row.set("designer", json!(true));
        assert_eq!(
            serde_json::to_string(&row).unwrap(),
            r#"{"id":"a","kind":"codex","model":"m","designer":true}"#
        );
    }

    #[test]
    fn a_row_writes_its_fields_in_the_order_it_holds_them() {
        let row = read(json!({ "model": "m", "id": "a", "z": 1, "a": 2 })).unwrap();
        assert_eq!(
            serde_json::to_string(&row).unwrap(),
            r#"{"model":"m","id":"a","z":1,"a":2}"#
        );
    }

    #[test]
    fn the_effort_is_kept_under_thinking_for_pi_and_effort_for_every_other_kind() {
        assert_eq!(effort_key(Some("pi")), "thinking");
        for kind in [
            Some("claude-code"),
            Some("codex"),
            Some("opencode"),
            Some("devin"),
            Some("kimi"),
            Some("Pi"),
            Some(""),
            None,
        ] {
            assert_eq!(effort_key(kind), "effort", "{kind:?}");
        }
    }
}
