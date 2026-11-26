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

/// What the participant row keeps as the hold's form: the column itself.
fn column(ledger: &Ledger, member: i64) -> Option<String> {
    ledger
        .sqlite()
        .query_row(
            "SELECT out_form FROM participant WHERE id = ?",
            [member],
            |row| row.get(0),
        )
        .unwrap()
}

fn pi_member(made: &ProjectView) -> i64 {
    made.participants
        .iter()
        .find(|p| p.harness.as_deref() == Some("pi"))
        .unwrap()
        .id
}

const FOREVER: &str = "9999-12-31T23:59:59.998Z";

#[test]
fn the_form_lives_in_the_hold_s_own_state_and_every_statement_that_clears_or_replaces_the_hold_clears_it(
) {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let made = project(&mut ledger);
    let member = pi_member(&made);
    let held = |ledger: &mut Ledger, member: i64| {
        ledger
            .mark_out_with_form(
                member,
                FOREVER,
                "account unavailable",
                login(Some(401), Some("authentication_error")),
            )
            .unwrap();
        assert!(column(ledger, member).is_some(), "the hold wrote its form");
    };
    // 1. The hold's own statement writes it, as the closed form's JSON.
    held(&mut ledger, member);
    assert_eq!(
        serde_json::from_str::<Value>(&column(&ledger, member).unwrap()).unwrap(),
        json!({ "kind": "login", "status": 401, "code": "authentication_error" })
    );
    // 2. mark_back (the human's Try again now, a window's answer, an edit that lifts it).
    ledger.mark_back(member, "the human logged in").unwrap();
    assert_eq!(column(&ledger, member), None, "mark_back");
    // 3. A hold that changes kind, here an account's form replaced by a moved model's hold.
    held(&mut ledger, member);
    ledger
        .mark_out_with_kind(
            member,
            "9999-12-31T23:59:59.999Z",
            "model moved",
            HoldKind::Moved,
        )
        .unwrap();
    assert_eq!(
        column(&ledger, member),
        None,
        "an account's hold replaced by another kind"
    );
    ledger.mark_back(member, "back").unwrap();
    // ... a quota hold and a busy hold, and an account hold with no form of its own.
    for (until, reason, kind) in [
        ("9999-12-31T23:59:59.997Z", "out of quota", HoldKind::Quota),
        ("9999-12-31T23:59:59.997Z", "model busy", HoldKind::Busy),
    ] {
        held(&mut ledger, member);
        ledger.mark_back(member, "back").unwrap();
        ledger
            .mark_out_with_kind(member, until, reason, kind)
            .unwrap();
        assert_eq!(column(&ledger, member), None, "{reason}");
        ledger.mark_back(member, "back").unwrap();
    }
    held(&mut ledger, member);
    ledger.mark_back(member, "back").unwrap();
    ledger
        .mark_out_with_form(member, FOREVER, "account unavailable", None)
        .unwrap();
    assert_eq!(
        column(&ledger, member),
        None,
        "a hold with no form writes none"
    );
    ledger.mark_back(member, "back").unwrap();
    // 4. A hold made before the kind was kept, given its kind and form on the same end.
    ledger
        .sqlite()
        .execute(
            "UPDATE participant SET out_until = ?, out_since = ?, out_kind = NULL, out_form = NULL WHERE id = ?",
            rusqlite::params![FOREVER, "2026-11-26T12:00:00.000Z", member],
        )
        .unwrap();
    ledger
        .mark_out_with_form(
            member,
            FOREVER,
            "account unavailable",
            login(None, Some("invalid_grant")),
        )
        .unwrap();
    assert!(
        column(&ledger, member).is_some(),
        "the kind and the form set together"
    );
    ledger.mark_back(member, "back").unwrap();
    // 5. The chief's switch: the chief held, then switched to another harness.
    let chief = made
        .participants
        .iter()
        .find(|p| p.handle == "chief")
        .unwrap()
        .id;
    held(&mut ledger, chief);
    ledger
        .switch_chief(
            made.id,
            &cf_ledger::ChiefSwitch {
                harness: "pi".into(),
                model: "claude-fable-5-2".into(),
                effort: None,
                model_name: None,
                cut: false,
            },
        )
        .unwrap();
    assert_eq!(column(&ledger, chief), None, "the chief's switch");
    // 6. An edit that keeps the hold keeps it; one that lifts it clears it.
    held(&mut ledger, member);
    let handle = ledger
        .board(made.id)
        .map(|b| serde_json::to_value(b).unwrap())
        .unwrap()["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["participant"]["id"] == member)
        .unwrap()["participant"]["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    ledger
        .edit_member(
            made.id,
            &handle,
            &cf_ledger::MemberEdit {
                harness: "pi".into(),
                model: Some("claude-fable-5-2".into()),
                effort: Some("high".into()),
                model_name: None,
                tier: "standard".into(),
            },
        )
        .unwrap();
    assert!(
        column(&ledger, member).is_some(),
        "an edit that keeps the hold keeps its form"
    );
}

#[test]
fn a_form_outside_the_lists_or_not_a_form_reads_as_no_form_and_the_board_loads() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let made = project(&mut ledger);
    let member = pi_member(&made);
    ledger
        .mark_out_with_form(
            member,
            FOREVER,
            "account unavailable",
            login(Some(401), None),
        )
        .unwrap();
    for stored in [
        r#"{"kind":"login","status":"401"}"#,
        r#"{"kind":"login","status":499}"#,
        r#"{"kind":"login","code":"owner@example.invalid"}"#,
        r#"{"kind":"login","code":7}"#,
        r#"{"kind":"billing","code":"authentication_error"}"#,
        r#"{"kind":"moved"}"#,
        r#"{"kind":5}"#,
        r#"{"status":401}"#,
        r#"[]"#,
        "not json at all",
        "",
    ] {
        ledger
            .sqlite()
            .execute(
                "UPDATE participant SET out_form = ? WHERE id = ?",
                rusqlite::params![stored, member],
            )
            .unwrap();
        let said = lane_of(&ledger.board(made.id).unwrap(), member);
        assert!(said.get("outForm").is_none(), "{stored}: {said}");
        assert_eq!(
            said["outKind"], "account",
            "the hold itself stands: {stored}"
        );
    }
    ledger
        .sqlite()
        .execute(
            "UPDATE participant SET out_form = ? WHERE id = ?",
            rusqlite::params![
                r#"{"kind":"login","status":401,"code":"invalid_grant"}"#,
                member
            ],
        )
        .unwrap();
    let said = lane_of(&ledger.board(made.id).unwrap(), member);
    assert_eq!(
        said["outForm"],
        json!({ "kind": "login", "status": 401, "code": "invalid_grant" })
    );
}

/// The check's two sequences of finding 21 (T-2364): the form is the hold's own state, so a
/// later hold with none, or another member's, can neither bring a cleared form back nor hide a
/// present one.
#[test]
fn a_cleared_form_never_comes_back_and_another_member_s_event_never_hides_a_present_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let (made, a, b) = two_members(&mut ledger);
    let billing = HoldForm::checked("billing", Some(402), Some("insufficient_quota"));
    // First: a billing hold cleared, a hold with no form, an effort-only rename.
    ledger
        .mark_out_with_form(a, FOREVER, "account unavailable", billing)
        .unwrap();
    ledger.mark_back(a, "paid").unwrap();
    ledger
        .mark_out_with_kind(a, FOREVER, "account unavailable", HoldKind::Account)
        .unwrap();
    effort_of(&mut ledger, made.id, a, "high");
    let said = lane_of(&ledger.board(made.id).unwrap(), a);
    assert_eq!(said["outKind"], "account");
    assert!(
        said.get("outForm").is_none(),
        "a cleared form came back: {said}"
    );
    // Second: a held member's form stays while another takes its freed handle and is held
    // with no form.
    ledger.mark_back(a, "back").unwrap();
    ledger
        .mark_out_with_form(a, FOREVER, "account unavailable", login(Some(401), None))
        .unwrap();
    let freed = effort_of(&mut ledger, made.id, b, "xhigh");
    assert_eq!(freed.id, b);
    effort_of(&mut ledger, made.id, a, "low");
    ledger
        .mark_out_with_kind(b, FOREVER, "account unavailable", HoldKind::Account)
        .unwrap();
    let board = ledger.board(made.id).unwrap();
    assert_eq!(
        lane_of(&board, a)["outForm"],
        json!({ "kind": "login", "status": 401 })
    );
    assert!(lane_of(&board, b).get("outForm").is_none());
}

