//! A member held for its account keeps the harness's own words with the hold, and the board
//! says them with the member, whether or not any window is left to say them.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use cf_ledger::{
    open_ledger, HoldKind, Ledger, NewChief, NewMember, NewProject, Options, ProjectView,
};
use serde_json::Value;

fn project(ledger: &mut Ledger) -> ProjectView {
    ledger
        .create_project_overflowing(&NewProject {
            directory: "/work/words".into(),
            name: "words".into(),
            chief: NewChief {
                harness: "codex".into(),
                model: None,
                effort: None,
                model_name: None,
            },
            staff: vec![NewMember {
                harness: "pi".into(),
                model: Some("claude-fable-5-2".into()),
                effort: None,
                model_name: None,
                designer: false,
                roles: vec!["frontend".into()],
                tier: "standard".into(),
            }],
            gate: false,
            wake_up: None,
            specs: None,
            staff_rules: None,
            compaction: None,
            advisor_tool: false,
            prompt_suggestion: false,
            own_hands: false,
        })
        .expect("a project")
}

fn lane_of(board: &cf_ledger::Board, member: i64) -> Value {
    let board = serde_json::to_value(board).unwrap();
    board["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|lane| lane["participant"]["id"] == member)
        .unwrap()["participant"]
        .clone()
}

#[test]
fn the_board_says_the_words_a_hold_was_made_from() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let made = project(&mut ledger);
    let member = made
        .participants
        .iter()
        .find(|p| p.harness.as_deref() == Some("pi"))
        .unwrap()
        .id;
    let words = "its login was refused: 401 authentication_error: invalid x-api-key";
    ledger
        .mark_out_saying(
            member,
            "9999-12-31T23:59:59.998Z",
            "account unavailable",
            HoldKind::Account,
            words,
        )
        .unwrap();
    let said = lane_of(&ledger.board(made.id).unwrap(), member);
    assert_eq!(said["outKind"], "account");
    assert_eq!(said["outBecause"], words);
}

#[test]
fn a_hold_of_a_quota_and_a_hold_ended_say_no_words() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let made = project(&mut ledger);
    let member = made
        .participants
        .iter()
        .find(|p| p.harness.as_deref() == Some("pi"))
        .unwrap()
        .id;
    ledger
        .mark_out_saying(
            member,
            "9999-12-31T23:59:59.998Z",
            "account unavailable",
            HoldKind::Account,
            "its billing was refused: 402",
        )
        .unwrap();
    ledger.mark_back(member, "the human logged in").unwrap();
    let said = lane_of(&ledger.board(made.id).unwrap(), member);
    assert!(said.get("outBecause").is_none(), "{said}");
    // A later hold of another kind does not keep the earlier one's words.
    ledger
        .mark_out_with_kind(
            member,
            "9999-12-31T23:59:59.997Z",
            "out of quota",
            HoldKind::Quota,
        )
        .unwrap();
    let said = lane_of(&ledger.board(made.id).unwrap(), member);
    assert!(said.get("outBecause").is_none(), "{said}");
}
