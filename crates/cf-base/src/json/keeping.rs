//! A line of JSON read for some of what it holds: the members and items a
//! reader asks for are built into a value, and everything else is read and
//! dropped without ever becoming one ([`Keep`]).
//!
//! What is kept is what [`from_slice_lossy`](super::from_slice_lossy) reads of
//! it: the last of a key's duplicates in the place of the first, keys in the
//! order JavaScript enumerates them, numbers as the doubles they read as,
//! invalid UTF-8 and a lone surrogate's escape as U+FFFD. What is dropped is
//! read all the same, so a line fails where `from_slice_lossy` fails, and
//! nowhere else: malformed, nested past [`DEEPEST`](super::DEEPEST) levels,
//! or with a number past a double's range. Skipping it as serde's
//! `IgnoredAny` does would not: `IgnoredAny` has no depth limit and reads any
//! number. This reads through the same `deserialize_any` that builds a
//! value, which has both.

use std::fmt;

use serde::de::{DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::{Map, Number, Value};

use super::{as_parsed, without_lone_surrogates};

/// What a reading keeps of a value.
#[derive(Debug)]
pub enum Keep {
    /// The value whole, as `from_slice_lossy` reads it.
    All,
    /// A text, a number, a flag or null as it is read; a list or an object
    /// as an empty one of its own kind.
    Scalar,
    /// The members of an object, each kept as its entry says. A list is kept
    /// as an empty list; any other value as it is.
    Members(&'static [(&'static str, &'static Keep)]),
    /// Each item of a list, kept as said. An object is kept as an empty
    /// object; any other value as it is.
    Items(&'static Keep),
}

/// The JSON in `bytes` as [`from_slice_lossy`](super::from_slice_lossy) holds
/// it, kept as `keep` says and no more.
pub fn from_slice_lossy_keeping(bytes: &[u8], keep: &Keep) -> serde_json::Result<Value> {
    let text = String::from_utf8_lossy(bytes);
    let fixed = without_lone_surrogates(&text);
    let mut reading = serde_json::Deserializer::from_str(&fixed);
    let mut value = Kept(keep).deserialize(&mut reading)?;
    reading.end()?;
    as_parsed(&mut value);
    Ok(value)
}

/// A value read as `keep` says: a seed to read one with, and the visitor
/// that builds it.
#[derive(Clone, Copy)]
struct Kept<'k>(&'k Keep);

impl<'de> DeserializeSeed<'de> for Kept<'_> {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        match self.0 {
            Keep::All => Value::deserialize(deserializer),
            _ => deserializer.deserialize_any(self),
        }
    }
}

impl<'de> Visitor<'de> for Kept<'_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any valid JSON value")
    }

    fn visit_bool<E>(self, flag: bool) -> Result<Value, E> {
        Ok(Value::Bool(flag))
    }

    fn visit_i64<E>(self, number: i64) -> Result<Value, E> {
        Ok(Value::Number(number.into()))
    }

    fn visit_u64<E>(self, number: u64) -> Result<Value, E> {
        Ok(Value::Number(number.into()))
    }

    fn visit_f64<E>(self, number: f64) -> Result<Value, E> {
        Ok(Number::from_f64(number).map_or(Value::Null, Value::Number))
    }

    fn visit_str<E>(self, text: &str) -> Result<Value, E> {
        Ok(Value::String(text.to_owned()))
    }

    fn visit_string<E>(self, text: String) -> Result<Value, E> {
        Ok(Value::String(text))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut list: A) -> Result<Value, A::Error> {
        let Keep::Items(keep) = self.0 else {
            while list.next_element_seed(Discard)?.is_some() {}
            return Ok(Value::Array(Vec::new()));
        };
        let mut items = Vec::new();
        while let Some(item) = list.next_element_seed(Kept(keep))? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<Value, A::Error> {
        let Keep::Members(members) = self.0 else {
            while object.next_key_seed(Discard)?.is_some() {
                object.next_value_seed(Discard)?;
            }
            return Ok(Value::Object(Map::new()));
        };
        let mut kept = Map::new();
        while let Some(member) = object.next_key_seed(Member(members))? {
            match member {
                // A key said twice is the later value in the earlier place,
                // as `JSON.parse` has it.
                Some((name, keep)) => {
                    let value = object.next_value_seed(Kept(keep))?;
                    kept.insert((*name).to_owned(), value);
                }
                None => object.next_value_seed(Discard)?,
            }
        }
        Ok(Value::Object(kept))
    }
}

/// A key of an object, read as the entry of the members it names, if any.
struct Member(&'static [(&'static str, &'static Keep)]);

type Entry = &'static (&'static str, &'static Keep);

impl<'de> DeserializeSeed<'de> for Member {
    type Value = Option<Entry>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Member {
    type Value = Option<Entry>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a key")
    }

    fn visit_str<E>(self, key: &str) -> Result<Self::Value, E> {
        Ok(self.0.iter().find(|(name, _)| *name == key))
    }
}

/// A value read to its end and dropped, a key too. Read through
/// `deserialize_any`, as a value built is, so that what cannot be built
/// (nested too deep, a number past a double's range) cannot be skipped either.
struct Discard;

impl<'de> DeserializeSeed<'de> for Discard {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Discard {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any valid JSON value")
    }

    fn visit_bool<E>(self, _: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E>(self, _: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E>(self, _: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E>(self, _: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E>(self, _: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut list: A) -> Result<(), A::Error> {
        while list.next_element_seed(Discard)?.is_some() {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<(), A::Error> {
        while object.next_key_seed(Discard)?.is_some() {
            object.next_value_seed(Discard)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
