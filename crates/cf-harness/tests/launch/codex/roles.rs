//! The role a Codex window is given, as the Codex cases of Node's role-skills
//! suite held them: its whole text, appended to the instructions Codex itself
//! has, which are read through its configuration and nothing else. Node's cases
//! also held the text a role is made of (`roleInstructions`), which the engine
//! writes and an adapter is given.

use super::*;

/// The instructions a window was launched with, as the arguments carry them.
fn instructions(argv: &[String]) -> String {
    let argument = argv
        .iter()
        .find_map(|arg| arg.strip_prefix("developer_instructions="))
        .expect("the role text rides along");
    serde_json::from_str(argument).expect("a JSON text")
}

#[test]
fn every_role_enters_codex_with_its_whole_text_already_loaded() {
    for name in ["chief", "advisor", "worker", "reviewer", "designer"] {
        let home = Home::new();
        let existing = "User instructions: preserve \"quotes\", `backticks`, $HOME\nand newlines.";
        let (adapter, _fakes) = adapter_with(&home, None, existing, true);
        let content = format!(
            "The {name}'s whole text: \"quotes\", `backticks`, $HOME\nand newlines.\n| saved-worker |"
        );
        let request = Request {
            instructions: content.clone(),
            role: name,
            ..Request::default()
        };
        let plan = prepare(&adapter, &request).unwrap();
        let loaded = instructions(&plan.argv);
        assert!(loaded.starts_with(&format!("{existing}\n\n")), "{name}");
        assert!(
            loaded.contains(&content),
            "the whole role text is loaded: {name}"
        );
        assert!(
            loaded.contains(&format!("Your ConsensFlow role is {name}.")),
            "{name}"
        );
    }
}

#[test]
fn appends_the_role_to_its_own_effective_instructions_read_through_configuration_only() {
    let home = Home::new();
    let (adapter, fakes) = adapter_with(&home, None, "existing user instructions", true);
    let request = Request {
        instructions: "# The chief\n\nconsensflow-chief".to_owned(),
        ..Request::chief()
    };
    let plan = prepare(&adapter, &request).unwrap();
    let loaded = instructions(&plan.argv);
    assert!(loaded.starts_with("existing user instructions\n\n"));
    assert!(loaded.contains("consensflow-chief"));
    assert!(!loaded.contains("consensflow-worker"));
    // Codex is asked for its configuration and for nothing else: no version, no thread.
    let written: Vec<Value> = fakes
        .processes
        .take_written()
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let methods: Vec<&str> = written
        .iter()
        .map(|line| line["method"].as_str().unwrap())
        .collect();
    assert_eq!(methods, ["initialize", "initialized", "config/read"]);
    assert_eq!(
        written[2]["params"],
        json!({ "cwd": "/work/app", "includeLayers": false })
    );
}

#[test]
fn a_window_without_its_role_text_is_refused_and_nothing_is_started_or_written() {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    let request = Request {
        instructions: String::new(),
        ..Request::default()
    };
    assert_eq!(
        prepare(&adapter, &request).err().as_deref(),
        Some("the worker window needs its role text")
    );
    assert!(fakes.processes.take_spawned().is_empty(), "no app-server");
    assert!(!Path::new(&path::join(&[&home.root, "consensflow", "integrations"])).exists());
}
