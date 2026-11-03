//! The tier of a task no window has yet, changed with `cf task edit T-3 --tier`
//! (T-1108): the same states the brief may be changed in (open, queued, paused), read
//! as `cf task add` reads a tier (a tier nobody holds goes to the nearest one somebody
//! does, a role nobody holds is refused), and in one step with the brief or none.

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewProject, NewTask, Options, ProjectView, TaskEdit,
    TaskEdited, TaskView,
};

fn member(agent: &str, roles: &[&str], tier: &str) -> NewMember {
    NewMember {
        agent: agent.into(),
        harness: "claude-code".into(),
        designer: false,
        roles: roles.iter().map(|role| (*role).to_owned()).collect(),
        tier: tier.into(),
    }
}

/// A project at /work/app with apollo for its chief and `staff`.
fn project(ledger: &mut Ledger, staff: Vec<NewMember>) -> ProjectView {
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("apollo".into()),
            },
            staff,
            gate: false,
            wake_up: None,
            specs: None,
        })
        .expect("a project")
}

fn id_of(project: &ProjectView, handle: &str) -> i64 {
    project
        .participants
        .iter()
        .find(|participant| participant.handle == handle)
        .expect("a participant")
        .id
}

/// The chief opens a task for `tier` of `pool`.
fn open(ledger: &mut Ledger, project: &ProjectView, pool: &str, tier: &str) -> TaskView {
    ledger
        .create_task(
            project.id,
            &NewTask {
                from: "chief".into(),
                pool: Some(pool.into()),
                tier: Some(tier.into()),
                body: "Do the work".into(),
                ..NewTask::default()
            },
        )
        .expect("a task")
        .task
}

fn tier_of(ledger: &Ledger, project: &ProjectView, number: i64) -> Option<String> {
    ledger
        .task(project.id, number)
        .expect("a read")
        .expect("the task")
        .task
        .tier
}

fn to(tier: &str) -> TaskEdit {
    TaskEdit {
        tier: Some(tier.into()),
        ..TaskEdit::default()
    }
}

fn edit(
    ledger: &mut Ledger,
    project: &ProjectView,
    number: i64,
    edit: &TaskEdit,
) -> Result<TaskEdited, cf_ledger::LedgerError> {
    ledger.edit_task_with(project.id, number, "chief", edit)
}

/// What the chief was written that says T-`number` was taken back.
fn notes_to_the_chief(ledger: &Ledger, project: &ProjectView) -> Vec<String> {
    let mut notes: Vec<String> = ledger
        .inbox(id_of(project, "chief"), 100)
        .expect("an inbox")
        .into_iter()
        .filter(|message| message.kind == "note")
        .map(|message| message.body)
        .collect();
    notes.reverse();
    notes
}

fn ledger_in(dir: &tempfile::TempDir) -> Ledger {
    open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger")
}

#[test]
fn an_open_task_asks_the_new_tier_and_the_change_is_in_the_log() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(
        &mut ledger,
        vec![
            member("zeus", &["worker"], "standard"),
            member("hera", &["worker"], "light"),
        ],
    );
    let task = open(&mut ledger, &project, "worker", "light").number;

    let edited = edit(&mut ledger, &project, task, &to("standard")).expect("edited");
    assert_eq!(
        (edited.task.tier.as_deref(), edited.task.state.as_str()),
        (Some("standard"), "open")
    );
    assert_eq!((edited.asked, edited.holders), (None, Vec::<String>::new()));
    let moved: Vec<_> = ledger
        .events(project.id, 0, 500)
        .expect("the events")
        .into_iter()
        .filter(|event| event.kind == "task.retiered")
        .map(|event| {
            (
                event.data["from"].as_str().map(str::to_owned),
                event.data["to"].as_str().map(str::to_owned),
                event.data["by"].as_str().map(str::to_owned),
            )
        })
        .collect();
    assert_eq!(
        moved,
        [(
            Some("light".to_owned()),
            Some("standard".to_owned()),
            Some("chief".to_owned())
        )]
    );
}

#[test]
fn a_tier_nobody_holds_goes_to_the_nearest_and_says_which_was_asked_and_whose_it_went_to() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(
        &mut ledger,
        vec![
            member("zeus", &["worker"], "standard"),
            member("hera", &["worker"], "light"),
        ],
    );
    let task = open(&mut ledger, &project, "worker", "light").number;
    let edited = edit(&mut ledger, &project, task, &to("complex")).expect("edited");
    assert_eq!(edited.task.tier.as_deref(), Some("standard"));
    assert_eq!(edited.asked.as_deref(), Some("complex"));
    assert_eq!(edited.holders, ["zeus"]);
}

#[test]
fn a_role_nobody_holds_is_refused_in_the_words_of_task_add_and_nothing_changes() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(
        &mut ledger,
        vec![
            member("zeus", &["worker"], "standard"),
            member("tess", &["tester"], "standard"),
        ],
    );
    let task = open(&mut ledger, &project, "tester", "standard").number;
    ledger.remove_member(project.id, "tess").expect("it leaves");
    let both = TaskEdit {
        body: Some("A new brief".into()),
        tier: Some("light".into()),
        ..TaskEdit::default()
    };
    let refused = edit(&mut ledger, &project, task, &both).expect_err("nobody tests");
    assert_eq!(
        (refused.code(), refused.to_string()),
        (
            Some("no-member-of-tier"),
            "no tester is on the staff: ask the human for one, in your terminal".to_owned()
        )
    );
    let read = ledger
        .task(project.id, task)
        .expect("a read")
        .expect("it")
        .task;
    assert_eq!(
        (read.tier.as_deref(), read.body.as_str()),
        (Some("standard"), "Do the work"),
        "all or none: the brief is the old one too"
    );
}

