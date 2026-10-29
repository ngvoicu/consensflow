//! The role a Codex window is given, as the Codex cases of Node's role-skills
//! suite held them: its whole text, appended to the instructions Codex itself
//! has, which are read through its configuration and nothing else. Node's cases
//! also held the text a role is made of (`roleInstructions`), which the engine
//! writes and an adapter is given.

use super::*;

#[test]
fn every_role_enters_codex_with_its_whole_text_already_loaded() {
    for name in [
        "chief", "advisor", "worker", "reviewer", "tester", "designer",
    ] {
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
        let loaded = role_file(&plan);
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
    let loaded = role_file(&plan);
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

/// The words of a role a command line must not hold: each line of its text that is
/// long enough to be the role's and no other word's.
fn role_lines(text: &str) -> Vec<&str> {
    text.lines().filter(|line| line.len() >= 12).collect()
}

/// That `argv` holds nothing of `role`, nor the setting that carried it.
fn holds_no_role(argv: &[String], role: &str, case: &str) {
    for word in argv {
        assert!(
            !word.contains("developer_instructions"),
            "{case}: the role is on the command line: {word}"
        );
        for line in role_lines(role) {
            assert!(
                !word.contains(line),
                "{case}: the command line holds the role's {line:?}"
            );
        }
    }
}

/// Every role text is on the command line of no Codex window, as both platforms'
/// recordings of the launch hold them: anyone on the machine reads a command line
/// (`ps` is setuid root on macOS), and a pattern that matched its words matched the
/// window. It goes by the file the window's environment names.
#[test]
fn no_codex_launch_of_either_platform_puts_its_role_on_a_command_line() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens/launch");
    for platform in ["darwin", "win32"] {
        let file = folder.join(format!("scenarios.{platform}.json"));
        let scenarios: Vec<Value> =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        let mut launches = 0;
        for scenario in scenarios
            .iter()
            .filter(|scenario| scenario["harness"] == "codex")
        {
            for record in scenario["records"].as_array().unwrap() {
                for settled in record["settled"].as_array().into_iter().flatten() {
                    let Some(argv) = settled["answer"]["argv"].as_array() else {
                        continue;
                    };
                    let step = &scenario["steps"]
                        [usize::try_from(settled["op"].as_u64().unwrap()).unwrap()];
                    let role = step["prepare"]["instructions"].as_str().unwrap_or_default();
                    let argv: Vec<String> = argv
                        .iter()
                        .map(|word| word.as_str().unwrap().to_owned())
                        .collect();
                    holds_no_role(&argv, role, &format!("{platform}: {}", scenario["name"]));
                    let named = settled["answer"]["env"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|pair| pair[0] == "CF_CODEX_ROLE_FILE");
                    assert!(
                        named,
                        "{platform}: {}: no role file is named",
                        scenario["name"]
                    );
                    launches += 1;
                }
            }
        }
        assert!(
            launches > 50,
            "{platform}: {launches} Codex launches recorded"
        );
    }
}

/// The adapter itself, on this platform: no window of any role, opened afresh or on
/// a thread, has its role or Codex's own instructions on its command line; the file
/// its environment names holds them whole.
#[test]
fn no_codex_window_this_adapter_launches_has_its_role_on_its_command_line() {
    let existing = "Codex's own instructions, which begin the developer's.";
    for name in [
        "chief", "advisor", "worker", "reviewer", "tester", "designer",
    ] {
        for resume in [None, Some(THREAD)] {
            let home = Home::new();
            let (adapter, _fakes) = adapter_with(&home, None, existing, true);
            let content = format!("# The {name}\n\nThe {name}'s role text, whole.");
            let request = Request {
                instructions: content.clone(),
                role: name,
                resume: resume.map(str::to_owned),
                message: resume.is_none().then(|| TASK.to_owned()),
                ..Request::default()
            };
            let plan = prepare(&adapter, &request).unwrap();
            let case = format!("{name}, resumed: {}", resume.is_some());
            holds_no_role(&plan.argv, &content, &case);
            holds_no_role(&plan.argv, existing, &case);
            let loaded = role_file(&plan);
            assert!(
                loaded.starts_with(existing) && loaded.ends_with(&content),
                "{case}"
            );
        }
    }
}
