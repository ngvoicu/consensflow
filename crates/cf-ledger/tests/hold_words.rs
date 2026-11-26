//! A member held for its account keeps a closed form of the refusal with the hold (its kind,
//! status and error type, each a literal of `cf_proto::hold`, and the member's id), and the board
//! says it with the member, whether or not any window is left to say it. No word of a harness's
//! is kept (T-2353).
#![allow(clippy::expect_used, clippy::unwrap_used)]

use cf_ledger::{
    open_ledger, HoldKind, Ledger, NewChief, NewMember, NewProject, Options, ProjectView,
};
use cf_proto::hold::HoldForm;
use serde_json::{json, Value};

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

fn login(status: Option<u64>, code: Option<&str>) -> Option<HoldForm> {
    HoldForm::checked("login", status, code)
}

/// The data of the `member.out` events of a project.
fn out_events(ledger: &Ledger, project: i64) -> Vec<Value> {
    ledger
        .events(project, 0, 1000)
        .unwrap()
        .into_iter()
        .filter(|event| event.kind == "member.out")
        .map(|event| event.data)
        .collect()
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
fn the_board_says_the_form_an_account_hold_was_made_from_and_the_event_keeps_no_word() {
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
        .mark_out_with_form(
            member,
            "9999-12-31T23:59:59.998Z",
            "account unavailable",
            login(Some(401), Some("authentication_error")),
        )
        .unwrap();
    let said = lane_of(&ledger.board(made.id).unwrap(), member);
    assert_eq!(said["outKind"], "account");
    assert_eq!(
        said["outForm"],
        json!({ "kind": "login", "status": 401, "code": "authentication_error" })
    );
    assert!(said.get("outBecause").is_none(), "{said}");
    // The event keeps the form and the member's id, and no other key of any word.
    let outs = out_events(&ledger, made.id);
    assert_eq!(outs.len(), 1);
    let mut keys: Vec<&String> = outs[0].as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["form", "handle", "participant", "reason", "until"]);
    assert_eq!(outs[0]["participant"], member);
}

#[test]
fn a_hold_of_a_quota_a_busy_model_or_a_moved_one_keeps_nothing_new_and_an_ended_hold_says_none() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let made = project(&mut ledger);
    let member = made
        .participants
        .iter()
        .find(|p| p.harness.as_deref() == Some("pi"))
        .unwrap()
        .id;
    for (kind, reason) in [
        (HoldKind::Quota, "out of quota"),
        (HoldKind::Busy, "model busy"),
        (HoldKind::Moved, "model moved"),
    ] {
        ledger
            .mark_out_with_kind(member, "9999-12-31T23:59:59.997Z", reason, kind)
            .unwrap();
        let said = lane_of(&ledger.board(made.id).unwrap(), member);
        assert!(said.get("outForm").is_none(), "{reason}: {said}");
        ledger.mark_back(member, "back").unwrap();
    }
    for out in out_events(&ledger, made.id) {
        let keys: Vec<&String> = out.as_object().unwrap().keys().collect();
        assert!(
            !keys
                .iter()
                .any(|key| *key == "form" || *key == "participant"),
            "{out}"
        );
    }
    // A hold ended says none.
    ledger
        .mark_out_with_form(
            member,
            "9999-12-31T23:59:59.998Z",
            "account unavailable",
            login(Some(402), None),
        )
        .unwrap();
    ledger.mark_back(member, "the human logged in").unwrap();
    let said = lane_of(&ledger.board(made.id).unwrap(), member);
    assert!(said.get("outForm").is_none(), "{said}");
}

#[test]
fn an_edit_that_keeps_the_hold_and_renames_the_member_keeps_the_holds_form() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let made = project(&mut ledger);
    let held = made
        .participants
        .iter()
        .find(|p| p.harness.as_deref() == Some("pi"))
        .unwrap();
    let (member, handle) = (held.id, held.handle.clone());
    ledger
        .mark_out_with_form(
            member,
            "9999-12-31T23:59:59.998Z",
            "account unavailable",
            login(Some(401), Some("invalid_api_key")),
        )
        .unwrap();
    // Only its effort changes: the name follows it, and the hold, which this edit keeps, stays.
    let edited = ledger
        .edit_member(
            made.id,
            &handle,
            &cf_ledger::MemberEdit {
                harness: "pi".into(),
                model: Some("claude-fable-5-2".into()),
                effort: Some("xhigh".into()),
                model_name: None,
                tier: "standard".into(),
            },
        )
        .unwrap();
    assert_ne!(edited.handle, handle, "the effort renames the member");
    assert!(edited.out_until.is_some(), "the edit kept the hold");
    let said = lane_of(&ledger.board(made.id).unwrap(), member);
    assert_eq!(
        said["outForm"],
        json!({ "kind": "login", "status": 401, "code": "invalid_api_key" })
    );
}

