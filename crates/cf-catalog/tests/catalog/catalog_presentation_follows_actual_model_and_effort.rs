//! The tests under `describe('catalog presentation follows actual model and
//! effort')` of Node's catalog suite.

use super::*;

#[test]
fn gives_every_curated_entry_an_explicit_model_route_and_practical_description() {
    let catalog = catalog();
    for (_, entry) in entries(&catalog) {
        assert!(!entry.profile.model_key.is_empty(), "{}", entry.name);
        assert!(!entry.profile.model_label.is_empty(), "{}", entry.name);
        assert!(!entry.profile.route_label.is_empty(), "{}", entry.name);
        let written = serde_json::to_value(&entry.profile).unwrap();
        assert!(written.get("categories").is_none(), "no role pills");
    }
}

#[test]
fn unifies_reviewed_provider_aliases_while_keeping_model_snapshots_distinct() {
    let catalog = catalog();
    let key = |name: &str| found(&catalog, name).entry.profile.model_key;
    assert_eq!(key("astraeus"), "gpt-6-astra");
    assert_eq!(key("phosphoros"), "gpt-6-astra");
    assert_eq!(key("aurvandil"), "gpt-6-astra");
    assert_eq!(key("logi"), key("gefjon"));
    assert_ne!(key("freya"), key("hades"));
}
