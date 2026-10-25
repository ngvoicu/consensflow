//! `cf task get T-n --transcript`: what a task's window did, from ConsensFlow's
//! copy of it. `--last N` gives the newest items, ten unless told, fifty at
//! most; `--page N` and `--find "…"` reach the rest, fifty to a page counted
//! from the newest, of every item or of the items that say the words, in any
//! case, as `cf history` reads the chief's history (the field report of
//! 2026-10-15, B7: `--last 400` never reached the window that overwrote a file).
//! A search shows each long item around the words it found, not from its start.
//! A task whose brief reached no window has no transcript, and `cf` says so.

use cf_base::js;
use cf_base::text::{utf16_len, utf16_prefix};
use cf_board::Board;
use serde_json::{Map, Value};

use super::words::{quoted, Split};
use super::{Failure, Said};

/// How much of one transcript item is shown, in UTF-16 units.
const ITEM_CHARS: usize = 600;

/// How much of an item a search shows before the words it found, in characters.
const BEFORE_FOUND: usize = 200;

/// How many items a page holds, as the board pages them.
const PER_PAGE: u64 = 50;

/// What a command asks of a transcript.
pub(super) enum Asked {
    /// Its newest items, how many as JavaScript reads the number.
    Last(f64),
    /// A page of them, from the newest, of those that say `find` when it is given.
    Page { page: u64, find: Option<String> },
}

/// What the words ask of the transcript, said wrong before the board is asked
/// anything: `--last` is for the newest items, and `--page` and `--find` for
/// the rest, so not with them; a page is a whole number from 1, and a search
/// has words.
pub(super) fn asked(words: &Split) -> Result<Asked, Failure> {
    let (page, find) = (words.value("--page"), words.value("--find"));
    if page.is_none() && find.is_none() {
        let last = words.value("--last").map_or(10.0, js::number);
        if last.fract() != 0.0 || !last.is_finite() || last < 1.0 {
            return Err(Failure::Usage("--last takes a number of items".into()));
        }
        return Ok(Asked::Last(last));
    }
    if words.value("--last").is_some() {
        return Err(Failure::Usage(
            "--last gives the newest items, and --page and --find the rest: give one or the other"
                .into(),
        ));
    }
    let page = match page {
        None => 1,
        Some(page) => page
            .parse()
            .ok()
            .filter(|page| *page >= 1)
            .ok_or_else(|| Failure::Usage("--page takes a page number: 1, 2, …".into()))?,
    };
    if find.is_some_and(|find| js::trim(find).is_empty()) {
        return Err(Failure::Usage("--find takes the words to look for".into()));
    }
    Ok(Asked::Page {
        page,
        find: find.map(str::to_owned),
    })
}

/// The transcript of task `number` as `cf` prints it, below the task's own
/// line (`head`), and as JSON with the task beside the board's answer.
pub(super) fn read(
    number: u64,
    task: Value,
    head: String,
    asked: &Asked,
    board: &Board,
) -> Result<Said, Failure> {
    let path = match asked {
        Asked::Last(last) => format!("/api/tasks/{number}/transcript?last={last}"),
        Asked::Page { page, find } => {
            let mut query = form_urlencoded::Serializer::new(String::new());
            query.append_pair("page", &page.to_string());
            if let Some(find) = find {
                query.append_pair("find", find);
            }
            format!("/api/tasks/{number}/transcript?{}", query.finish())
        }
    };
    let copy = board.get(&path)?;
    let listed = copy.list(copy.value().get("items"), "items")?;
    let at = |field: &str| copy.value().get(field).and_then(Value::as_u64);
    let reached = copy.value().get("reached") != Some(&Value::Bool(false));
    let text = match asked {
        _ if !reached => {
            "Its brief has not reached a window, so it has no transcript of its own.".to_owned()
        }
        Asked::Last(_) if at("total") == Some(0) => {
            "Its window has written nothing yet.".to_owned()
        }
        Asked::Last(_) => format!(
            "What its window did, the last {} of {} items:\n\n{}",
            listed.len(),
            js::text(copy.value().get("total")),
            shown(listed, None)
        ),
        Asked::Page { find, .. } => {
            let paged = Paged {
                number,
                page: at("page").unwrap_or_default(),
                pages: at("pages").unwrap_or_default(),
                total: at("total").unwrap_or_default(),
                found: at("found"),
                find: find.as_deref(),
            };
            paged.text(listed)
        }
    };
    let text = format!("{head}\n\n{text}");
    let mut data = Map::new();
    data.insert("task".into(), task);
    if let Value::Object(copy) = copy.into_value() {
        data.extend(copy);
    }
    Ok(Said::new(Value::Object(data), text))
}

/// A page of a transcript, as the board answered it.
struct Paged<'a> {
    number: u64,
    page: u64,
    pages: u64,
    total: u64,
    found: Option<u64>,
    find: Option<&'a str>,
}