/// A ledger a build before 0048 left, with holds in it: every row kept, the column NULL, the
/// holds say the generic sentence (no form), and a second open writes nothing. An older build
/// never reads the column: it refuses a ledger of a newer version before it reads anything.
#[test]
fn a_ledger_with_holds_from_before_the_column_gains_it_null_keeps_every_row_and_a_second_open_writes_nothing(
) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("ledger.db");
    let mut ledger = open_ledger(&file, Options::default()).unwrap();
    let made = project(&mut ledger);
    let member = pi_member(&made);
    ledger
        .mark_out_with_form(
            member,
            FOREVER,
            "account unavailable",
            login(Some(401), None),
        )
        .unwrap();
    ledger.close().unwrap();
    // As the build before 0048 left it.
    let raw = rusqlite::Connection::open(&file).unwrap();
    cf_ledger::testing::undo_migration_0048(&raw).unwrap();
    raw.execute_batch(&format!(
        "PRAGMA user_version = {}",
        cf_ledger::SCHEMA_VERSION - 1
    ))
    .unwrap();
    let rows = |db: &rusqlite::Connection| -> Vec<String> {
        let mut out = Vec::new();
        for table in ["participant", "event", "project", "task", "message"] {
            let columns: Vec<String> = db
                .prepare(&format!(
                    "SELECT name FROM pragma_table_info('{table}') WHERE name != 'out_form' ORDER BY cid"
                ))
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            let select = columns
                .iter()
                .map(|c| format!("quote(\"{c}\")"))
                .collect::<Vec<_>>()
                .join("||'|'||");
            out.extend(
                db.prepare(&format!(
                    "SELECT '{table}:'||{select} FROM {table} ORDER BY rowid"
                ))
                .unwrap()
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .map(Result::unwrap),
            );
        }
        out
    };
    let before = rows(&raw);
    drop(raw);
    let mut ledger = open_ledger(&file, Options::default()).unwrap();
    assert_eq!(rows(ledger.sqlite()), before, "every row kept");
    assert_eq!(
        column(&ledger, member),
        None,
        "a hold from before has no form"
    );
    let said = lane_of(&ledger.board(made.id).unwrap(), member);
    assert_eq!(said["outKind"], "account");
    assert!(said.get("outForm").is_none(), "{said}");
    ledger.close().unwrap();
    let ledger = open_ledger(&file, Options::default()).unwrap();
    ledger.close().unwrap();
    let again = rusqlite::Connection::open(&file).unwrap();
    assert_eq!(rows(&again), before, "a second open writes no row");
}
