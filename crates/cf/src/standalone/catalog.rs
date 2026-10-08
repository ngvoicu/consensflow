//! `cf catalog [--harness <h>] [--json]`: the ready-made agents of every
//! harness, or of one, as a table or as JSON. It reads nothing of the home and
//! writes nothing.

use std::io::Write;

use cf_base::args::{self, Opt, Positionals};
use cf_base::js;
use cf_base::text::{pad_end, utf16_len};
use cf_catalog::CatalogEntry;
use serde_json::{json, Map};

use super::{bundled, to_json, Done, Stop};

const OPTIONS: [Opt; 2] = [Opt::text("harness"), Opt::flag("json")];

/// What the table ends with.
const FOOTER: &str = "every one of them is in your agents already; define your own with `cf agent add <name> --harness … --model …`";

/// Each harness to list, with its entries: all of them in the catalog's
/// order, or the one asked for, whose entries are none where the catalog has
/// none under that name.
///
/// Node asked a plain object for the harness, which answers for the names every
/// object has (`constructor`, `__proto__`, `toString`…): that listed a
/// function, and the table crashed on it (`entries.map is not a function`) after
/// its first line. Here such a name is a harness the catalog has no entries
/// for, like any other (a difference kept on purpose, recorded as such).
fn selected<'a>(
    groups: &'a [cf_catalog::Group],
    harness: Option<&'a str>,
) -> Vec<(&'a str, &'a [CatalogEntry])> {
    match harness {
        None => groups
            .iter()
            .map(|group| (group.harness.as_str(), group.entries.as_slice()))
            .collect(),
        Some(harness) => {
            let entries = groups
                .iter()
                .find(|group| group.harness.as_str() == harness)
                .map_or(&[][..], |group| group.entries.as_slice());
            vec![(harness, entries)]
        }
    }
}

pub(super) fn run(words: &[String], out: &mut dyn Write) -> Done {
    let parsed = args::parse(words, &OPTIONS, Positionals::Allowed).map_err(Stop::Said)?;
    let catalog = bundled()?;
    let listed = selected(catalog.groups(), parsed.text("harness"));
    if parsed.flag("json") {
        let mut harnesses = Map::new();
        for (harness, entries) in &listed {
            harnesses.insert((*harness).to_owned(), to_json(entries)?);
        }
        let text = js::stringify_indented(&json!({ "catalog": harnesses }), 2);
        writeln!(out, "{text}")?;
        return Ok(());
    }
    for (harness, entries) in listed {
        writeln!(out, "{harness}:")?;
        // Width from the rows, not a guess: the models of OpenCode's Go and
        // Zen run to 40 characters and ran straight into the effort column.
        let model_width = entries
            .iter()
            .map(|entry| utf16_len(&entry.model) + 2)
            .fold(34, usize::max);
        for entry in entries {
            writeln!(
                out,
                "  {}{}{}{}",
                pad_end(&entry.name, 12),
                pad_end(&entry.model, model_width),
                pad_end(entry.effort.as_deref().unwrap_or("-"), 8),
                entry.description
            )?;
        }
        writeln!(out)?;
    }
    writeln!(out, "{FOOTER}")?;
    Ok(())
}

#[cfg(test)]
mod tests;
