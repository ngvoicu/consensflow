//! The traces the receipt and stop redesign moved on purpose, which a player
//! names with the one thing that differs from Node's, does not hold to Node's
//! answer, and holds to still departing: a trace that now answers as Node's
//! did, or that is not recorded, is a line to take out, not a test left
//! passing. Found by playing the recordings against the daemon, never by
//! guessing.

use crate::support::trace;

/// A trace the redesign moved: its name, and the one thing that differs from
/// Node's.
pub type Departed = (&'static str, &'static str);

/// The traces a player is to hold to Node's answer: `names` but for the departed.
pub fn held<'a>(names: &'a [String], departed: &[Departed]) -> Vec<&'a String> {
    names
        .iter()
        .filter(|name| !departed.iter().any(|(moved, _)| moved == name))
        .collect()
}

/// What is wrong with the list `departed` of traces of `suites`: a name that
/// is no recorded trace of them, or a trace that `holds` to Node's answer now.
pub fn wrong(departed: &[Departed], suites: &[&str], holds: impl Fn(&str) -> bool) -> Vec<String> {
    let names = trace::names(suites);
    departed
        .iter()
        .filter_map(|(name, why)| {
            if !names.iter().any(|found| found == name) {
                Some(format!("{name} is not a recorded trace ({why})"))
            } else if holds(name) {
                Some(format!("{name} is answered as Node's was now ({why})"))
            } else {
                None
            }
        })
        .collect()
}

/// A line to print for each departed trace of `names`: what it is and why.
pub fn said(names: &[String], departed: &[Departed]) -> Vec<String> {
    departed
        .iter()
        .filter(|(moved, _)| names.iter().any(|name| name == moved))
        .map(|(name, why)| format!("  departed {name}: {why}"))
        .collect()
}