impl Paged<'_> {
    /// What `cf` prints of the page: where it is among them, its items, and
    /// the command that reads the one before, or that it is the oldest.
    fn text(&self, listed: &[Value]) -> String {
        if self.pages == 0 {
            return match self.find {
                Some(find) => format!("Nothing its window did says {}.", quoted(find)),
                None => "Its window has written nothing yet.".to_owned(),
            };
        }
        let count = self.found.unwrap_or(self.total);
        let last = count - (self.page - 1) * PER_PAGE;
        let first = last.saturating_sub(PER_PAGE) + 1;
        let (page, pages) = (self.page, self.pages);
        let opening = match self.find {
            None => format!(
                "What its window did, page {page} of {pages}: items {first} to {last} of {}.",
                self.total
            ),
            Some(find) => format!(
                "What its window did that says {}, page {page} of {pages}: {count} of its {} items say it.",
                quoted(find),
                self.total
            ),
        };
        let closing = if page < pages {
            let search = self
                .find
                .map_or_else(String::new, |find| format!(" --find {}", quoted(find)));
            format!(
                "Older: cf task get T-{} --transcript --page {}{search}",
                self.number,
                page + 1
            )
        } else {
            "This is the oldest page.".to_owned()
        };
        format!("{opening}\n\n{}\n\n{closing}", shown(listed, self.find))
    }
}

/// The items as `cf` prints them: whose each is, and its text cut to what is
/// shown of one, around the words `find` looks for where it looks.
fn shown(listed: &[Value], find: Option<&str>) -> String {
    listed
        .iter()
        .map(|item| {
            let role = match item.get("role").and_then(Value::as_str) {
                Some("user") => "Sent to the window".into(),
                Some("assistant") => "The agent".into(),
                Some("tool") => "Tool output".into(),
                Some("custom") => "Note".into(),
                _ => js::text(item.get("role")),
            };
            let writing = if js::truthy(item.get("complete")) {
                ""
            } else {
                " · still writing"
            };
            let text = js::text(item.get("text"));
            let cut = match find {
                Some(find) => clip_around(&text, find, ITEM_CHARS),
                None => clip(&text, ITEM_CHARS),
            };
            format!("[{role}{writing}]\n{cut}")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// `text` cut to `units` UTF-16 units, with an ellipsis when it was longer.
fn clip(text: &str, units: usize) -> String {
    if utf16_len(text) > units {
        format!("{}…", utf16_prefix(text, units))
    } else {
        text.to_string()
    }
}

/// `text` cut to `units` UTF-16 units around the first place it says `find`,
/// in any case: from a little before it, with an ellipsis for what is left out
/// on either side. Where it says nothing of it, as [`clip`] cuts it.
fn clip_around(text: &str, find: &str, units: usize) -> String {
    if utf16_len(text) <= units {
        return text.to_string();
    }
    let Some(found) = first_place(text, find) else {
        return clip(text, units);
    };
    let starts = text[..found].chars().count().saturating_sub(BEFORE_FOUND);
    let from = text
        .char_indices()
        .nth(starts)
        .map_or(0, |(offset, _)| offset);
    let rest = &text[from..];
    let kept = utf16_prefix(rest, units);
    format!(
        "{}{kept}{}",
        if from > 0 { "…" } else { "" },
        if kept.len() < rest.len() { "…" } else { "" }
    )
}

/// Where `text` first says `find`, in any case: the offset it begins at.
fn first_place(text: &str, find: &str) -> Option<usize> {
    let wanted: Vec<char> = find.chars().flat_map(char::to_lowercase).collect();
    text.char_indices()
        .map(|(offset, _)| offset)
        .find(|offset| {
            let mut said = text[*offset..].chars().flat_map(char::to_lowercase);
            wanted.iter().all(|want| said.next() == Some(*want))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_shows_a_long_item_around_the_words_it_found_in_any_case() {
        let text = format!(
            "{}git checkout -- src/parser.rs{}",
            "x".repeat(1_000),
            "y".repeat(1_000)
        );
        let cut = clip_around(&text, "GIT CHECKOUT", 600);
        assert!(cut.starts_with('…') && cut.ends_with('…'), "{cut}");
        assert!(cut.contains("git checkout -- src/parser.rs"), "{cut}");
        assert_eq!(
            cut.chars().filter(|c| *c == 'x').count(),
            200,
            "a little before it"
        );
        // Short items are whole, and one near its start is cut at its end alone.
        assert_eq!(
            clip_around("ran git checkout", "checkout", 600),
            "ran git checkout"
        );
        let near = format!("git checkout{}", "z".repeat(1_000));
        let cut = clip_around(&near, "checkout", 600);
        assert!(cut.starts_with("git checkout") && cut.ends_with('…'));
        // Where it says nothing of them, it is cut as any item is.
        assert_eq!(clip_around(&near, "absent", 600), clip(&near, 600));
    }

    #[test]
    fn the_first_place_is_found_whatever_the_case_and_past_wide_characters() {
        assert_eq!(first_place("Le Café Overwrote", "overwrote"), Some(9));
        assert_eq!(first_place("漢字 GIT", "git"), Some(7));
        assert_eq!(first_place("nothing here", "git"), None);
    }
}
