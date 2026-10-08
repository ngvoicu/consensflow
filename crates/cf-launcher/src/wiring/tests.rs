use super::*;

fn wiring(shape: Shape, exists: bool, mine: bool) -> Wiring {
    Wiring {
        shape,
        runtime: "/App/MacOS/node".to_owned(),
        entry: "/App/Resources/cli/bin/cf.mjs".to_owned(),
        exists,
        mine,
        live: false,
    }
}

#[test]
fn the_old_shape_is_reported_in_node_s_three_sentences() {
    assert_eq!(
        wiring(Shape::Node, false, false).report(),
        "runtime:      /App/MacOS/node — MISSING. Reinstall from the app to point the wiring at its runtime."
    );
    // Missing comes first: a runtime that is not there is not the copy asking.
    assert_eq!(
        wiring(Shape::Node, false, true).report(),
        "runtime:      /App/MacOS/node — MISSING. Reinstall from the app to point the wiring at its runtime."
    );
    assert_eq!(
        wiring(Shape::Node, true, true).report(),
        "runtime:      /App/MacOS/node"
    );
    assert_eq!(
        wiring(Shape::Node, true, false).report(),
        "runtime:      /App/MacOS/node — another ConsensFlow. `cf` runs that one; `cf setup` from this one claims the command."
    );
}

#[test]
fn the_new_shape_is_reported_in_the_same_three_states_of_its_own_words() {
    let native = |exists, mine| Wiring {
        runtime: "/App/cli/bin/cf".to_owned(),
        entry: "/App/cli/bin/cf".to_owned(),
        ..wiring(Shape::Native, exists, mine)
    };
    assert_eq!(
        native(false, false).report(),
        "command:      /App/cli/bin/cf — MISSING. Reinstall from the app to point the command at its cf."
    );
    assert_eq!(native(true, true).report(), "command:      /App/cli/bin/cf");
    assert_eq!(
        native(true, false).report(),
        "command:      /App/cli/bin/cf — another ConsensFlow. `cf` runs that one; `cf setup` from this one claims the command."
    );
}

#[test]
fn the_label_of_a_line_is_as_wide_as_doctors_other_labels() {
    // `home:         `, `harnesses:    `, `agents:       `, `roles:        `.
    for shape in [Shape::Node, Shape::Native] {
        let line = wiring(shape, true, true).report();
        assert_eq!(line.find('/'), Some(14), "{line}");
    }
}

#[cfg(not(windows))]
#[test]
fn the_folder_above_a_path_is_what_node_s_dirname_says() {
    // What `path.posix.dirname` answered, as the launcher's recording holds it
    // (`tests/goldens/README.md`).
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join("readings.json");
    let golden: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let cases = golden["dirnames"].as_array().unwrap();
    assert!(cases.len() >= 15);
    for case in cases {
        let (path, expected) = (
            case["path"].as_str().unwrap(),
            case["dirname"].as_str().unwrap(),
        );
        assert_eq!(dirname(path), expected, "{path:?}");
    }
}

#[cfg(windows)]
#[test]
fn on_windows_either_separator_ends_a_segment() {
    assert_eq!(dirname(r"C:\App\cli\bin\cf.exe"), r"C:\App\cli\bin");
    assert_eq!(dirname("C:/App/cli/bin/cf.exe"), "C:/App/cli/bin");
}
