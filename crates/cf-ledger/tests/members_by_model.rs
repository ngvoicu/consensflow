//! A member is a harness, a model it lists, an effort and a tier (spec `harness-models`):
//! no saved agent behind it and no catalog's tier for it. It is named by its model as its
//! harness's list names it, with its effort (`@opus-high`); where another participant of the
//! project has that name its harness goes in front (`@devin-claude-opus-5-5-high`), and
//! where one has that too a number after. Its windows are named after it and run what it
//! runs; Edit member moves it to another model, effort or tier and keeps its handle. A
//! member's tier is the human's, and one is required.

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, MemberEdit, NewChief, NewMember, NewProject, NewTask, Options,
    ParticipantView, ProjectView,
};
use serde_json::json;

/// Session names in a fixed order.
fn options() -> Options {
    let mut names = ["amber-pine", "brisk-birch"].into_iter().cycle();
    Options {
        names: Box::new(move || names.next().unwrap_or_default().to_owned()),
        ..Options::default()
    }
}

fn on(harness: &str, model: &str, effort: Option<&str>, name: &str, tier: &str) -> NewMember {
    NewMember {
        harness: harness.into(),
        model: Some(model.into()),
        effort: effort.map(str::to_owned),
        model_name: Some(name.into()),
        designer: false,
        roles: vec!["worker".into()],
        tier: tier.into(),
    }
}

fn project(ledger: &mut Ledger, staff: Vec<NewMember>) -> ProjectView {
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("claude-opus-5-5".into()),
                effort: None,
            },
            staff,
            gate: false,
            wake_up: None,
            specs: None,
        })
        .expect("a project")
}

fn ledger_in(dir: &tempfile::TempDir) -> Ledger {
    open_ledger(&dir.path().join("consensflow.db"), options()).expect("a ledger")
}

fn handles(project: &ProjectView) -> Vec<&str> {
    project
        .participants
        .iter()
        .map(|participant| participant.handle.as_str())
        .collect()
}

fn named(ledger: &Ledger, project: i64, handle: &str) -> ParticipantView {
    ledger
        .project(project)
        .expect("the project read")
        .expect("the project")
        .participants
        .into_iter()
        .find(|participant| participant.handle == handle)
        .expect("the participant")
}

#[test]
fn a_member_is_named_by_its_model_and_effort_its_harness_in_front_where_that_is_taken() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(
        &mut ledger,
        vec![
            on(
                "claude-code",
                "claude-opus-5-5",
                Some("high"),
                "Opus",
                "complex",
            ),
            on(
                "codex",
                "gpt-6-astra",
                Some("max"),
                "GPT-6-Astra",
                "critical",
            ),
            on("devin", "swe-2-max", Some("max"), "SWE-2", "light"),
            on("devin", "adaptive", None, "Adaptive", "standard"),
            on(
                "pi",
                "anthropic/claude-opus-5-5",
                Some("high"),
                "Claude Opus 5.5",
                "complex",
            ),
            on(
                "devin",
                "claude-opus-5-5-high",
                Some("high"),
                "Claude Opus 5.5",
                "complex",
            ),
        ],
    );
    assert_eq!(
        handles(&project),
        [
            "human",
            "chief",
            "opus-high",
            "gpt-6-astra-max",
            "swe-2-max",
            "adaptive",
            "claude-opus-5-5-high",
            "devin-claude-opus-5-5-high",
        ]
    );
    let opus = &project.participants[2];
    assert_eq!(
        (
            opus.harness.as_deref(),
            opus.model.as_deref(),
            opus.effort.as_deref(),
            opus.model_name.as_deref(),
            opus.agent.as_deref(),
            opus.tier.as_deref(),
        ),
        (
            Some("claude-code"),
            Some("claude-opus-5-5"),
            Some("high"),
            Some("Opus"),
            None,
            Some("complex")
        ),
        "it holds what it runs on itself, and names no agent"
    );
    // A third of one name and harness takes a number.
    let third = ledger
        .add_member(
            project.id,
            &on(
                "devin",
                "claude-opus-5-5-high-fast",
                Some("high"),
                "Claude Opus 5.5",
                "light",
            ),
        )
        .expect("added");
    assert_eq!(third.handle, "devin-claude-opus-5-5-high-2");
}

