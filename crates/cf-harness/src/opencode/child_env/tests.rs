//! The `childEnv` of `tests/goldens/launch/tables.json`, each as Node made
//! the environment.

use std::collections::BTreeMap;

use serde_json::Value;

use super::*;

/// An environment as the table writes it: its variables by name.
fn variables(env: &Env) -> BTreeMap<String, String> {
    env.iter()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect()
}

/// An object of texts as pairs.
fn pairs(object: &Value) -> Vec<(String, String)> {
    object
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, value)| (name.clone(), value.as_str().unwrap().to_owned()))
        .collect()
}

#[test]
fn every_environment_of_the_table_is_made_as_node_made_it() {
    let tables: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/goldens/launch/tables.json"
    )))
    .unwrap();
    let rows = tables["childEnv"].as_array().unwrap();
    for row in rows {
        let base = Env::from_vars(pairs(&row["base"]));
        let added = pairs(&row["declared"]["env"]);
        let added: Vec<(&str, &str)> = added
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let dropped: Vec<&str> = row["declared"]["dropEnv"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|key| key.as_str().unwrap())
            .collect();
        let made = child_env(
            &base,
            &Declared {
                env: &added,
                drop_env: &dropped,
            },
        );
        let expected: BTreeMap<String, String> = pairs(&row["env"]).into_iter().collect();
        assert_eq!(variables(&made), expected, "{row}");
    }
    assert_eq!(rows.len(), 7);
}

#[test]
fn what_the_engine_runs_with_loses_the_keys_of_cmux_and_nothing_else() {
    let base = Env::from_vars([
        ("PATH", "/bin"),
        ("CMUX_SOCKET_PATH", "/s"),
        ("CMUX_CLAUDE_HOOK_CMUX_BIN", "/c"),
        ("CMUX_OTHER", "kept"),
    ]);
    assert_eq!(
        variables(&child_env(&base, &Declared::default())),
        BTreeMap::from([
            ("CMUX_OTHER".to_owned(), "kept".to_owned()),
            ("PATH".to_owned(), "/bin".to_owned()),
        ])
    );
}