#[test]
fn a_tier_that_is_none_is_refused_and_critical_work_needs_its_purpose() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(
        &mut ledger,
        vec![
            member("zeus", &["worker"], "standard"),
            member("ares", &["worker"], "critical"),
        ],
    );
    let task = open(&mut ledger, &project, "worker", "standard").number;
    let refused = edit(&mut ledger, &project, task, &to("huge")).expect_err("no such tier");
    assert_eq!(refused.code(), Some("invalid-tier"));
    let refused = edit(&mut ledger, &project, task, &to("critical")).expect_err("no purpose");
    assert_eq!(refused.code(), Some("purpose-required"));
    assert_eq!(
        tier_of(&ledger, &project, task).as_deref(),
        Some("standard")
    );
    let nothing = TaskEdit::default();
    let refused = edit(&mut ledger, &project, task, &nothing).expect_err("nothing to change");
    assert_eq!(refused.code(), Some("invalid-text"));
}

#[test]
fn a_task_given_by_name_or_for_the_designer_has_no_tier_to_change() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let image = NewMember {
        designer: true,
        ..member("pygmalion", &["designer"], "standard")
    };
    let project = project(
        &mut ledger,
        vec![member("zeus", &["worker"], "standard"), image],
    );
    let drawing = ledger
        .create_task(
            project.id,
            &NewTask {
                from: "chief".into(),
                pool: Some("designer".into()),
                body: "Draw it".into(),
                ..NewTask::default()
            },
        )
        .expect("a task for the designer")
        .task
        .number;
    let named = ledger
        .create_task(
            project.id,
            &NewTask {
                from: "chief".into(),
                to: Some("chief".into()),
                body: "Mine".into(),
                ..NewTask::default()
            },
        )
        .expect("a task by name")
        .task
        .number;
    for number in [drawing, named] {
        let refused = edit(&mut ledger, &project, number, &to("light")).expect_err("no tier");
        assert_eq!(refused.code(), Some("invalid-transition"), "T-{number}");
    }
}

#[test]
fn a_task_queued_for_a_member_of_another_tier_goes_back_to_the_board_and_one_for_its_own_stays() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(
        &mut ledger,
        vec![
            member("zeus", &["worker"], "standard"),
            member("hera", &["worker"], "light"),
        ],
    );
    let task = open(&mut ledger, &project, "worker", "standard").number;
    ledger
        .assign_task(project.id, task, id_of(&project, "zeus"))
        .expect("zeus takes it");

    // The tier it already has: nothing moves.
    let same = edit(&mut ledger, &project, task, &to("standard")).expect("the same tier");
    assert_eq!(same.task.state, "queued");

    let edited = edit(&mut ledger, &project, task, &to("light")).expect("edited");
    assert_eq!(
        (
            edited.task.state.as_str(),
            edited.task.assignee.clone(),
            edited.task.tier.as_deref()
        ),
        ("open", None, Some("light")),
        "back on the board, for a light worker"
    );
    let body = ledger
        .task(project.id, task)
        .expect("a read")
        .expect("it")
        .task
        .body;
    assert_eq!(
        body, "Do the work",
        "nothing was given, so nothing is warned of"
    );
    let notes = notes_to_the_chief(&ledger, &project);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].starts_with("T-1 was taken back from @zeus-")
            && notes[0].ends_with(
                " (its tier was changed to light by @chief) and waits for another light worker."
            ),
        "{notes:?}"
    );
    // And the next pass has a light worker to give it to.
    let candidates = ledger.candidates(project.id, task).expect("candidates");
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.member.handle.as_str())
            .collect::<Vec<_>>(),
        ["hera"]
    );
}

#[test]
fn a_task_paused_before_any_window_had_it_keeps_its_pause_and_asks_the_new_tier() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(
        &mut ledger,
        vec![
            member("zeus", &["worker"], "standard"),
            member("hera", &["worker"], "light"),
        ],
    );
    let task = open(&mut ledger, &project, "worker", "standard").number;
    ledger
        .pause_task(project.id, task, Some("chief"), None)
        .expect("paused");
    let edited = edit(&mut ledger, &project, task, &to("light")).expect("edited");
    assert_eq!(
        (edited.task.state.as_str(), edited.task.tier.as_deref()),
        ("paused", Some("light"))
    );
}

#[test]
fn a_task_a_window_has_is_not_edited_for_its_tier_either() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(
        &mut ledger,
        vec![
            member("zeus", &["worker"], "standard"),
            member("hera", &["worker"], "light"),
        ],
    );
    let task = open(&mut ledger, &project, "worker", "standard").number;
    let moved = ledger
        .assign_task(project.id, task, id_of(&project, "zeus"))
        .expect("zeus takes it");
    let message = moved.message.expect("its brief").id;
    ledger.begin_delivery(message).expect("the brief goes in");
    let refused = edit(&mut ledger, &project, task, &to("light")).expect_err("given");
    assert_eq!(refused.code(), Some("already-given"));
}

#[test]
fn the_brief_and_the_tier_change_in_one_step() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(
        &mut ledger,
        vec![
            member("zeus", &["worker"], "standard"),
            member("hera", &["worker"], "light"),
        ],
    );
    let task = open(&mut ledger, &project, "worker", "standard").number;
    let both = TaskEdit {
        body: Some("Do it the new way".into()),
        tier: Some("light".into()),
        ..TaskEdit::default()
    };
    let edited = edit(&mut ledger, &project, task, &both).expect("edited");
    assert_eq!(
        (edited.task.body.as_str(), edited.task.tier.as_deref()),
        ("Do it the new way", Some("light"))
    );
}
