//! A value read from a record, as a JavaScript `Map` or `Set` takes it for a
//! key (SameValueZero): `undefined`, `null`, a flag, a number and a text by
//! what they are, an object or a list by which one it is. A reader that keys
//! its maps by what a record says (a turn's id, a call's) keys them so, and
//! a record that says something odd there is read as Node read it.

use std::sync::Arc;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    Undefined,
    Null,
    Flag(bool),
    /// The double's bits, `-0` as `0`: JSON holds no `NaN`.
    Number(u64),
    Text(Arc<str>),
    /// An object or a list: the how-manyth one the reader keyed by. Each one
    /// a record holds is another, as each object `JSON.parse` builds is.
    Object(u64),
}

impl Key {
    /// Whether JavaScript takes the key for true (`if (turnId)`).
    pub(crate) fn truthy(&self) -> bool {
        match self {
            Key::Undefined | Key::Null => false,
            Key::Flag(flag) => *flag,
            Key::Number(bits) => f64::from_bits(*bits) != 0.0,
            Key::Text(text) => !text.is_empty(),
            Key::Object(_) => true,
        }
    }

    /// Whether `??` passes it by: `undefined` or `null`.
    pub(crate) fn is_nullish(&self) -> bool {
        matches!(self, Key::Undefined | Key::Null)
    }

    /// A number as a key: by its value, `-0` as `0`.
    pub(crate) fn number(number: f64) -> Self {
        Key::Number(if number == 0.0 { 0.0_f64 } else { number }.to_bits())
    }
}

/// What tells one object a reader keys by from another.
#[derive(Debug, Default)]
pub(crate) struct Keys {
    objects: u64,
}

impl Keys {
    /// `value` as a key, none being `undefined`. An object or a list is a
    /// key no other is: read it once for every use of the one value.
    pub(crate) fn of(&mut self, value: Option<&Value>) -> Key {
        match value {
            None => Key::Undefined,
            Some(Value::Null) => Key::Null,
            Some(Value::Bool(flag)) => Key::Flag(*flag),
            Some(Value::Number(number)) => Key::number(number.as_f64().unwrap_or(f64::NAN)),
            Some(Value::String(text)) => Key::Text(Arc::from(text.as_str())),
            Some(Value::Array(_) | Value::Object(_)) => self.object(),
        }
    }

    /// The key of an object no key was made of before.
    pub(crate) fn object(&mut self) -> Key {
        self.objects += 1;
        Key::Object(self.objects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_compare_as_same_value_zero() {
        let mut keys = Keys::default();
        assert_eq!(keys.of(Some(&json!("a"))), keys.of(Some(&json!("a"))));
        assert_ne!(keys.of(Some(&json!("1"))), keys.of(Some(&json!(1))));
        assert_eq!(keys.of(Some(&json!(1))), keys.of(Some(&json!(1.0))));
        let zero: Value = serde_json::from_str("-0.0").unwrap();
        assert_eq!(keys.of(Some(&zero)), keys.of(Some(&json!(0))));
        assert_ne!(keys.of(None), keys.of(Some(&Value::Null)));
        // Two objects are two keys, however alike.
        assert_ne!(keys.of(Some(&json!({}))), keys.of(Some(&json!({}))));
    }

    #[test]
    fn truth_is_javascripts() {
        let mut keys = Keys::default();
        for falsy in [
            None,
            Some(json!(null)),
            Some(json!(false)),
            Some(json!(0)),
            Some(json!("")),
        ] {
            assert!(!keys.of(falsy.as_ref()).truthy(), "{falsy:?}");
        }
        for truthy in [json!(true), json!(-1.5), json!("0"), json!([]), json!({})] {
            assert!(keys.of(Some(&truthy)).truthy(), "{truthy}");
        }
        assert!(keys.of(None).is_nullish() && keys.of(Some(&json!(null))).is_nullish());
        assert!(!keys.of(Some(&json!(0))).is_nullish());
    }
}
