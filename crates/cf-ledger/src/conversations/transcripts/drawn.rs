//! What the history draws of an item besides its words, as the copy keeps it
//! and reads it back.

use cf_proto::ledger::ItemDetail;
use rusqlite::{params, Connection};
use serde_json::Value;

use super::{number, TRANSCRIPT_ITEM_MAX};
use crate::conversations::ID_SHOWN;
use crate::model::cut;

/// What the history draws of an item besides its words, as the copy keeps it: what the
/// item's `detail` says, each field read as the ledger takes it (a kind or a status it does
/// not know is none, a header cut to [`HEADER_MAX`] characters, a name to [`NAME_MAX`], a
/// change to [`TRANSCRIPT_ITEM_MAX`]). None where the item says no kind the ledger knows:
/// it draws as a plain one, of its role.
#[derive(Debug, PartialEq)]
pub(super) struct Drawn {
    pub(super) kind: &'static str,
    pub(super) name: Option<String>,
    pub(super) header: Option<String>,
    pub(super) status: Option<&'static str>,
    pub(super) took: Option<u64>,
    pub(super) tokens: Option<u64>,
    pub(super) diff: Option<String>,
}

/// The kinds of item the history draws.
const KINDS: [&str; 6] = ["prompt", "text", "thinking", "tool", "compaction", "notice"];

/// How a tool's call may stand.
const STATUSES: [&str; 3] = ["running", "ok", "failed"];

/// The most characters of a call's header the copy keeps: as many as a page shows of an
/// id ([`ID_SHOWN`]).
pub(crate) const HEADER_MAX: usize = ID_SHOWN;

/// The most characters of a tool's name the copy keeps.
pub(crate) const NAME_MAX: usize = 64;

impl Drawn {
    pub(super) fn of(item: &Value) -> Option<Self> {
        let detail = item.get("detail")?;
        let word = |name: &str, words: &[&'static str]| {
            let said = detail.get(name).and_then(Value::as_str)?;
            words.iter().copied().find(|word| *word == said)
        };
        let text = |name: &str, max: usize| {
            detail
                .get(name)
                .and_then(Value::as_str)
                .map(|text| text.chars().take(max).collect::<String>())
        };
        Some(Self {
            kind: word("kind", &KINDS)?,
            name: text("name", NAME_MAX),
            header: text("header", HEADER_MAX),
            status: word("status", &STATUSES),
            took: detail.get("tookMs").and_then(Value::as_u64),
            tokens: detail.get("tokens").and_then(Value::as_u64),
            diff: detail
                .get("diff")
                .and_then(Value::as_str)
                .map(|diff| cut(diff, TRANSCRIPT_ITEM_MAX)),
        })
    }

    /// As the item's row keeps it: JSON of what it says, in one order, so that the same
    /// is the same text.
    pub(super) fn json(&self) -> String {
        serde_json::to_string(&self.detail()).unwrap_or_default()
    }

    /// As a reader of the ledger hands it out.
    fn detail(&self) -> ItemDetail {
        ItemDetail {
            kind: self.kind.to_owned(),
            name: self.name.clone(),
            header: self.header.clone(),
            status: self.status.map(str::to_owned),
            took_ms: self.took,
            tokens: self.tokens,
            diff: self.diff.clone(),
        }
    }
}

/// What an item's row keeps of what the history draws of it, read back: none for none, or
/// for JSON no build wrote.
pub(crate) fn detail_of(json: Option<&str>) -> Option<ItemDetail> {
    serde_json::from_str(json?).ok()
}

/// Writes what the history draws of the item at row `item` besides its words where the
/// viewer reads it: its short fields in its head, and the change an edit made in a row of
/// its own (taken away where there is none). An item that says none draws as a plain one.
pub(super) fn write_drawn(
    db: &Connection,
    item: i64,
    drawn: Option<&Drawn>,
) -> rusqlite::Result<()> {
    let diff = drawn.and_then(|drawn| drawn.diff.as_deref());
    db.prepare_cached(
        "UPDATE transcript_head SET kind = ?2, name = ?3, header = ?4, status = ?5, took = ?6,
           tokens = ?7, diff_bytes = ?8
         WHERE item = ?1",
    )?
    .execute(params![
        item,
        drawn.map(|drawn| drawn.kind),
        drawn.and_then(|drawn| drawn.name.as_deref()),
        drawn.and_then(|drawn| drawn.header.as_deref()),
        drawn.and_then(|drawn| drawn.status),
        drawn
            .and_then(|drawn| drawn.took)
            .map(|took| i64::try_from(took).unwrap_or(i64::MAX)),
        drawn
            .and_then(|drawn| drawn.tokens)
            .map(|tokens| i64::try_from(tokens).unwrap_or(i64::MAX)),
        number(diff.map_or(0, str::len)),
    ])?;
    match diff {
        Some(diff) => db
            .prepare_cached(
                "INSERT INTO transcript_diff (item, text) VALUES (?, ?)
           ON CONFLICT (item) DO UPDATE SET text = excluded.text
             WHERE transcript_diff.text != excluded.text",
            )?
            .execute(params![item, diff])?,
        None => db
            .prepare_cached("DELETE FROM transcript_diff WHERE item = ?")?
            .execute([item])?,
    };
    Ok(())
}
