//! A quota the person renewed by hand (T-1357, Gabriel, 2026-11-09: he reset his Codex quota to
//! let a waiting review start, and the board kept it waiting). The board learns of a quota from
//! a record of one of the member's windows; a member out until its reset, with no window
//! running, stays out until that time passes unless the human says it is back (Try again now,
//! `back_from_quota`). The human's word is for the account they renewed: every member on the
//! machine's one login for the harness is back, in every project, and a wait says what to
//! press. Held to no Node trace: Node's dispatcher had neither.

use cf_engine::testing::{Context, Made};
use cf_ledger::{NewMember, NewTask};
use serde_json::{json, Value};

use crate::fixtures::{assert_match, exhausted, now, participant, placed, soon, Tiers};

/// The staff of a project of `directory`, each member a worker of the standard tier on the
/// harness its pair names, and the chief on Claude Code.
fn project_of(context: &Context, directory: &str, members: &[(&str, &str)]) -> i64 {
    let staff: Vec<Value> = members
        .iter()
        .map(|(agent, harness)| {
            json!({ "agent": agent, "harness": harness, "role": "worker", "tier": "standard" })
        })
        .collect();
    context
        .open_project(json!({
            "directory": directory,
            "name": directory.trim_start_matches("/work/"),
            "chief": { "harness": "claude-code", "model": "claude-opus-5" },
            "staff": staff,
        }))
        .unwrap()
        .unwrap()
        .id
}

/// Marks `handle` of `project` out of quota until `until`, as a refusal of its harness does.
fn mark_out(context: &Context, project: i64, handle: &str, until: &str) {
    let id = context.id(project, handle);
    context
        .ledger
        .borrow_mut()
        .mark_out(id, until, "out of quota")
        .unwrap();
}

/// Until when `handle` of `project` is out, none when it is not.
fn out_until(context: &Context, project: i64, handle: &str) -> Option<String> {
    participant(context, project, handle)
        .unwrap_or_else(|| panic!("no @{handle}"))
        .out_until
}

#[test]
fn a_wait_for_a_member_out_of_quota_says_to_press_try_again_now_if_it_was_renewed_early() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    let until = soon(&context, 3);
    mark_out(&context, tiers.project.id, "zeus", &until);
    mark_out(&context, tiers.project.id, "diana", &until);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "open");
    assert_eq!(
        tiers.notes("chief"),
        [format!(
            "T-1 waits for a free standard worker: @zeus is out of quota until {until}; @diana is out of quota until {until}. If the quota was renewed early, the human presses Try again now on @zeus or @diana."
        )]
    );
}

#[test]
fn a_wait_for_a_member_held_for_its_account_or_its_model_names_no_renewal_of_a_quota() {
    // A renewed quota ends neither: an account that refused for its billing or its login, and a
    // model its harness moved, wait for what the human does about them, and say so elsewhere.
    for held in ["9999-12-31T23:59:59.998Z", "9999-12-31T23:59:59.999Z"] {
        let context = Context::new();
        let tiers = Tiers::new(&context, &["zeus"]);
        mark_out(&context, tiers.project.id, "zeus", held);
        tiers.open();
        context.pass().unwrap();
        context.pass().unwrap();
        let notes = tiers.notes("chief");
        assert_eq!(notes.len(), 1, "{held}: {notes:?}");
        assert_match(&notes[0], "^T-1 waits for a free standard worker: @zeus's ");
        assert!(!notes[0].contains("renewed"), "{held}: {}", notes[0]);
    }
}

#[test]
fn a_member_the_human_says_is_back_is_given_the_next_task_at_once_and_a_refusal_after_marks_it_out_again(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let until = soon(&context, 3);
    mark_out(&context, tiers.project.id, "zeus", &until);
    tiers.open();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "open", "out until {until}");

    // The human renewed the quota by hand and presses Try again now: no window of zeus was
    // running, and nothing but their word could tell.
    context.back_from_quota(tiers.project.id, "zeus").unwrap();
    assert_eq!(out_until(&context, tiers.project.id, "zeus"), None);
    context.pass().unwrap();
    let session = tiers.assignee(1);
    assert_match(&session, "^zeus-");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "given at once");

    // The quota was not renewed after all: its harness refuses, a minute on, and zeus is out
    // again. (A refusal dated before the human's word is history.)
    context.advance(60_000);
    let resets_at = soon(&context, 2);
    context
        .adapter
        .refuse(&session, exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("paused", Some(session.as_str())),
        "the work waits with its window"
    );
    assert_eq!(
        out_until(&context, tiers.project.id, "zeus"),
        Some(resets_at)
    );
}