#[test]
fn a_member_whose_harness_was_never_read_is_named_by_its_model_s_id_and_one_on_a_default_by_its_harness(
) {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let mut by_id = on("claude-code", "worker", None, "x", "standard");
    by_id.model_name = None;
    let mut designer = on("codex", "x", None, "x", "light");
    designer.model = None;
    designer.model_name = None;
    designer.designer = true;
    designer.roles = vec!["designer".into()];
    let project = project(&mut ledger, vec![by_id, designer]);
    assert_eq!(handles(&project)[2..], ["worker", "codex-default"]);
}

#[test]
fn a_member_s_window_runs_what_it_runs_under_its_handle_and_two_words() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(
        &mut ledger,
        vec![on(
            "codex",
            "gpt-6.1-sol",
            Some("xhigh"),
            "GPT-6.1-Sol",
            "standard",
        )],
    );
    let member = named(&ledger, project.id, "gpt-6-1-sol-xhigh");
    ledger
        .create_task(
            project.id,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Fix the build".into(),
                ..NewTask::default()
            },
        )
        .expect("a task for it");
    ledger
        .assign_task(project.id, 1, member.id)
        .expect("assigned");
    let window = named(&ledger, project.id, "gpt-6-1-sol-xhigh-amber-pine");
    assert_eq!(window.member_id, Some(member.id));
    assert_eq!(window.session.as_deref(), Some("amber-pine"));
    assert_eq!(
        (
            window.harness,
            window.model,
            window.effort,
            window.model_name,
            window.tier
        ),
        (
            member.harness,
            member.model,
            member.effort,
            member.model_name,
            member.tier
        )
    );
}

#[test]
fn edit_member_moves_a_member_and_its_windows_to_another_model_and_keeps_its_handle() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(
        &mut ledger,
        vec![on(
            "claude-code",
            "claude-opus-5-5",
            Some("high"),
            "Opus",
            "complex",
        )],
    );
    let edited = ledger
        .edit_member(
            project.id,
            "opus-high",
            &MemberEdit {
                harness: "codex".into(),
                model: Some("gpt-6.1-sol".into()),
                effort: Some("max".into()),
                model_name: Some("GPT-6.1-Sol".into()),
                tier: "critical".into(),
            },
        )
        .expect("edited");
    assert_eq!(
        (
            edited.handle.as_str(),
            edited.harness.as_deref(),
            edited.model.as_deref(),
            edited.effort.as_deref(),
            edited.model_name.as_deref(),
            edited.tier.as_deref()
        ),
        (
            "opus-high",
            Some("codex"),
            Some("gpt-6.1-sol"),
            Some("max"),
            Some("GPT-6.1-Sol"),
            Some("critical")
        )
    );
    let logged = ledger.events(project.id, 0, 500).expect("the log");
    let last = logged.last().expect("an event");
    assert_eq!(last.kind, "member.edited");
    assert_eq!(
        last.data,
        json!({
            "handle": "opus-high",
            "from": { "harness": "claude-code", "model": "claude-opus-5-5", "effort": "high", "tier": "complex" },
            "to": { "harness": "codex", "model": "gpt-6.1-sol", "effort": "max", "tier": "critical" },
        })
    );
}

#[test]
fn a_member_that_left_rejoins_under_its_handle_when_its_model_and_effort_come_back() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(
        &mut ledger,
        vec![on(
            "claude-code",
            "claude-opus-5-5",
            Some("high"),
            "Opus",
            "complex",
        )],
    );
    let first = named(&ledger, project.id, "opus-high").id;
    ledger
        .remove_member(project.id, "opus-high")
        .expect("removed");
    let back = ledger
        .add_member(
            project.id,
            &on(
                "claude-code",
                "claude-opus-5-5",
                Some("high"),
                "Opus",
                "light",
            ),
        )
        .expect("back");
    assert_eq!(
        (back.id, back.handle.as_str(), back.tier.as_deref()),
        (first, "opus-high", Some("light"))
    );
    assert!(back.left_at.is_none());
    // Another effort is another member.
    let other = ledger
        .add_member(
            project.id,
            &on(
                "claude-code",
                "claude-opus-5-5",
                Some("max"),
                "Opus",
                "critical",
            ),
        )
        .expect("added");
    assert_eq!(other.handle, "opus-max");
}