#[test]
fn a_hold_without_a_form_says_none_and_never_an_earlier_hold_s_form() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let made = project(&mut ledger);
    let member = made
        .participants
        .iter()
        .find(|p| p.harness.as_deref() == Some("pi"))
        .unwrap()
        .id;
    let until = "9999-12-31T23:59:59.998Z";
    ledger
        .mark_out_with_form(
            member,
            until,
            "account unavailable",
            HoldForm::checked("billing", Some(402), Some("insufficient_quota")),
        )
        .unwrap();
    ledger.mark_back(member, "paid").unwrap();
    // The next hold is an older build's, or one whose reason had no form: no id, no form.
    ledger
        .mark_out_with_kind(member, until, "account unavailable", HoldKind::Account)
        .unwrap();
    let said = lane_of(&ledger.board(made.id).unwrap(), member);
    assert_eq!(said["outKind"], "account");
    assert!(
        said.get("outForm").is_none(),
        "an earlier hold's form stood in: {said}"
    );
    // And a hold with a form after it says its own.
    ledger.mark_back(member, "fixed").unwrap();
    ledger
        .mark_out_with_form(member, until, "account unavailable", login(None, None))
        .unwrap();
    let said = lane_of(&ledger.board(made.id).unwrap(), member);
    assert_eq!(said["outForm"], json!({ "kind": "login" }));
}

/// A project of two Pi members of one model and two efforts: the effort is in a member's handle,
/// so an edit of it renames the member and frees its handle for the other one to take.
fn two_members(ledger: &mut Ledger) -> (ProjectView, i64, i64) {
    let member = |effort: &str, role: &str| NewMember {
        harness: "pi".into(),
        model: Some("claude-fable-5-2".into()),
        effort: Some(effort.into()),
        model_name: None,
        designer: false,
        roles: vec![role.into()],
        tier: "standard".into(),
    };
    let made = ledger
        .create_project_overflowing(&NewProject {
            directory: "/work/words2".into(),
            name: "words2".into(),
            chief: NewChief {
                harness: "codex".into(),
                model: None,
                effort: None,
                model_name: None,
            },
            staff: vec![member("low", "frontend"), member("xhigh", "tester")],
            gate: false,
            wake_up: None,
            specs: None,
            staff_rules: None,
            compaction: None,
            advisor_tool: false,
            prompt_suggestion: false,
            own_hands: false,
        })
        .expect("a project");
    let pis: Vec<&cf_ledger::ParticipantView> = made
        .participants
        .iter()
        .filter(|p| p.harness.as_deref() == Some("pi"))
        .collect();
    let (a, b) = (pis[0].id, pis[1].id);
    (made, a, b)
}

fn effort_of(
    ledger: &mut Ledger,
    project: i64,
    member: i64,
    effort: &str,
) -> cf_ledger::ParticipantView {
    let handle = ledger
        .board(project)
        .map(|board| serde_json::to_value(board).unwrap())
        .unwrap()["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|lane| lane["participant"]["id"] == member)
        .unwrap()["participant"]["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    ledger
        .edit_member(
            project,
            &handle,
            &cf_ledger::MemberEdit {
                harness: "pi".into(),
                model: Some("claude-fable-5-2".into()),
                effort: Some(effort.into()),
                model_name: None,
                tier: "standard".into(),
            },
        )
        .unwrap()
}

/// A hold's form belongs to the member by its identity, never by a handle (T-2353, the check
/// of T-2350): a handle a member left is taken by another, held or not yet, and neither shows
/// the other's words.
fn two_holds_one_handle(the_taker_is_held_first: bool) {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let (made, a, b) = two_members(&mut ledger);
    let first = login(Some(401), Some("authentication_error"));
    let second = HoldForm::checked("billing", Some(402), Some("insufficient_quota"));
    let mark = |ledger: &mut Ledger, member, form| {
        ledger
            .mark_out_with_form(
                member,
                "9999-12-31T23:59:59.998Z",
                "account unavailable",
                form,
            )
            .unwrap();
    };
    if the_taker_is_held_first {
        mark(&mut ledger, b, second);
    }
    mark(&mut ledger, a, first);
    // A changes its effort, which renames it and frees its handle; B takes that handle.
    let renamed = effort_of(&mut ledger, made.id, a, "high");
    assert_eq!(renamed.id, a);
    let taker = effort_of(&mut ledger, made.id, b, "low");
    assert_eq!(taker.id, b);
    if !the_taker_is_held_first {
        mark(&mut ledger, b, second);
    }
    let board = ledger.board(made.id).unwrap();
    assert_eq!(
        lane_of(&board, a)["outForm"]["kind"],
        "login",
        "the first member"
    );
    assert_eq!(
        lane_of(&board, b)["outForm"]["kind"],
        "billing",
        "the second member"
    );
}

#[test]
fn a_member_that_takes_the_handle_another_left_says_its_own_form_when_held_after_it() {
    two_holds_one_handle(false);
}

#[test]
fn a_member_that_takes_the_handle_another_left_says_its_own_form_when_held_before_it() {
    two_holds_one_handle(true);
}
