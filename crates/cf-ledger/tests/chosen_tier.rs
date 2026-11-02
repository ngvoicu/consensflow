//! A member's tier the human chose (T-1143): Add member and Edit member take a tier the human
//! picks, for a model its harness lists that the catalog's tier rules may not know. The
//! start's following of the agents' tiers (`refresh_member_tiers`) moves every other
//! member to its agent's tier, and passes a chosen one by: only the human changes it.

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, MemberEdit, NewChief, NewMember, NewProject, Options, ProjectView,
};

fn member(agent: &str, tier: &str) -> NewMember {
    NewMember {
        agent: agent.into(),
        harness: "claude-code".into(),
        designer: false,
        roles: vec!["worker".into()],
        tier: tier.into(),
    }
}

/// A project at /work/app with no staff.
fn project(ledger: &mut Ledger) -> ProjectView {
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("claude-opus-5-5".into()),
            },
            staff: Vec::new(),
            gate: false,
            wake_up: None,
            specs: None,
        })
        .expect("a project")
}

fn ledger_in(dir: &tempfile::TempDir) -> Ledger {
    open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger")
}

fn tier_of(ledger: &Ledger, project: i64, handle: &str) -> Option<String> {
    ledger
        .project(project)
        .expect("the project read")
        .expect("the project")
        .participants
        .into_iter()
        .find(|participant| participant.handle == handle)
        .expect("the member")
        .tier
}

#[test]
fn a_tier_the_human_chose_survives_the_agents_tiers_being_followed_and_one_that_follows_moves() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger).id;
    ledger
        .add_member_in_chosen_tier(project, &member("claude-mythos-high", "critical"))
        .expect("a member in the tier the human chose");
    ledger
        .add_member(project, &member("zeus", "standard"))
        .expect("a member whose tier follows its agent's");
    // The catalog's rules say light of both agents: `mythos` is a model they do not know.
    let changed = ledger
        .refresh_member_tiers(|_| Some("light".to_owned()))
        .expect("the tiers followed");
    let moved: Vec<&str> = changed
        .iter()
        .map(|change| change.handle.as_str())
        .collect();
    assert_eq!(
        moved,
        ["zeus"],
        "only the member whose tier follows its agent's"
    );
    assert_eq!(
        tier_of(&ledger, project, "claude-mythos-high").as_deref(),
        Some("critical")
    );
    assert_eq!(tier_of(&ledger, project, "zeus").as_deref(), Some("light"));
}

#[test]
fn an_edited_member_runs_on_its_new_agent_in_the_tier_chosen_and_keeps_it_from_then_on() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger).id;
    ledger
        .add_member(project, &member("zeus", "standard"))
        .expect("a member whose tier follows its agent's");
    let edited = ledger
        .edit_member(
            project,
            "zeus",
            &MemberEdit {
                agent: "codex-gpt-6-1-sol-max".into(),
                harness: "codex".into(),
                designer: false,
                tier: "complex".into(),
            },
        )
        .expect("edited");
    assert_eq!(edited.handle, "zeus", "its handle stays");
    assert_eq!(edited.agent.as_deref(), Some("codex-gpt-6-1-sol-max"));
    assert_eq!(edited.harness.as_deref(), Some("codex"));
    assert_eq!(edited.tier.as_deref(), Some("complex"));
    assert_eq!(edited.roles, ["worker"], "its roles stay");
    let changed = ledger
        .refresh_member_tiers(|_| Some("light".to_owned()))
        .expect("the tiers followed");
    assert!(changed.is_empty(), "{changed:?}");
    assert_eq!(
        tier_of(&ledger, project, "zeus").as_deref(),
        Some("complex")
    );
}

#[test]
fn an_edit_is_refused_for_the_chief_and_for_a_tier_or_an_agent_the_ledger_does_not_take() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger).id;
    ledger
        .add_member(project, &member("zeus", "standard"))
        .expect("a member");
    let edit = |tier: &str, agent: &str| MemberEdit {
        agent: agent.into(),
        harness: "claude-code".into(),
        designer: false,
        tier: tier.into(),
    };
    let chief = ledger
        .edit_member(project, "chief", &edit("complex", "zeus"))
        .expect_err("the chief is no member");
    assert!(
        chief.to_string().contains("not a member of its staff"),
        "{chief}"
    );
    let tier = ledger
        .edit_member(project, "zeus", &edit("huge", "zeus"))
        .expect_err("no such tier");
    assert!(tier.to_string().contains("huge"), "{tier}");
    let agent = ledger
        .edit_member(project, "zeus", &edit("complex", "Not An Agent"))
        .expect_err("no agent id");
    assert!(agent.to_string().contains("Not An Agent"), "{agent}");
    assert_eq!(
        tier_of(&ledger, project, "zeus").as_deref(),
        Some("standard")
    );
}
