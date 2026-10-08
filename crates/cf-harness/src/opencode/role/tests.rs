//! The configurations Node's role code merged a role into, each as Node
//! (v26.8.1) wrote the result.

use std::fs;

use serde_json::json;

use super::*;

const SKILLS: &str = "/role/.claude/skills";
const FILE: &str = "/role/.claude/skills/consensflow-worker/SKILL.md";

/// `current` with the role merged in, or why not.
fn merge(current: &str) -> Result<String, String> {
    merged(current, SKILLS, FILE)
}

#[test]
fn a_configuration_of_nothing_gets_the_roles_skills_folder_and_file() {
    let expected = format!(r#"{{"skills":{{"paths":["{SKILLS}"]}},"instructions":["{FILE}"]}}"#);
    assert_eq!(merge("{}").unwrap(), expected);
}

#[test]
fn what_the_human_set_is_kept_where_it_was_and_the_role_added_after_it() {
    let current = r#"{"theme":"user","skills":{"paths":["/p"],"urls":["u"]},"instructions":["/i"],"z":1,"1":2}"#;
    assert_eq!(
        merge(current).unwrap(),
        format!(
            r#"{{"1":2,"theme":"user","skills":{{"paths":["/p","{SKILLS}"],"urls":["u"]}},"instructions":["/i","{FILE}"],"z":1}}"#
        ),
        "an index key first, as JavaScript writes them, and the rest where they were"
    );
}

#[test]
fn a_configuration_already_merged_is_merged_again_to_the_same_one() {
    let first = merge(r#"{"instructions":["/i"]}"#).unwrap();
    assert_eq!(merge(&first).unwrap(), first);
}

#[test]
fn each_path_is_once_where_it_first_is_the_ones_the_human_repeated_too() {
    assert_eq!(
        merge(r#"{"skills":{"paths":["a","a","b"]},"instructions":["r","r"]}"#).unwrap(),
        format!(r#"{{"skills":{{"paths":["a","b","{SKILLS}"]}},"instructions":["r","{FILE}"]}}"#)
    );
}

#[test]
fn skill_paths_that_are_none_or_null_are_none_and_any_other_that_is_no_list_of_texts_is_refused() {
    let paths = format!(r#""paths":["{SKILLS}"]"#);
    for current in [r#"{"skills":null}"#, r#"{"skills":{"paths":null}}"#] {
        let merged = merge(current).unwrap();
        assert!(
            merged.starts_with(&format!(r#"{{"skills":{{{paths}}}"#)),
            "{merged}"
        );
    }
    for current in [
        r#"{"skills":{"paths":"x"}}"#,
        r#"{"skills":{"paths":[1]}}"#,
        r#"{"skills":{"paths":{}}}"#,
    ] {
        assert_eq!(
            merge(current).unwrap_err(),
            "OpenCode skill paths must be an array of paths",
            "{current}"
        );
    }
}

#[test]
fn instructions_that_are_no_list_of_texts_are_refused_a_null_one_too() {
    for current in [
        r#"{"instructions":"rules.md"}"#,
        r#"{"instructions":null}"#,
        r#"{"instructions":{}}"#,
        r#"{"instructions":7}"#,
        r#"{"instructions":["rules.md",7]}"#,
    ] {
        assert_eq!(
            merge(current).unwrap_err(),
            "OpenCode instructions must be an array of paths",
            "{current}"
        );
    }
}

#[test]
fn the_skill_paths_are_looked_at_first() {
    assert_eq!(
        merge(r#"{"skills":{"paths":"x"},"instructions":7}"#).unwrap_err(),
        "OpenCode skill paths must be an array of paths"
    );
}

#[test]
fn a_configuration_that_is_no_object_is_refused_as_one_and_no_json_as_that() {
    for current in ["null", "[]", "7", r#""x""#, "true", "false", "0"] {
        assert_eq!(
            merge(current).unwrap_err(),
            "OpenCode process configuration must be an object",
            "{current}"
        );
    }
    for current in ["not json", " ", "{", "{} {}"] {
        assert_eq!(
            merge(current).unwrap_err(),
            "OpenCode process configuration must be JSON",
            "{current:?}"
        );
    }
}

#[test]
fn a_skills_setting_that_is_no_object_is_spread_as_javascript_spreads_it() {
    let with = |skills: Value| {
        js::stringify(&json!({
            "skills": skills,
            "instructions": [FILE],
        }))
    };
    // Node: {"skills":"abc"} came out with "0":"a","1":"b","2":"c" in it.
    assert_eq!(
        merge(r#"{"skills":"abc"}"#).unwrap(),
        with(json!({ "0": "a", "1": "b", "2": "c", "paths": [SKILLS] }))
    );
    assert_eq!(
        merge(r#"{"skills":["a","b"]}"#).unwrap(),
        with(json!({ "0": "a", "1": "b", "paths": [SKILLS] }))
    );
    assert_eq!(
        merge(r#"{"skills":5}"#).unwrap(),
        with(json!({ "paths": [SKILLS] }))
    );
    // A pair past the plane holds two halves in Node, which a Rust text cannot.
    assert_eq!(
        merge(r#"{"skills":"a😀"}"#).unwrap(),
        with(json!({ "0": "a", "1": "\u{FFFD}", "2": "\u{FFFD}", "paths": [SKILLS] }))
    );
}

#[test]
fn a_key_written_twice_is_its_last_value_where_it_was_first_written() {
    assert_eq!(
        merge(r#"{"a":1,"a":2}"#).unwrap(),
        format!(r#"{{"a":2,"skills":{{"paths":["{SKILLS}"]}},"instructions":["{FILE}"]}}"#)
    );
}

#[test]
fn the_role_is_written_in_the_launchs_folder_before_the_configuration_is_looked_at() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().to_string_lossy().into_owned();
    let launch = LaunchId::new("0a1b2c3d-4e5f-4061-8a7b-9c0d1e2f3a4b").unwrap();
    let env = |config: Option<&str>| {
        let mut vars = vec![
            ("HOME".to_owned(), home.clone()),
            ("CONSENSFLOW_HOME".to_owned(), path::join(&[&home, "app"])),
        ];
        vars.extend(config.map(|config| ("OPENCODE_CONFIG_CONTENT".to_owned(), config.to_owned())));
        Env::from_vars(vars)
    };
    let refused = configure(&env(Some("[]")), &launch, "worker", "role text");
    assert_eq!(
        refused.unwrap_err(),
        "OpenCode process configuration must be an object"
    );
    let role = path::join(&[
        &home,
        "app",
        "integrations",
        "opencode",
        launch.as_str(),
        "role",
        ".claude",
        "skills",
    ]);
    let file = path::join(&[&role, "consensflow-worker", "SKILL.md"]);
    assert_eq!(fs::read_to_string(&file).unwrap(), "role text");
    for config in [None, Some("")] {
        let made = configure(&env(config), &launch, "worker", "role text").unwrap();
        assert_eq!(
            made,
            js::stringify(&serde_json::json!({
                "skills": { "paths": [role] },
                "instructions": [file],
            })),
            "an empty one is none"
        );
    }
}

#[test]
fn a_window_without_its_role_text_is_refused_before_the_configuration_is_looked_at() {
    let dir = tempfile::tempdir().unwrap();
    let env = Env::from_vars([
        (
            "CONSENSFLOW_HOME",
            dir.path().to_string_lossy().into_owned(),
        ),
        ("OPENCODE_CONFIG_CONTENT", "not json".to_owned()),
    ]);
    let launch = LaunchId::new("0a1b2c3d-4e5f-4061-8a7b-9c0d1e2f3a4b").unwrap();
    assert_eq!(
        configure(&env, &launch, "worker", "").unwrap_err(),
        "the worker window needs its role text"
    );
    assert!(!dir.path().join("integrations").exists());
}
