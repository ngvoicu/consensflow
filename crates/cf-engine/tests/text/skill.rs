//! The staff table the chief reads: the cases of Node's skill suite.

use cf_engine::roles::{team_table, work_tier_list, StaffRow};
use cf_proto::agents::WorkTier;

use super::support::staff_row;

#[test]
fn shows_one_row_per_member_name_roles_and_work_tier_and_nothing_to_pick_a_member_by() {
    let table = team_table(&[
        staff_row("zeus", &["worker", "reviewer"], WorkTier::Standard),
        staff_row("diana", &["advisor"], WorkTier::Light),
    ])
    .unwrap();
    assert_eq!(
        table,
        [
            "| Member | Roles | Work tier |",
            "|---|---|---|",
            "| zeus (Opus · high) | worker, reviewer | Standard work |",
            "| diana (Opus · high) | advisor | Light work |",
        ]
        .join("\n")
    );
}

/// A member is named by its model (T-1263): the name says what it runs on, so the table
/// says the model once, in the name, and has no cell for it (T-1234: a large staff's text
/// was read in part again once the table named every model twice). A member whose name
/// holds no model (on its harness's default, or one that needs an edit) has it beside the
/// name.
#[test]
fn names_a_model_only_where_the_member_s_name_does_not() {
    let named = |name: &str, roles: &[&str], tier, model: Option<&str>| StaffRow {
        model: model.map(str::to_owned),
        ..staff_row(name, roles, tier)
    };
    let table = team_table(&[
        named(
            "claude-opus-5-5-high",
            &["worker"],
            WorkTier::Standard,
            None,
        ),
        named(
            "codex-default",
            &["designer"],
            WorkTier::Light,
            Some("Codex default"),
        ),
        named(
            "old-one",
            &["reviewer"],
            WorkTier::Complex,
            Some("no model: needs an edit"),
        ),
    ])
    .unwrap();
    assert_eq!(
        table,
        [
            "| Member | Roles | Work tier |",
            "|---|---|---|",
            "| claude-opus-5-5-high | worker | Standard work |",
            "| codex-default (Codex default) | image designer | Light work |",
            "| old-one (no model: needs an edit) | reviewer | Complex work |",
        ]
        .join("\n")
    );
    assert!(!table.contains("Model"), "no column of models: {table}");
}

#[test]
fn says_so_when_the_staff_is_empty() {
    assert!(team_table(&[])
        .unwrap()
        .starts_with("Nobody is on the staff yet"));
}

#[test]
fn lists_the_four_work_tiers_one_line_each() {
    let list = work_tier_list();
    let lines: Vec<&str> = list.split('\n').collect();
    assert_eq!(lines.len(), 4);
    assert!(lines[0].starts_with("- Critical work: "));
    assert!(lines[3].starts_with("- Light work: "));
}

/// A member named by its model (T-1263) is read off the project with no model beside its name:
/// the name says it. One whose name holds none (on its harness's default, or one that needs an
/// edit) has what it runs on beside it (T-1234).
#[test]
fn a_member_named_by_its_model_is_read_off_the_project_with_no_model_beside_its_name() {
    use cf_engine::roles::staff_of;
    use serde_json::json;

    use super::support::{participant_of, project_of};

    let row = |id: i64, handle: &str, model: Option<&str>, effort: Option<&str>| {
        json!({
            "id": id, "projectId": 1, "handle": handle, "role": "worker", "roles": ["worker"],
            "harness": "claude-code", "model": model, "effort": effort, "modelName": model,
            "designer": false, "createdAt": "2026-11-08T10:00:00.000Z", "tier": "standard",
        })
    };
    let project = project_of(
        [
            row(
                3,
                "claude-opus-5-5-high",
                Some("claude-opus-5-5"),
                Some("high"),
            ),
            row(4, "zeus", Some("claude-opus-5-5"), Some("high")),
            row(5, "claude-default", None, None),
        ]
        .iter()
        .map(participant_of)
        .collect(),
    );
    let staff = staff_of(&project);
    assert_eq!(
        staff
            .iter()
            .map(|member| (member.name.as_str(), member.model.as_deref()))
            .collect::<Vec<_>>(),
        [
            ("claude-opus-5-5-high", None),
            ("zeus", Some("claude-opus-5-5 · high")),
            ("claude-default", Some("Claude Code default")),
        ]
    );
}

/// A design task has no tier (`--design`, no tier), so the chief's table says none of an image
/// designer, which the project reads without one; any other member with none is refused
/// as it was.
#[test]
fn says_none_for_an_image_designer_s_tier_and_refuses_any_other_member_with_none() {
    use cf_engine::roles::RoleError;

    let designer = StaffRow {
        work_tier: None,
        ..staff_row("codex-default", &["designer"], WorkTier::Light)
    };
    let table =
        team_table(&[staff_row("zeus", &["worker"], WorkTier::Standard), designer]).unwrap();
    assert!(
        table.ends_with("| codex-default (Opus · high) | image designer | none |"),
        "{table}"
    );
    let advisor = StaffRow {
        work_tier: None,
        ..staff_row("diana", &["advisor"], WorkTier::Light)
    };
    assert!(matches!(
        team_table(&[advisor]),
        Err(RoleError::NoWorkTier { .. })
    ));
}

/// The project is read as the chief's text needs it: an image designer's tier is none.
#[test]
fn an_image_designer_is_read_off_the_project_with_no_tier() {
    use cf_engine::roles::staff_of;
    use serde_json::json;

    use super::support::{participant_of, project_of};

    let row = |id: i64, handle: &str, role: &str, designer: bool| {
        json!({
            "id": id, "projectId": 1, "handle": handle, "role": role, "roles": [role],
            "harness": "codex", "model": null, "effort": null, "modelName": null,
            "designer": designer, "createdAt": "2026-11-08T10:00:00.000Z", "tier": "light",
        })
    };
    let project = project_of(
        [
            row(3, "codex-default", "designer", true),
            row(4, "zeus", "worker", false),
        ]
        .iter()
        .map(participant_of)
        .collect(),
    );
    let staff = staff_of(&project);
    assert_eq!(
        staff
            .iter()
            .map(|member| member.work_tier)
            .collect::<Vec<_>>(),
        [None, Some(WorkTier::Light)]
    );
}

/// The table says each role by its name (spec new-roles, FD-04): the frontend designer and the
/// image designer are never one word, and a member that holds only roles with no tier says none.
#[test]
fn says_each_role_by_its_name_and_none_for_a_member_of_roles_with_no_tier() {
    let untiered = |handle: &str, roles: &[&str]| StaffRow {
        work_tier: None,
        ..staff_row(handle, roles, WorkTier::Light)
    };
    let table = team_table(&[
        untiered("iris", &["frontend"]),
        untiered("daedalus", &["architect", "researcher"]),
        staff_row("zeus", &["worker", "integrator"], WorkTier::Standard),
        untiered("codex-default", &["designer"]),
    ])
    .unwrap();
    assert_eq!(
        table.lines().skip(2).collect::<Vec<_>>(),
        [
            "| iris (Opus · high) | frontend designer | none |",
            "| daedalus (Opus · high) | architect, researcher | none |",
            "| zeus (Opus · high) | worker, integrator | Standard work |",
            "| codex-default (Opus · high) | image designer | none |",
        ]
    );
}
