//! Claude Code's settings are reported, never written, as Node's host-payloads
//! suite held it: the three sentences of that suite, then the events Node named
//! in every settings file the recorder played (`tests/goldens/README.md` says
//! what it holds) and the places the file may be.

// The goldens' own reading and a test's own folders: a failure is the test's.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::Path;

use cf_base::env::Env;
use cf_harness::claude::stale_hooks;
use common::Home;
use serde_json::{json, Value};

fn settings(home: &Home) -> std::path::PathBuf {
    home.user().join(".claude").join("settings.json")
}

fn seed(home: &Home, value: &Value) {
    let file = settings(home);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(
        file,
        format!("{}\n", serde_json::to_string_pretty(value).unwrap()),
    )
    .unwrap();
}

#[test]
fn names_the_events_still_holding_a_hook_of_ours() {
    let home = Home::new();
    seed(
        &home,
        &json!({
            "model": "opus",
            "hooks": {
                "SessionStart": [{ "hooks": [{ "type": "command", "command": "node /x/consensflow/hook.mjs" }] }],
                "Stop": [{ "hooks": [{ "type": "command", "command": "echo hello" }] }],
            },
        }),
    );

    let stale = stale_hooks(&home.env());

    assert_eq!(
        stale.events,
        ["SessionStart"],
        "ours is named, theirs is not"
    );
    assert_eq!(Path::new(&stale.path), settings(&home));
}

#[test]
fn leaves_the_file_untouched_byte_for_byte() {
    let home = Home::new();
    seed(
        &home,
        &json!({ "hooks": { "SessionStart": [{ "command": "consensflow" }] } }),
    );
    let before = fs::read(settings(&home)).unwrap();
    let mtime = fs::metadata(settings(&home)).unwrap().modified().unwrap();

    stale_hooks(&home.env());

    assert_eq!(
        fs::read(settings(&home)).unwrap(),
        before,
        "not ours to write"
    );
    assert_eq!(
        fs::metadata(settings(&home)).unwrap().modified().unwrap(),
        mtime
    );
}

#[test]
fn reports_nothing_for_settings_with_no_hooks_and_never_creates_the_file() {
    let home = Home::new();
    let stale = stale_hooks(&home.env());
    assert_eq!(stale.events, Vec::<String>::new());
    assert!(!settings(&home).exists());
    assert!(!home.user().join(".claude").exists());
    assert_eq!(stale.report(), None);
}

#[test]
fn says_what_it_found_in_the_line_doctor_prints() {
    let home = Home::new();
    seed(
        &home,
        &json!({ "hooks": { "SessionStart": [{ "c": "consensflow" }], "Stop": [{ "c": "consensflow" }] } }),
    );
    let stale = stale_hooks(&home.env());
    assert_eq!(
        stale.report().unwrap(),
        format!(
            "hooks:        SessionStart, Stop in {} still reference consensflow — no version answers them; remove those entries",
            settings(&home).display()
        )
    );
}

#[test]
fn every_settings_file_node_was_played_is_read_as_node_read_it() {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join("claude")
        .join("stale-hooks.json");
    let golden: Value = serde_json::from_str(&fs::read_to_string(golden).unwrap()).unwrap();
    let cases = golden["settings"].as_array().unwrap();
    assert!(cases.len() >= 35);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let home = Home::new();
        let file = settings(&home);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        if let Some(text) = case["text"].as_str() {
            fs::write(&file, text).unwrap();
        } else if let Some(hex) = case["hex"].as_str() {
            let bytes: Vec<u8> = (0..hex.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
                .collect();
            fs::write(&file, bytes).unwrap();
        }

        let events = stale_hooks(&home.env()).events;

        match case["events"].as_array() {
            Some(node) => {
                let node: Vec<&str> = node.iter().map(|event| event.as_str().unwrap()).collect();
                assert_eq!(events, node, "{name}");
            }
            // Where Node threw (`null.hooks`, a TypeError its own runtime made
            // of the JSON `null`) this says nothing: no sentence of ours.
            None => {
                assert!(case["throws"].as_str().is_some(), "{name}");
                assert_eq!(events, Vec::<String>::new(), "{name}");
            }
        }
    }
}

#[test]
fn the_settings_are_where_claude_config_dir_says_even_if_it_says_nothing_else_in_the_home() {
    let home = Home::new();
    let elsewhere = home.root().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(
        elsewhere.join("settings.json"),
        r#"{"hooks":{"Stop":[{"c":"consensflow"}]}}"#,
    )
    .unwrap();
    // A hook in the user's own `.claude`, which is not where it is told to look.
    seed(
        &home,
        &json!({ "hooks": { "Start": [{ "c": "consensflow" }] } }),
    );
    let env = Env::from_vars([
        ("HOME", home.user().to_string_lossy().into_owned()),
        (
            "CLAUDE_CONFIG_DIR",
            elsewhere.to_string_lossy().into_owned(),
        ),
    ]);

    let stale = stale_hooks(&env);

    assert_eq!(stale.events, ["Stop"]);
    assert_eq!(Path::new(&stale.path), elsewhere.join("settings.json"));
}

#[test]
fn without_a_claude_config_dir_the_settings_are_in_dot_claude_of_the_home() {
    let home = Home::new();
    seed(
        &home,
        &json!({ "hooks": { "Start": [{ "c": "consensflow" }] } }),
    );
    for variable in ["HOME", "USERPROFILE"] {
        let env = Env::from_vars([(variable, home.user().to_string_lossy().into_owned())]);
        let stale = stale_hooks(&env);
        assert_eq!(stale.events, ["Start"], "{variable}");
        assert_eq!(Path::new(&stale.path), settings(&home), "{variable}");
    }
    // HOME wins over USERPROFILE, whichever is set, empty or not.
    let both = Env::from_vars([
        ("HOME", home.user().to_string_lossy().into_owned()),
        (
            "USERPROFILE",
            home.root().join("other").to_string_lossy().into_owned(),
        ),
    ]);
    assert_eq!(stale_hooks(&both).events, ["Start"]);
}

#[test]
fn an_empty_claude_config_dir_is_the_working_folder_as_node_kept_it_and_no_home_is_no_failure() {
    // `CLAUDE_CONFIG_DIR ?? ~/.claude`: an empty value is kept, and is the
    // folder `settings.json` is looked for in; no home at all is an empty home.
    let empty = Env::from_vars([("CLAUDE_CONFIG_DIR", "")]);
    assert_eq!(stale_hooks(&empty).path, "settings.json");
    let none = Env::default();
    assert_eq!(
        stale_hooks(&none).path,
        cf_base::path::join(&[".claude", "settings.json"])
    );
    assert_eq!(stale_hooks(&none).events, Vec::<String>::new());
}

#[test]
fn json_nested_past_what_this_reads_and_a_number_past_a_double_are_no_json_here_as_documented() {
    // Kept from Node on purpose (the module says so): Node read both, and
    // named the event, where this reads neither and names none.
    let home = Home::new();
    let file = settings(&home);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let deep = format!(
        r#"{{"hooks":{{"A":[{}"consensflow"{}]}}}}"#,
        "[".repeat(200),
        "]".repeat(200)
    );
    fs::write(&file, deep).unwrap();
    assert_eq!(stale_hooks(&home.env()).events, Vec::<String>::new());
    fs::write(&file, r#"{"hooks":{"A":[{"n":1e999,"c":"consensflow"}]}}"#).unwrap();
    assert_eq!(stale_hooks(&home.env()).events, Vec::<String>::new());
}
