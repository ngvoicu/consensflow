//! `Number(value)` of a JSON value: what a record's field says when the code
//! that read it asked for a number.

use serde_json::Value;

use super::{number, string};

/// `value` as `Number(value)` reads it, none being `undefined`, or why V8
/// threw. `undefined` is NaN, `null` is 0, a flag is 0 or 1, a number is
/// itself and a text is read as [`number`] reads it. A list or an object is
/// read as the text `String(value)` writes for it: `[]` is 0, `[5]` is 5,
/// `[1,2]` and `{}` are NaN; and one that holds an object with a `toString`
/// of its own has no number, as it has no text ([`string`]).
pub fn to_number(value: Option<&Value>) -> Result<f64, String> {
    Ok(match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(flag)) => f64::from(u8::from(*flag)),
        Some(Value::Number(value)) => value.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(written)) => number(written),
        Some(listed) => number(&string(Some(listed))?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Whether two numbers are the same one, NaN included.
    fn same(left: f64, right: f64) -> bool {
        left == right || (left.is_nan() && right.is_nan())
    }

    #[test]
    fn reads_a_value_as_number_did() {
        let nan = f64::NAN;
        // Node 26's Number(value) for each.
        let cases = [
            (json!(null), 0.0),
            (json!(true), 1.0),
            (json!(false), 0.0),
            (json!(429), 429.0),
            (json!(-2.5), -2.5),
            (json!(1e21), 1e21),
            (json!("429"), 429.0),
            (json!(" 429 "), 429.0),
            (json!(""), 0.0),
            (json!("  "), 0.0),
            (json!("abc"), nan),
            (json!("0x1AD"), 429.0),
            (json!("1e3"), 1000.0),
            (json!("Infinity"), f64::INFINITY),
            (json!("-Infinity"), f64::NEG_INFINITY),
            (json!("4.29e2"), 429.0),
            (json!("402.0"), 402.0),
            (json!("1_000"), nan),
            (json!([]), 0.0),
            (json!([5]), 5.0),
            (json!(["7"]), 7.0),
            (json!([null]), 0.0),
            (json!([[3]]), 3.0),
            (json!([1, 2]), nan),
            (json!([" 9 "]), 9.0),
            (json!([true]), nan),
            (json!([[]]), 0.0),
            (json!([null, null]), nan),
            (json!([429]), 429.0),
            (json!({}), nan),
            (json!({ "a": 1 }), nan),
        ];
        for (value, number) in cases {
            assert!(same(to_number(Some(&value)).unwrap(), number), "{value}");
        }
    }

    #[test]
    fn a_field_that_is_not_there_is_nan() {
        assert!(to_number(None).unwrap().is_nan());
    }

    #[test]
    fn a_value_with_a_to_string_of_its_own_has_no_number_where_v8_threw() {
        // Node: Number(JSON.parse(text)) throws for each.
        for thrown in [json!({ "toString": null }), json!([{ "toString": 0 }])] {
            assert!(to_number(Some(&thrown)).is_err(), "{thrown}");
        }
        assert!(to_number(Some(&json!({ "valueOf": 1 }))).unwrap().is_nan());
    }
}
