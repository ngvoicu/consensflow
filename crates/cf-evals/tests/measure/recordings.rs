//! The numbers `measure` gave while it read the ledger through Node's code, on the
//! file each recording was made from: what reading it with SQLite is held to,
//! every field of the answer and not only those the tests look at. A recording of
//! a real run is among them.

use std::fs;
use std::path::Path;

use crate::support::{measured, recording};

/// The names of the recordings that have a `.json`.
fn recorded() -> Vec<String> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("ledgers");
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .filter_map(|entry| {
            let file = entry.unwrap().file_name().to_string_lossy().into_owned();
            file.strip_suffix(".json").map(str::to_owned)
        })
        .collect();
    names.sort();
    names
}

#[test]
fn there_is_a_recording_of_each_shape_of_ledger_the_evals_read() {
    let names = recorded();
    assert!(names.len() >= 10, "{names:?}");
    for shape in [
        "controls",
        "long-messages",
        "chief-turns",
        "parallel-unrun",
        "parallel-cancelled",
        "chief-switch",
        "open-question-asked",
        "open-question-answered",
        "devin-chief",
        "placed",
        "eval-round-trip",
    ] {
        assert!(names.iter().any(|name| name == shape), "{shape}: {names:?}");
    }
}

#[test]
fn the_numbers_of_each_ledger_are_the_same_byte_for_byte_as_when_nodes_ledger_read_it() {
    for name in recorded() {
        // The compact text of each, in the order of its keys, which a report is
        // written in: what the recording holds is what the measure says, to the byte.
        let said = measured(&name).to_value();
        let held = recording(&name)["measure"].clone();
        assert_eq!(
            serde_json::to_string(&said).unwrap(),
            serde_json::to_string(&held).unwrap(),
            "the numbers of {name}"
        );
        assert_eq!(said, held, "the numbers of {name}");
    }
}
