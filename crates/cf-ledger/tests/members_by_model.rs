//! A member is a harness, a model it lists, an effort and a tier (spec `harness-models`):
//! no saved agent behind it and no catalog's tier for it. It is named by its model as its
//! harness's list names it, its id, with its effort (`@claude-opus-5-5-high`); where another
//! participant of the project has that name a Pi model's provider (`@anthropic-claude-opus-5-5-high`)
//! or its harness goes in front (`@devin-claude-opus-5-5-high`), and where one has that too
//! a number after. Its windows are named after it and run what it runs; Edit member moves
//! it to another model, effort or tier, and renames it and its windows by the model it now
//! runs (HMD-12). A member's tier is the human's, and one is required.

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
                "claude-opus-5-5",
                "complex",
            ),
            on(
                "codex",
                "gpt-6-astra",
                Some("max"),
                "gpt-6-astra",
                "critical",
            ),
            on("devin", "swe-2-max", Some("max"), "swe-2", "light"),
            on("devin", "adaptive", None, "adaptive", "standard"),
            on(
                "pi",
                "anthropic/claude-opus-5-5",
                Some("high"),
                "anthropic/claude-opus-5-5",
                "complex",
            ),
            on(
                "devin",
                "claude-opus-5-5-high",
                Some("high"),
                "claude-opus-5-5",
                "complex",
            ),
        ],
    );
    assert_eq!(
        handles(&project),
        [
            "human",
            "chief",
            "claude-opus-5-5-high",
            "gpt-6-astra-max",
            "swe-2-max",
            "adaptive",
            // Pi's name leaves its provider out, and takes it in front only where the
            // name is taken.
            "anthropic-claude-opus-5-5-high",
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
            Some("claude-opus-5-5"),
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
                "claude-opus-5-5",
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
            "gpt-6.1-sol",
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
fn edit_member_moves_a_member_and_its_windows_to_another_model_and_names_them_by_it() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(
        &mut ledger,
        vec![on(
            "claude-code",
            "claude-opus-5-5",
            Some("high"),
            "claude-opus-5-5",
            "complex",
        )],
    );
    let edited = ledger
        .edit_member(
            project.id,
            "claude-opus-5-5-high",
            &MemberEdit {
                harness: "codex".into(),
                model: Some("gpt-6.1-sol".into()),
                effort: Some("max".into()),
                model_name: Some("gpt-6.1-sol".into()),
                tier: "critical".into(),
            },
        )
        .expect("edited");
    assert_eq!(
        (
            edited.handle.as_str(),
            edited.former_handles.as_slice(),
            edited.harness.as_deref(),
            edited.model.as_deref(),
            edited.effort.as_deref(),
            edited.model_name.as_deref(),
            edited.tier.as_deref()
        ),
        (
            "gpt-6-1-sol-max",
            ["claude-opus-5-5-high".to_owned()].as_slice(),
            Some("codex"),
            Some("gpt-6.1-sol"),
            Some("max"),
            Some("gpt-6.1-sol"),
            Some("critical")
        )
    );
    let logged = ledger.events(project.id, 0, 500).expect("the log");
    let kinds: Vec<&str> = logged
        .iter()
        .rev()
        .take(2)
        .map(|e| e.kind.as_str())
        .collect();
    assert_eq!(kinds, ["member.renamed", "member.edited"]);
    assert_eq!(
        logged[logged.len() - 2].data,
        json!({
            "handle": "claude-opus-5-5-high",
            "from": { "harness": "claude-code", "model": "claude-opus-5-5", "effort": "high", "tier": "complex" },
            "to": { "harness": "codex", "model": "gpt-6.1-sol", "effort": "max", "tier": "critical" },
        })
    );
    assert_eq!(
        logged[logged.len() - 1].data,
        json!({ "from": "claude-opus-5-5-high", "to": "gpt-6-1-sol-max", "windows": 0 })
    );
}

/// The human's rule: the name holds the model. A member edited to another model or another
/// effort is renamed, its windows with it, one with a task in hand included (the engine's
/// suite holds the window at work): a window is its token's participant by id, so it goes
/// on, and its task stays its own; the names it left are kept, oldest first. A member whose name stays right (its tier edited) keeps it, and one whose new
/// name is taken takes the harness in front.
#[test]
fn edit_member_renames_a_member_and_all_its_windows_one_with_a_task_in_hand_too() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(
        &mut ledger,
        vec![
            on(
                "codex",
                "gpt-6.1-sol",
                Some("high"),
                "gpt-6.1-sol",
                "standard",
            ),
            on(
                "codex",
                "gpt-6.1-sol",
                Some("max"),
                "gpt-6.1-sol",
                "standard",
            ),
        ],
    );
    let member = named(&ledger, project.id, "gpt-6-1-sol-high");
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
    let at_work = named(&ledger, project.id, "gpt-6-1-sol-high-amber-pine");
    assert_eq!(at_work.member_id, Some(member.id));
    let edit = |effort: &str, tier: &str| MemberEdit {
        harness: "codex".into(),
        model: Some("gpt-6.1-sol".into()),
        effort: Some(effort.into()),
        model_name: Some("gpt-6.1-sol".into()),
        tier: tier.into(),
    };
    // Only its tier: the name is right.
    let same = ledger
        .edit_member(project.id, "gpt-6-1-sol-high", &edit("high", "light"))
        .expect("edited");
    assert_eq!(
        (same.handle.as_str(), same.former_handles.len()),
        ("gpt-6-1-sol-high", 0)
    );
    // Its effort: the member and its window at work take the new name.
    let edited = ledger
        .edit_member(project.id, "gpt-6-1-sol-high", &edit("xhigh", "light"))
        .expect("edited");
    assert_eq!(edited.handle, "gpt-6-1-sol-xhigh");
    assert_eq!(edited.former_handles, ["gpt-6-1-sol-high"]);
    let view = ledger
        .project(project.id)
        .expect("read")
        .expect("the project");
    let window = view
        .participants
        .iter()
        .find(|participant| participant.id == at_work.id)
        .expect("the window");
    assert_eq!(
        (
            window.handle.as_str(),
            window.member.as_deref(),
            window.session.as_deref(),
            window.former_handles.as_slice(),
            window.effort.as_deref()
        ),
        (
            "gpt-6-1-sol-xhigh-amber-pine",
            Some("gpt-6-1-sol-xhigh"),
            Some("amber-pine"),
            ["gpt-6-1-sol-high-amber-pine".to_owned()].as_slice(),
            Some("xhigh")
        ),
        "a window with a task in hand follows at once, and says its effort"
    );
    // Its task is still its own: the author is the same participant.
    let task = ledger.task(project.id, 1).expect("read").expect("the task");
    assert_eq!(
        (task.task.assignee.as_deref(), task.task.state.as_str()),
        (Some("gpt-6-1-sol-xhigh-amber-pine"), "queued")
    );
    // Edited again to what another member's name already is: its harness goes in front, and
    // both of its earlier names are kept.
    let again = ledger
        .edit_member(project.id, "gpt-6-1-sol-xhigh", &edit("max", "light"))
        .expect("edited");
    assert_eq!(again.handle, "codex-gpt-6-1-sol-max");
    assert_eq!(
        again.former_handles,
        ["gpt-6-1-sol-high", "gpt-6-1-sol-xhigh"]
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
            "claude-opus-5-5",
            "complex",
        )],
    );
    let first = named(&ledger, project.id, "claude-opus-5-5-high").id;
    ledger
        .remove_member(project.id, "claude-opus-5-5-high")
        .expect("removed");
    let back = ledger
        .add_member(
            project.id,
            &on(
                "claude-code",
                "claude-opus-5-5",
                Some("high"),
                "claude-opus-5-5",
                "light",
            ),
        )
        .expect("back");
    assert_eq!(
        (back.id, back.handle.as_str(), back.tier.as_deref()),
        (first, "claude-opus-5-5-high", Some("light"))
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
                "claude-opus-5-5",
                "critical",
            ),
        )
        .expect("added");
    assert_eq!(other.handle, "claude-opus-5-5-max");
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
        "anthropic/claude-opus-5-5",
        "complex",
    );
    let devin = on(
        "devin",
        "claude-opus-5-5-high",
        Some("high"),
        "claude-opus-5-5",
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
                "claude-opus-5-5",
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
        "harness": "codex", "model": "gpt-6.1-sol", "effort": "high", "modelName": "gpt-6.1-sol",
        "roles": ["worker", "reviewer"], "tier": "standard",
    }))
    .expect("a member");
    project(&mut ledger, vec![member]);
    let staff = serde_json::to_value(ledger.last_staff().expect("the last staff")).expect("json");
    assert_eq!(
        staff,
        json!([{
            "harness": "codex", "model": "gpt-6.1-sol", "effort": "high", "modelName": "gpt-6.1-sol",
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
    let untiered = on("codex", "gpt-6.1-sol", Some("high"), "gpt-6.1-sol", "");
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

/// A member that needs an edit (the update could not tell what it ran on, T-1202) is
/// edited to a model: it runs on it, its windows with it, and it no longer needs one.
#[test]
fn edit_member_takes_the_mark_off_a_member_that_needs_an_edit() {
    let dir = tempfile::tempdir().expect("a home");
    let file = dir.path().join("consensflow.db");
    let mut ledger = open_ledger(&file, options()).expect("a ledger");
    let project = project(
        &mut ledger,
        vec![on(
            "codex",
            "gpt-6.1-sol",
            Some("high"),
            "gpt-6.1-sol",
            "standard",
        )],
    );
    drop(ledger);
    let db = rusqlite::Connection::open(&file).expect("the file");
    db.execute_batch("UPDATE participant SET model = NULL, effort = NULL, model_name = NULL, needs_edit = 1 WHERE handle = 'gpt-6-1-sol-high'")
        .expect("marked");
    drop(db);
    let mut ledger = open_ledger(&file, options()).expect("the ledger again");
    assert!(named(&ledger, project.id, "gpt-6-1-sol-high").needs_edit);
    let edited = ledger
        .edit_member(
            project.id,
            "gpt-6-1-sol-high",
            &MemberEdit {
                harness: "codex".into(),
                model: Some("gpt-6.1-sol".into()),
                effort: Some("ultra".into()),
                model_name: Some("gpt-6.1-sol".into()),
                tier: "standard".into(),
            },
        )
        .expect("edited");
    assert!(!edited.needs_edit);
    assert_eq!(edited.effort.as_deref(), Some("ultra"));
}
