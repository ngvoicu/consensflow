//! A column's value as `node:sqlite` hands it to JavaScript, and what
//! JavaScript makes of one: `String(value)`, `Number(value)`, `===`, `>`, a
//! `Map`'s key, `JSON.parse(value)`, the JSON `JSON.stringify` writes of it,
//! and the parameter `node:sqlite` binds it as. A reader holds each column as
//! it was read and makes of it what its JavaScript made, where it made it.

use std::borrow::Cow;
use std::sync::Arc;

use cf_base::js;
use cf_base::json::{from_slice_lossy, is_json_lossy, DEEPEST};
use rusqlite::types::Value as Bound;
use serde_json::{Number, Value};

use crate::shared::record::key::{Key, Keys};

/// The largest integer a double holds exactly, either way.
const EXACT: f64 = 9_007_199_254_740_992.0;

/// A column's value as JavaScript held it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Cell {
    Null,
    /// An integer a double holds exactly, or a real: an infinity too, never
    /// NaN, which SQLite stores as NULL.
    Number(f64),
    Text(String),
    /// A blob: a `Uint8Array`, an object, so no other value is it.
    Bytes(Arc<[u8]>),
}

impl Cell {
    /// `String(value)`: a blob as its bytes' numbers joined by commas, as an
    /// array's `toString` joins them.
    pub(crate) fn text(&self) -> Cow<'_, str> {
        match self {
            Cell::Null => Cow::Borrowed("null"),
            Cell::Number(number) => Cow::Owned(js::number_text(*number)),
            Cell::Text(text) => Cow::Borrowed(text),
            Cell::Bytes(bytes) => Cow::Owned(
                bytes
                    .iter()
                    .map(u8::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        }
    }

    /// `self === other`: a blob is itself alone.
    pub(crate) fn same(&self, other: &Cell) -> bool {
        match (self, other) {
            (Cell::Null, Cell::Null) => true,
            (Cell::Number(left), Cell::Number(right)) => left == right,
            (Cell::Text(left), Cell::Text(right)) => left == right,
            (Cell::Bytes(left), Cell::Bytes(right)) => Arc::ptr_eq(left, right),
            _ => false,
        }
    }

    /// `self > other`: two texts (a blob's as `String` writes it) by their
    /// UTF-16 code units; anything else as numbers, null as 0, and text that
    /// is no number as NaN, never greater.
    pub(crate) fn greater(&self, other: &Cell) -> bool {
        let textual = |cell: &Cell| matches!(cell, Cell::Text(_) | Cell::Bytes(_));
        if textual(self) && textual(other) {
            return self.text().encode_utf16().gt(other.text().encode_utf16());
        }
        self.number() > other.number()
    }

    /// `Number(value)`.
    pub(crate) fn number(&self) -> f64 {
        match self {
            Cell::Null => 0.0,
            Cell::Number(number) => *number,
            Cell::Text(_) | Cell::Bytes(_) => js::number(&self.text()),
        }
    }

    /// `JSON.parse(value)`: the value made text as `String` makes it, then
    /// read as JSON.
    pub(crate) fn parse(&self) -> Result<Value, Unparsed> {
        let text = self.text();
        from_slice_lossy(text.as_bytes()).map_err(|_| {
            if is_json_lossy(text.as_bytes()) {
                Unparsed::Unheld
            } else {
                Unparsed::NoJson
            }
        })
    }

    /// The key a `Map` or a `Set` takes it by (SameValueZero): a blob a key
    /// no other value is, made once for each one read.
    pub(crate) fn key(&self, keys: &mut Keys) -> Key {
        match self {
            Cell::Null => Key::Null,
            Cell::Number(number) => Key::number(*number),
            Cell::Text(text) => Key::Text(Arc::from(text.as_str())),
            Cell::Bytes(_) => keys.object(),
        }
    }

    /// The value as `JSON.stringify` writes it: an infinity as null, and a
    /// blob as an object of its bytes by their places (`{"0":1,"1":2}`).
    pub(crate) fn json(&self) -> Value {
        match self {
            Cell::Null => Value::Null,
            Cell::Number(number) => number_json(*number),
            Cell::Text(text) => Value::String(text.clone()),
            Cell::Bytes(bytes) => Value::Object(
                bytes
                    .iter()
                    .enumerate()
                    .map(|(at, byte)| (at.to_string(), Value::from(*byte)))
                    .collect(),
            ),
        }
    }

    /// The parameter `node:sqlite` binds the value as: a number as a
    /// double, an integer too, and a blob as a blob.
    pub(crate) fn bound(&self) -> Bound {
        match self {
            Cell::Null => Bound::Null,
            Cell::Number(number) => Bound::Real(*number),
            Cell::Text(text) => Bound::Text(text.clone()),
            Cell::Bytes(bytes) => Bound::Blob(bytes.to_vec()),
        }
    }
}

/// `String(row.name)`: `undefined` where the row has no such column, as a
/// row's names are its columns' as the table declares them.
pub(crate) fn text_of(cell: Option<&Cell>) -> Cow<'_, str> {
    cell.map_or(Cow::Borrowed("undefined"), Cell::text)
}

/// `left === right` of two rows' columns: `undefined` is itself alone.
pub(crate) fn same_of(left: Option<&Cell>, right: Option<&Cell>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.same(right),
        (None, None) => true,
        _ => false,
    }
}

/// `left > right` of two rows' columns: `undefined` is no number and no
/// text, never greater nor less.
pub(crate) fn greater_of(left: Option<&Cell>, right: Option<&Cell>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.greater(right),
        _ => false,
    }
}

/// The key of a row's column: `undefined` where the row has no such column.
pub(crate) fn key_of(cell: Option<&Cell>, keys: &mut Keys) -> Key {
    cell.map_or(Key::Undefined, |cell| cell.key(keys))
}

/// Why a cell is no JSON value here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unparsed {
    /// It is no JSON: `JSON.parse` threw.
    NoJson,
    /// It is JSON this build cannot hold, nested past [`DEEPEST`] levels or
    /// with a number past a double's range, which Node read: a difference
    /// kept on purpose.
    Unheld,
}

impl Unparsed {
    /// The failure of reading `what`: `no_json` where it is no JSON, else
    /// this build's own sentence.
    pub(crate) fn said(self, what: &str, no_json: String) -> String {
        match self {
            Unparsed::NoJson => no_json,
            Unparsed::Unheld => format!(
                "{what} is JSON this build cannot hold: nested past {DEEPEST} levels, or a number past a double's range"
            ),
        }
    }
}

/// A number as JSON holds it: a whole one as an integer, as JavaScript
/// writes it, and an infinity as null.
fn number_json(number: f64) -> Value {
    if number.fract() == 0.0 && number.abs() <= EXACT {
        // Whole and within 2^53: an i64 holds it exactly.
        #[allow(clippy::cast_possible_truncation)]
        return Value::from(number as i64);
    }
    Number::from_f64(number).map_or(Value::Null, Value::Number)
}

#[cfg(test)]
mod tests;
