//! `cf <verb> --help` in a window, end to end against the real daemon
//! (`npm run test:integration`): every verb of the board answers `--help` and
//! `-h` with its usage and exit code 0, and asks the board nothing: what the
//! chief's report of 2026-10-08 found posted as a note saying "--help" (m-1542)
//! and refused as no task is the usage. A text that holds the word among
//! others, or follows a `--`, is still text.

use cf_e2e::rig::{write_staff, Project, OPEN};
use regex::Regex;
use serde_json::{json, Value};

use crate::{rig, Outcome};

/// Every command of the board that takes words, as the usage names it.
const VERBS: [&[&str]; 18] = [
    &["note"],
    &["ask"],
    &["tell"],
    &["answer"],
    &["inbox"],
    &["inbox", "read"],
    &["staff"],
    &["whoami"],
    &["history"],
    &["task", "add"],
    &["task", "list"],
    &["task", "get"],
    &["task", "done"],
    &["task", "accept"],
    &["task", "cancel"],
    &["task", "reopen"],
    &["task", "pause"],
    &["task", "resume"],
];

/// What is on the board at all: the human's inbox, the chief's, and the number
/// of every task.
fn everything(project: &Project) -> cf_e2e::Result<Value> {
    let tasks: Vec<Value> = project.board()?["lanes"]
        .as_array()
        .map(|lanes| {
            lanes
                .iter()
                .flat_map(|lane| lane["tasks"].as_array().cloned().unwrap_or_default())
                .map(|task| task["number"].clone())
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({
        "human": project.inbox("human")?,
        "chief": project.inbox("chief")?,
        "tasks": tasks,
    }))
}

#[test]
fn a_window_asks_every_verb_of_cf_for_help_and_nothing_is_posted_or_read() -> Outcome {
    let rig = rig()?;
    write_staff(&rig)?;
    let project = Project::open_with_staff(&rig, "chief", None, &[("worker", "worker")])?;
    let frame = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let before = everything(&project)?;

    for verb in VERBS {
        for word in ["--help", "-h"] {
            let mut words = verb.to_vec();
            words.push(word);
            let ran = rig.cf_in_window(&frame, &words)?;
            let said = format!("cf {} {word}", verb.join(" "));
            assert_eq!(ran.code, Some(0), "{said}: {}", ran.stderr);
            assert_eq!(ran.stderr, "", "{said}");
            // The command's own line of the usage, in the list of them.
            let usage = Regex::new(&format!(r"^  cf {} ", verb[0]))?;
            assert!(usage.is_match(&ran.stdout), "{said}: {}", ran.stdout);
        }
    }
    assert_eq!(
        everything(&project)?,
        before,
        "the board is as it was: nothing posted"
    );

    // The same word as text, which a command takes as it takes any.
    let noted = rig.cf_in_window(&frame, ["note", "--", "--help"])?;
    assert_eq!(noted.code, Some(0), "{}", noted.stderr);
    assert!(
        Regex::new(r"^m-\d+ noted to @human; nothing waits on it\.\n$")?.is_match(&noted.stdout),
        "{}",
        noted.stdout
    );
    let seen = rig.cf_in_window(&frame, ["note", "see", "--help"])?;
    assert_eq!(seen.code, Some(0), "{}", seen.stderr);
    let mut bodies: Vec<String> = project
        .inbox("human")?
        .iter()
        .filter(|m| m["kind"] == "note")
        .map(|m| m["body"].as_str().unwrap_or_default().to_owned())
        .collect();
    bodies.sort();
    assert_eq!(bodies, ["--help", "see --help"]);
    rig.close()?;
    Ok(())
}
