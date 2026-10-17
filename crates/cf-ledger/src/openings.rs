//! The first bytes of a value that may be long, read alone through SQLite's
//! incremental blob reads: a read of a column whole loads all of it, however
//! little of it is used, where this reads the pages its first bytes are on.

use std::ffi::CStr;

use rusqlite::{Connection, MAIN_DB};

/// The first `most` bytes of column `column` of row `row` of table `table`,
/// as text: a character they cut short at their end is left out.
pub(crate) fn opening_of(
    db: &Connection,
    table: &CStr,
    column: &CStr,
    row: i64,
    most: usize,
) -> rusqlite::Result<String> {
    let blob = db.blob_open(MAIN_DB, table, column, row, true)?;
    let mut bytes = vec![0; most.min(blob.len())];
    let read = blob.read_at(&mut bytes, 0)?;
    bytes.truncate(read);
    let end = match std::str::from_utf8(&bytes) {
        Err(cut) if cut.error_len().is_none() => cut.valid_up_to(),
        _ => bytes.len(),
    };
    Ok(text_of(&bytes[..end]))
}

/// A text's `bytes` as text: what is no UTF-8 in them (which no build writes,
/// each writing its language's own strings) is U+FFFD, never refused.
pub(crate) fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_no_utf_8_in_a_text_is_a_replacement() {
        assert_eq!(text_of("aé🙂".as_bytes()), "aé🙂");
        assert_eq!(text_of(b"a\xED\xA0\x80b"), "a\u{FFFD}\u{FFFD}\u{FFFD}b");
    }

    #[test]
    fn reads_the_first_bytes_of_a_value_and_never_loads_it_whole_a_character_cut_short_left_out() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, body TEXT NOT NULL) STRICT")
            .unwrap();
        let body = format!("é{}", "x".repeat(100_000));
        db.execute("INSERT INTO t VALUES (1, ?)", [&body]).unwrap();
        // A read that loaded the body whole would fail.
        db.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH, 50_000)
            .unwrap();
        assert_eq!(opening_of(&db, c"t", c"body", 1, 4).unwrap(), "éxx");
        assert_eq!(opening_of(&db, c"t", c"body", 1, 2).unwrap(), "é");
        assert_eq!(opening_of(&db, c"t", c"body", 1, 1).unwrap(), "");
        assert!(opening_of(&db, c"t", c"body", 2, 4).is_err(), "no row 2");
    }
}
