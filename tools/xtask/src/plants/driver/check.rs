//! Checking whether plants apply, without changing their target files.

use std::io::Write;

use super::super::report;
use super::{fare, Family, Fare, Plant, Runner, HERE, STOPPED};
use crate::context::Context;
use crate::dispatch::Failure;

/// The first XML entity `meant` holds (`&quot;`, `&amp;`, `&#34;`), if it holds one:
/// a name copied from a report that escapes it, which is not the test's name.
pub(super) fn entity(meant: &str) -> Option<&str> {
    meant.match_indices('&').find_map(|(at, _)| {
        let body = &meant[at + 1..];
        let body = &body[..body.find(';')?];
        let number = body
            .strip_prefix("#x")
            .or_else(|| body.strip_prefix('#'))
            .filter(|digits| !digits.is_empty());
        let entity = match number {
            Some(digits) => digits.chars().all(|digit| digit.is_ascii_hexdigit()),
            None => !body.is_empty() && body.chars().all(|letter| letter.is_ascii_alphanumeric()),
        };
        entity.then(|| &meant[at..at + body.len() + 2])
    })
}

/// Whether each of `plants` applies, which plants nothing: the status is 0 where every
/// one does, or does not and is listed in `family`'s `known_unapplied` (said as known,
/// and counted apart). A listed plant that applies again fails the check, so the list
/// is emptied as its plants are mended. The commands a plant needs first are run. A
/// meant name that holds an XML entity is refused, and so is a run that cannot be run as
/// it is written (an option of `cargo test` that nextest has no word for). A plant for
/// other systems is said SKIPPED (platform), its names checked (the table's data was),
/// nothing of it prepared or applied.
pub fn check(
    context: &Context,
    family: &Family,
    plants: &[&Plant],
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> Result<i32, Failure> {
    let (mut unapplied, mut known, mut stale, mut skipped) = (0, 0, 0, 0);
    for plant in plants {
        let listed = family.known_unapplied.contains(&plant.name);
        match fare(context, plant, runner)? {
            Fare::Stopped => return Ok(STOPPED),
            Fare::Skipped => {
                skipped += 1;
                writeln!(
                    out,
                    "{}",
                    report::skipped(&plant.name, &plant.platforms, HERE)
                )?;
            }
            Fare::Applies if listed => {
                stale += 1;
                writeln!(
                    out,
                    "{}: applies, so it is not known not to apply: take it off known_unapplied in kit.toml",
                    plant.name
                )?;
            }
            Fare::Applies => {}
            Fare::NotApplies(cause) if listed => {
                known += 1;
                writeln!(out, "known not to apply: {cause}")?;
            }
            Fare::NotApplies(cause) => {
                unapplied += 1;
                writeln!(out, "{cause}")?;
            }
        }
    }
    let checked = plants.len() - skipped;
    let mut said = if unapplied == 0 && known == 0 {
        format!("{checked} plants apply")
    } else {
        format!("{} of {checked} plants apply", checked - unapplied - known)
    };
    if known > 0 {
        said.push_str(&format!(", {known} known not to apply"));
    }
    if skipped > 0 {
        said.push_str(&format!(", {skipped} skipped (platform)"));
    }
    writeln!(out, "{said}")?;
    Ok(i32::from(unapplied + stale != 0))
}
