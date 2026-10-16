//! A task's transcript a page at a time, and searched: `?page=` and `?find=` on
//! `GET /api/tasks/<n>/transcript`, as `cf history` pages the chief's history
//! (the field report of 2026-10-15, B7: `--last 400` never reached the window
//! that overwrote a file, as no request reaches past the newest 50). Pages are of
//! [`PER_PAGE`] items, counted from the newest: page 1 holds the newest, page 2
//! the ones before them, each oldest first. A search keeps the items whose text
//! holds its words, in any case, and pages those. `?last=` with neither reads as
//! it always did.

use cf_base::js;
use cf_proto::ledger::TaskTranscript;
use serde_json::{json, Map, Value};

use super::Failure;
use crate::api::views::value;

/// How many items a page holds: the most one request gives.
pub(super) const PER_PAGE: usize = 50;

/// What a request asks for a page of: the page as it wrote it, and its words to
/// find, an empty one being none. None when it asks for neither, and so for the
/// last items.
pub(super) struct Asked<'a> {
    page: Option<&'a str>,
    find: Option<&'a str>,
}

impl<'a> Asked<'a> {
    /// The page and the search a request asked for, if it asked for either.
    pub(super) fn of(page: Option<&'a str>, find: Option<&'a str>) -> Option<Self> {
        let find = find.filter(|find| !find.is_empty());
        (page.is_some() || find.is_some()).then_some(Self { page, find })
    }

    /// The answer: the copy's `total` items, how many hold the words (`found`,
    /// with a search), the `page` and the `pages` there are, and the page's
    /// `items`. A transcript with nothing to show is page 0 of 0, whatever was
    /// asked; a page that is none of these is refused, as `cf history` refuses
    /// one, in words that say how many there are.
    pub(super) fn answer(&self, transcript: TaskTranscript) -> Result<Value, Failure> {
        let total = transcript.total;
        let found: Vec<_> = match self.find {
            None => transcript.items,
            Some(find) => {
                let needle = find.to_lowercase();
                transcript
                    .items
                    .into_iter()
                    .filter(|item| item.text.to_lowercase().contains(&needle))
                    .collect()
            }
        };
        let count = found.len();
        let pages = count.div_ceil(PER_PAGE);
        let mut answer = Map::new();
        answer.insert("total".to_owned(), json!(total));
        if self.find.is_some() {
            answer.insert("found".to_owned(), json!(count));
        }
        if pages == 0 {
            answer.insert("page".to_owned(), json!(0));
            answer.insert("pages".to_owned(), json!(0));
            answer.insert("items".to_owned(), json!([]));
            return Ok(Value::Object(answer));
        }
        let page = js::number(self.page.unwrap_or("1"));
        // A whole number from 1 to the pages, which a double holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let in_range =
            page.is_finite() && page.fract() == 0.0 && page >= 1.0 && page <= pages as f64;
        if !in_range {
            let how_many = if pages == 1 {
                "is 1 page".to_owned()
            } else {
                format!("are {pages} pages")
            };
            return Err(Failure::refuse(
                400,
                "no-such-page",
                format!("there {how_many}"),
            ));
        }
        // In range: the cast loses nothing.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let number = page as usize;
        let end = count - (number - 1) * PER_PAGE;
        let start = end.saturating_sub(PER_PAGE);
        let items = found[start..end]
            .iter()
            .map(value)
            .collect::<Result<Vec<_>, _>>()?;
        answer.insert("page".to_owned(), json!(number));
        answer.insert("pages".to_owned(), json!(pages));
        answer.insert("items".to_owned(), Value::Array(items));
        Ok(Value::Object(answer))
    }
}