/// A member that left comes back as itself, by what it ran on, and under the handle it
/// had: never as another member whose name its own would be, and never under a new one.
#[test]
fn a_member_that_left_is_known_by_its_harness_model_and_effort_not_by_its_name() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let pi = on(
        "pi",
        "anthropic/claude-opus-5-5",
        Some("high"),
        "Claude Opus 5.5",
        "complex",
    );
    let devin = on(
        "devin",
        "claude-opus-5-5-high",
        Some("high"),
        "Claude Opus 5.5",
        "complex",
    );
    let project = project(&mut ledger, vec![pi.clone(), devin.clone()]);
    let named = |ledger: &Ledger, handle: &str| named(ledger, project.id, handle).id;
    let (pi_id, devin_id) = (
        named(&ledger, "claude-opus-5-5-high"),
        named(&ledger, "devin-claude-opus-5-5-high"),
    );
    // Pi's leaves; Devin's on the same model's name and effort is not it.
    ledger
        .remove_member(project.id, "claude-opus-5-5-high")
        .expect("removed");
    ledger
        .remove_member(project.id, "devin-claude-opus-5-5-high")
        .expect("removed");
    let devin_back = ledger.add_member(project.id, &devin).expect("back");
    assert_eq!(
        (devin_back.id, devin_back.handle.as_str()),
        (devin_id, "devin-claude-opus-5-5-high"),
        "Devin's comes back as itself, under its own handle"
    );
    let pi_back = ledger.add_member(project.id, &pi).expect("back");
    assert_eq!(
        (
            pi_back.id,
            pi_back.handle.as_str(),
            pi_back.harness.as_deref()
        ),
        (pi_id, "claude-opus-5-5-high", Some("pi")),
        "Pi's comes back as itself"
    );
    // A member on another model with a name a departed one had is a member of its own.
    ledger
        .remove_member(project.id, "claude-opus-5-5-high")
        .expect("removed");
    let other = ledger
        .add_member(
            project.id,
            &on(
                "claude-code",
                "claude-opus-5-5",
                Some("high"),
                "Claude Opus 5.5",
                "light",
            ),
        )
        .expect("added");
    assert_ne!(other.id, pi_id);
    assert_eq!(other.handle, "claude-code-claude-opus-5-5-high");
}

#[test]
fn a_member_needs_a_tier_and_a_word_for_an_effort_and_the_staff_a_project_starts_from_says_what_each_runs(
) {
    for (member, code) in [
        (
            json!({ "harness": "codex", "model": "gpt-6.1-sol", "roles": ["worker"] }),
            "invalid-tier",
        ),
        (
            json!({ "harness": "codex", "model": "gpt-6.1-sol", "effort": "very high", "roles": ["worker"], "tier": "light" }),
            "invalid-effort",
        ),
        (
            json!({ "harness": "codex", "model": "-gpt", "roles": ["worker"], "tier": "light" }),
            "invalid-model",
        ),
        (
            json!({ "harness": "codex", "model": "gpt-6.1-sol", "modelName": "", "roles": ["worker"], "tier": "light" }),
            "invalid-model-name",
        ),
    ] {
        let refused = NewMember::from_json(&member).expect_err("refused");
        assert_eq!(refused.code(), Some(code), "{member}");
    }
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let member = NewMember::from_json(&json!({
        "harness": "codex", "model": "gpt-6.1-sol", "effort": "high", "modelName": "GPT-6.1-Sol",
        "roles": ["worker", "reviewer"], "tier": "standard",
    }))
    .expect("a member");
    project(&mut ledger, vec![member]);
    let staff = serde_json::to_value(ledger.last_staff().expect("the last staff")).expect("json");
    assert_eq!(
        staff,
        json!([{
            "harness": "codex", "model": "gpt-6.1-sol", "effort": "high", "modelName": "GPT-6.1-Sol",
            "designer": false, "tier": "standard", "role": "worker", "roles": ["worker", "reviewer"],
        }])
    );
}

/// A member made in code, not read from a request, is held to the same rule: no project
/// starts with a member that has no tier, and no Add saves one.
#[test]
fn a_member_with_no_tier_is_refused_at_a_project_s_start_and_at_an_add() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let untiered = on("codex", "gpt-6.1-sol", Some("high"), "GPT-6.1-Sol", "");
    let refused = ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("claude-opus-5-5".into()),
                effort: None,
            },
            staff: vec![untiered.clone()],
            gate: false,
            wake_up: None,
            specs: None,
        })
        .expect_err("refused");
    assert_eq!(refused.code(), Some("invalid-tier"));
    let started = project(&mut ledger, Vec::new());
    let refused = ledger
        .add_member(started.id, &untiered)
        .expect_err("refused");
    assert_eq!(refused.code(), Some("invalid-tier"));
    assert_eq!(
        handles(
            &ledger
                .project(started.id)
                .expect("read")
                .expect("the project")
        ),
        ["human", "chief"]
    );
}
