//! The tests under `describe('an agents file that cannot be read')` in Node's
//! roster suite.

use super::*;
use serde_json::json;

#[test]
fn is_said_never_read_as_an_empty_roster_and_never_saved_over() {
    let home = Home::new();
    let catalog = catalog();
    let roster = home.roster(&catalog);
    roster
        .add(
            &body(json!({ "name": "mybuilder", "harness": "claude", "model": "claude-opus-5-5" })),
            &mut Now,
        )
        .unwrap();
    // A hand edit's trailing comma.
    let text = home.text();
    let at = text.rfind("\n  ]").unwrap();
    let broken = format!("{},{}", &text[..at], &text[at..]);
    home.write(&broken);
    let said = |message: String| {
        assert!(
            message.contains("agents.json is not valid JSON: fix it or move it away"),
            "{message}"
        );
    };
    said(roster.list().unwrap_err().message);
    said(roster.agent_row("mybuilder").unwrap_err().message);
    said(roster.preferences().unwrap_err().message);
    said(roster.normalize().unwrap_err().message);
    said(
        roster
            .set_preferences(Some(&json!({ "ownHarnessOnly": true })))
            .unwrap_err()
            .message,
    );
    said(
        roster
            .add(
                &body(json!({ "name": "other", "harness": "codex", "model": "gpt-6-astra" })),
                &mut Now,
            )
            .unwrap_err()
            .message,
    );
    said(
        roster
            .edit(
                "mybuilder",
                &body(json!({ "model": "claude-sonnet-5-5" })),
                &mut Now,
            )
            .unwrap_err()
            .message,
    );
    said(roster.remove("mybuilder").unwrap_err().message);
    assert_eq!(home.text(), broken, "the file is as the human left it");
    home.write("null");
    assert!(roster
        .list()
        .unwrap_err()
        .message
        .contains("is not an agents file"));
}

#[test]
fn is_written_whole_or_not_at_all_beside_itself_and_then_in_its_place() {
    let home = Home::new();
    let catalog = catalog();
    let roster = home.roster(&catalog);
    roster
        .add(
            &body(json!({ "name": "mybuilder", "harness": "claude", "model": "claude-opus-5-5" })),
            &mut Now,
        )
        .unwrap();
    let file = home.path();
    let identity = || cf_base::file::identity(&fs::File::open(&file).unwrap()).unwrap();
    let before = identity();
    roster
        .set_preferences(Some(&json!({ "ownHarnessOnly": true })))
        .unwrap();
    assert_ne!(identity(), before, "a new file took its place");
    let left: Vec<_> = fs::read_dir(file.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(left, ["agents.json"], "nothing is left beside it");
    assert!(roster.preferences().unwrap().own_harness_only);
}