#[test]
fn the_humans_word_for_one_codex_member_brings_back_every_member_on_its_login_in_every_project() {
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    // Codex keeps the machine's one login; Pi logs into a provider for each model.
    context.codex.one_login.set(true);
    let first = project_of(
        &context,
        "/work/first",
        &[("zeus", "codex"), ("diana", "codex"), ("hera", "pi")],
    );
    let second = project_of(&context, "/work/second", &[("apollo", "codex")]);
    let until = soon(&context, 3);
    for (project, handle) in [
        (first, "zeus"),
        (first, "diana"),
        (first, "hera"),
        (first, "chief"),
        (second, "apollo"),
    ] {
        mark_out(&context, project, handle, &until);
    }

    context.back_from_quota(first, "zeus").unwrap();

    assert_eq!(out_until(&context, first, "zeus"), None);
    assert_eq!(
        out_until(&context, first, "diana"),
        None,
        "diana is on the same Codex login"
    );
    assert_eq!(
        out_until(&context, second, "apollo"),
        None,
        "and so is apollo, in another project"
    );
    assert_eq!(
        out_until(&context, first, "hera"),
        Some(until.clone()),
        "Pi's members may be on other accounts"
    );
    assert_eq!(
        out_until(&context, first, "chief"),
        Some(until),
        "the chief is on Claude Code, another login"
    );
}

#[test]
fn the_humans_word_for_one_pi_member_is_for_that_member_alone() {
    // Pi logs into a provider for each model: two of its members may be on two accounts.
    let context = Context::new();
    let project = project_of(&context, "/work/app", &[("hera", "pi"), ("apollo", "pi")]);
    let until = soon(&context, 3);
    mark_out(&context, project, "hera", &until);
    mark_out(&context, project, "apollo", &until);

    context.back_from_quota(project, "hera").unwrap();

    assert_eq!(out_until(&context, project, "hera"), None);
    assert_eq!(out_until(&context, project, "apollo"), Some(until));
}

#[test]
fn a_wait_names_for_a_renewal_only_the_members_out_now() {
    // hera is on a harness ConsensFlow cannot open and its old mark is past; zeus is out.
    let context = Context::made(Made {
        harnesses: vec!["claude-code"],
        ..Made::default()
    });
    let project = project_of(&context, "/work/app", &[("zeus", "claude-code")]);
    context
        .ledger
        .borrow_mut()
        .add_member(
            project,
            &NewMember {
                model: Some("muse-spark".to_owned()),
                effort: None,
                model_name: Some("hera".to_owned()),
                harness: "pi".to_owned(),
                designer: false,
                roles: vec!["worker".to_owned()],
                tier: "standard".to_owned(),
            },
        )
        .unwrap();
    mark_out(&context, project, "hera", &soon(&context, 1));
    context.advance(2 * 3_600_000);
    let until = soon(&context, 3);
    mark_out(&context, project, "zeus", &until);
    context.create_task(
        project,
        NewTask {
            from: "chief".to_owned(),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            body: "Write the parser".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    let chief = context.id(project, "chief");
    let notes = crate::fixtures::notes(&context, chief);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].ends_with(
            " If the quota was renewed early, the human presses Try again now on @zeus."
        ),
        "{notes:?}"
    );
}

#[test]
fn the_humans_word_gives_every_member_it_brought_back_the_work_that_waited_for_it() {
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    context.codex.one_login.set(true);
    let project = project_of(
        &context,
        "/work/app",
        &[("zeus", "codex"), ("diana", "codex")],
    );
    let until = soon(&context, 3);
    mark_out(&context, project, "zeus", &until);
    mark_out(&context, project, "diana", &until);
    for body in ["Write the lexer", "Write the parser"] {
        context.create_task(
            project,
            NewTask {
                from: "chief".to_owned(),
                pool: Some("worker".to_owned()),
                tier: Some("standard".to_owned()),
                body: body.to_owned(),
                ..NewTask::default()
            },
        );
    }
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "open");
    assert_eq!(context.task(project, 2).task.state, "open");

    context.back_from_quota(project, "zeus").unwrap();
    context.pass().unwrap();
    context.pass().unwrap();

    let given = |number: i64| context.task(project, number).task.assignee;
    let (first, second) = (given(1), given(2));
    let by = |prefix: &str| {
        [&first, &second]
            .into_iter()
            .flatten()
            .filter(|assignee| assignee.starts_with(prefix))
            .count()
    };
    assert_eq!(
        (by("zeus-"), by("diana-")),
        (1, 1),
        "one task each, diana's quota being back as zeus's: {first:?} {second:?}"
    );
}
