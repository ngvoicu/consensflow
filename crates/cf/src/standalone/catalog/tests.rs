//! Which harnesses `cf catalog` lists: the catalog's, in its order, or the one
//! asked for. What it prints of them is held to Node's recording.

use cf_catalog::{Catalog, CatalogEntry};

use super::selected;

fn names<'a>(listed: &[(&'a str, &[CatalogEntry])]) -> Vec<&'a str> {
    listed.iter().map(|(harness, _)| *harness).collect()
}

#[test]
fn every_harness_is_listed_in_the_order_its_first_preset_comes_in() {
    let catalog = Catalog::bundled().unwrap();
    let listed = selected(catalog.groups(), None);
    assert_eq!(
        names(&listed),
        ["devin", "codex", "pi", "opencode", "claude"]
    );
    assert!(listed.iter().all(|(_, entries)| !entries.is_empty()));
}

#[test]
fn one_harness_is_listed_with_its_entries_and_a_name_the_catalog_has_none_of_with_none() {
    let catalog = Catalog::bundled().unwrap();
    let claude = selected(catalog.groups(), Some("claude"));
    assert_eq!(names(&claude), ["claude"]);
    assert!(claude[0].1.iter().any(|entry| entry.name == "zeus"));
    // A name is a harness as it is spelled: its kind is none, nor is a capital.
    // Every other name is one too, those a plain object answers for among them.
    for name in [
        "nope",
        "",
        "claude-code",
        "Claude",
        "constructor",
        "__proto__",
        "toString",
        "hasOwnProperty",
    ] {
        let listed = selected(catalog.groups(), Some(name));
        assert_eq!(names(&listed), [name]);
        assert!(listed[0].1.is_empty(), "{name:?}");
    }
}
