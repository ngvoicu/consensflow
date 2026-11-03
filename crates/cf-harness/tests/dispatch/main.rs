//! Where the product tells one harness from another.
//!
//! Which adapter a harness has is the registry's to say (`cf_harness::launch::adapter`);
//! what differs between harnesses belongs behind the adapters' contract
//! (`cf_harness::contract`). A `match` on `Harness` anywhere else is one more place to change
//! when a harness comes or goes, and one that no compiler error finds when the branch is a
//! `_`. This test finds them by reading the code as Rust syntax (`scan`), and holds them to
//! the list in `baseline`: a site that is not on the list fails it. The list only gets
//! shorter; a site closed is taken off it, and the test fails until that is done, so that
//! the room it leaves is not taken by another.
//!
//! What is read: every crate of the product (`crates/*` and the app) but the black-box
//! suites' stand-in agents (`cf-e2e`) and the live evals (`cf-evals`), which are tools of
//! the tests and name harnesses as their subjects, from the root of its library and of its
//! program (`lib.rs`, `main.rs`) down the modules it declares. What is not: code gated to
//! the tests or the test kit, the registry's own file, and the programs a crate has under
//! `src/bin`, which are tools of the tests and the live runs.
//!
//! What it finds is a branch on a harness's identity, by name: `Harness` as the file calls
//! it, its variants, and the strings a harness goes by (its id and its kind) in a literal or
//! in a constant of the same file. It does not follow a value: an id held in a variable and
//! compared with another variable is a branch no syntax shows. And a site is named by its
//! file and the item it is in, so a function renamed or moved reads as one site closed and
//! another grown: the way through is to close the site, not to move it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod baseline;
mod fixtures;
mod scan;

use std::collections::BTreeMap;
use std::path::Path;

use scan::{Site, Sites, Vocabulary};

/// Crates that are tools of the tests.
const TOOLS_OF_THE_TESTS: [&str; 2] = ["cf-e2e", "cf-evals"];

/// The registry: the one file that says which adapter a harness has.
const REGISTRY: [&str; 1] = ["crates/cf-harness/src/launch.rs"];

fn checkout() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
}

/// Every site of the product's crates, outside the registry.
fn the_products_sites() -> Sites {
    let words = Vocabulary::of_the_product();
    let mut all = Sites::new();
    for (name, root) in scan::crate_roots(checkout(), &TOOLS_OF_THE_TESTS).unwrap() {
        let sites = scan::read_crate(checkout(), &root, name != "cf-harness", &REGISTRY, &words)
            .unwrap_or_else(|failed| panic!("{failed}"));
        for (site, count) in sites {
            *all.entry(site).or_default() += count;
        }
    }
    all
}

/// The list as sites.
fn listed() -> Sites {
    let mut all = Sites::new();
    for (file, scope, kind, count) in baseline::SITES {
        let site = Site {
            file: (*file).to_owned(),
            scope: (*scope).to_owned(),
            kind: *kind,
        };
        assert!(
            all.insert(site.clone(), *count).is_none(),
            "{site} is on the list twice: its count says how many"
        );
    }
    all
}

/// How `found` differs from `listed`: the sites there are more of than the list allows, and
/// those there are fewer of.
fn differences(found: &Sites, listed: &Sites) -> (Vec<String>, Vec<String>) {
    let mut grown = Vec::new();
    let mut closed = Vec::new();
    let all: BTreeMap<_, _> = found
        .iter()
        .map(|(site, count)| (site, (*count, 0)))
        .chain(listed.iter().map(|(site, count)| (site, (0, *count))))
        .fold(BTreeMap::new(), |mut all, (site, (found, listed))| {
            let entry: &mut (usize, usize) = all.entry(site).or_default();
            entry.0 += found;
            entry.1 += listed;
            all
        });
    for (site, (found, listed)) in all {
        if found > listed {
            grown.push(format!("{site}: {found} where the list has {listed}"));
        } else if found < listed {
            closed.push(format!("{site}: {found} where the list has {listed}"));
        }
    }
    (grown, closed)
}

#[test]
fn a_harness_is_told_from_another_only_where_the_list_allows() {
    let (grown, closed) = differences(&the_products_sites(), &listed());
    assert!(
        grown.is_empty(),
        "the code tells one harness from another where it did not:\n  {}\n\
         Which adapter a harness has is the registry's (cf_harness::launch), and what differs \
         between them goes behind the adapters' contract. Nothing is added to the list \
         (tests/dispatch/baseline.rs).",
        grown.join("\n  ")
    );
    assert!(
        closed.is_empty(),
        "sites on the list are closed, or are fewer:\n  {}\nTake them off the list \
         (tests/dispatch/baseline.rs), so that the room they leave is not taken by another.",
        closed.join("\n  ")
    );
}

#[test]
fn the_list_is_kept_in_order_so_that_what_is_closed_is_found_where_it_was() {
    let order: Vec<_> = baseline::SITES
        .iter()
        .map(|(file, scope, kind, _)| (*file, *scope, *kind))
        .collect();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(
        order, sorted,
        "the list is not in order of file, item and kind"
    );
}

#[test]
fn every_harness_has_a_module_of_its_id_in_the_harnesses_crate_which_is_what_an_adapter_path_names()
{
    let source = checkout().join("crates/cf-harness/src");
    for id in &Vocabulary::of_the_product().ids {
        assert!(
            source.join(format!("{id}.rs")).is_file() || source.join(id).join("mod.rs").is_file(),
            "no module {id} in cf-harness: a path into it is not found by the id"
        );
    }
}
