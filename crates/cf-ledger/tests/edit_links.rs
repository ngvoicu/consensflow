//! The tasks a task needs, and those that wait for it, changed with `cf task edit T-3
//! --needs … --before …` (T-1108): a list replaces what the task has, or some are added
//! and some removed; for a task no window has yet (open, queued, paused); no circle, no task
//! that is not there, and all or none with the brief and the tier of the same command.

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, LedgerError, Links, NewChief, NewMember, NewProject, NewTask, Options,
    ProjectView, TaskEdit, TaskEdited,
};

fn project(ledger: &mut Ledger) -> ProjectView {
    let member = |agent: &str, tier: &str| NewMember {
        model: None,
        effort: None,
        model_name: Some(agent.into()),
        harness: "claude-code".into(),
        designer: false,
        roles: vec!["worker".into()],
        tier: tier.into(),
    };
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("apollo".into()),
                effort: None,
                model_name: None,
            },
            staff: vec![member("zeus", "standard"), member("hera", "light")],
            gate: false,
            wake_up: None,
            specs: None,
            staff_rules: None,
            compaction: None,
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

/// A task for a standard worker, which waits on the board.
fn open(ledger: &mut Ledger, project: &ProjectView) -> i64 {
    ledger
        .create_task(
            project.id,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Do the work".into(),
                ..NewTask::default()
            },
        )
        .expect("a task")
        .task
        .number
}

fn needing(links: Links) -> TaskEdit {
    TaskEdit {
        needs: Some(links),
        ..TaskEdit::default()
    }
}

fn before(links: Links) -> TaskEdit {
    TaskEdit {
        before: Some(links),
        ..TaskEdit::default()
    }
}

fn add(numbers: &[i64]) -> Links {
    Links::Change {
        add: numbers.to_vec(),
        remove: Vec::new(),
    }
}

fn remove(numbers: &[i64]) -> Links {
    Links::Change {
        add: Vec::new(),
        remove: numbers.to_vec(),
    }
}

fn edit(
    ledger: &mut Ledger,
    project: &ProjectView,
    number: i64,
    edit: &TaskEdit,
) -> Result<TaskEdited, LedgerError> {
    ledger.edit_task_with(project.id, number, "chief", edit)
}

/// The numbers T-`number` needs now.
fn needs(ledger: &Ledger, project: &ProjectView, number: i64) -> Vec<i64> {
    ledger
        .task(project.id, number)
        .expect("a read")
        .expect("the task")
        .task
        .needs
        .iter()
        .map(|need| need.number)
        .collect()
}

fn ledger_in(dir: &tempfile::TempDir) -> Ledger {
    open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger")
}

fn code(refused: &LedgerError) -> Option<&str> {
    refused.code()
}

#[test]
fn a_list_replaces_what_a_task_needs_and_a_signed_one_adds_and_removes() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let (a, b, c) = (
        open(&mut ledger, &project),
        open(&mut ledger, &project),
        open(&mut ledger, &project),
    );
    let edited = edit(&mut ledger, &project, c, &needing(Links::Set(vec![a, b]))).expect("set");
    assert_eq!(needs(&ledger, &project, c), [a, b]);
    assert_eq!(edited.task.blocked_by, [a, b], "it waits for both");

    edit(&mut ledger, &project, c, &needing(Links::Set(vec![b]))).expect("replaced");
    assert_eq!(
        needs(&ledger, &project, c),
        [b],
        "the list replaces, a is gone"
    );

    edit(&mut ledger, &project, c, &needing(add(&[a, b]))).expect("added");
    assert_eq!(
        needs(&ledger, &project, c),
        [a, b],
        "b was there already: once"
    );

    let edited = edit(&mut ledger, &project, c, &needing(remove(&[a, 99]))).expect("removed");
    assert_eq!(
        needs(&ledger, &project, c),
        [b],
        "99 was never there: nothing"
    );
    assert_eq!(edited.task.blocked_by, [b]);

    let freed = edit(&mut ledger, &project, c, &needing(Links::Set(vec![]))).expect("cleared");
    assert_eq!(
        (needs(&ledger, &project, c), freed.task.blocked_by),
        (Vec::<i64>::new(), Vec::<i64>::new()),
        "nothing is waited for, and the board may give it"
    );
}

#[test]
fn the_tasks_that_wait_for_a_task_are_changed_the_same_way_and_said() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let (a, b, c) = (
        open(&mut ledger, &project),
        open(&mut ledger, &project),
        open(&mut ledger, &project),
    );
    let edited = edit(&mut ledger, &project, a, &before(Links::Set(vec![b, c]))).expect("set");
    assert_eq!(edited.waiting, Some(vec![b, c]), "what waits for T-1 now");
    assert_eq!(needs(&ledger, &project, b), [a]);
    assert_eq!(needs(&ledger, &project, c), [a]);

    let edited = edit(&mut ledger, &project, a, &before(remove(&[b]))).expect("removed");
    assert_eq!(edited.waiting, Some(vec![c]));
    assert_eq!(needs(&ledger, &project, b), Vec::<i64>::new());

    let edited = edit(&mut ledger, &project, a, &before(Links::Set(vec![b]))).expect("replaced");
    assert_eq!(edited.waiting, Some(vec![b]));
    assert_eq!(
        needs(&ledger, &project, c),
        Vec::<i64>::new(),
        "c waits no more"
    );
}

#[test]
fn a_circle_is_refused_near_or_far_and_a_task_cannot_wait_for_itself() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let (a, b, c) = (
        open(&mut ledger, &project),
        open(&mut ledger, &project),
        open(&mut ledger, &project),
    );
    edit(&mut ledger, &project, a, &needing(Links::Set(vec![b]))).expect("a needs b");
    edit(&mut ledger, &project, b, &needing(Links::Set(vec![c]))).expect("b needs c");
    for (task, change, why) in [
        (b, needing(Links::Set(vec![a])), "b needs a, which needs b"),
        (
            c,
            needing(add(&[a])),
            "c needs a, which needs b, which needs c",
        ),
        (c, before(Links::Set(vec![c])), "c waits for itself"),
        (a, needing(Links::Set(vec![a])), "a needs a"),
        (
            a,
            before(add(&[c])),
            "c waits for a, which needs b, which needs c",
        ),
    ] {
        let refused = edit(&mut ledger, &project, task, &change).expect_err(why);
        assert_eq!(code(&refused), Some("circular-needs"), "{why}");
    }
    assert_eq!(
        needs(&ledger, &project, b),
        [c],
        "nothing of the refused was kept"
    );
}

#[test]
fn a_task_that_is_not_there_or_cannot_be_waited_for_is_refused() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let (a, b, c) = (
        open(&mut ledger, &project),
        open(&mut ledger, &project),
        open(&mut ledger, &project),
    );
    for change in [needing(Links::Set(vec![99])), before(add(&[99]))] {
        let refused = edit(&mut ledger, &project, a, &change).expect_err("no such task");
        assert_eq!(code(&refused), Some("unknown-task"));
    }
    ledger
        .cancel_task(project.id, b, "chief")
        .expect("cancelled");
    let refused = edit(&mut ledger, &project, a, &needing(add(&[b]))).expect_err("cancelled");
    assert_eq!(code(&refused), Some("need-cancelled"));
    // What waits for a task is still on the board.
    let moved = ledger
        .assign_task(project.id, c, id_of(&project, "zeus"))
        .expect("zeus takes it");
    ledger
        .begin_delivery(moved.message.expect("its brief").id)
        .expect("begun");
    let refused = edit(&mut ledger, &project, a, &before(add(&[c]))).expect_err("not open");
    assert_eq!(code(&refused), Some("not-on-the-board"));
}

#[test]
fn a_task_a_window_has_begun_is_not_edited_for_what_it_needs_either() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let (a, c) = (open(&mut ledger, &project), open(&mut ledger, &project));
    let moved = ledger
        .assign_task(project.id, a, id_of(&project, "zeus"))
        .expect("zeus takes it");
    ledger
        .begin_delivery(moved.message.expect("its brief").id)
        .expect("begun");
    let refused = edit(&mut ledger, &project, a, &needing(add(&[c]))).expect_err("given");
    assert_eq!(code(&refused), Some("already-given"));
}

#[test]
fn a_task_queued_for_a_window_that_now_waits_goes_back_to_the_board_to_wait_there() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let (queued, other) = (open(&mut ledger, &project), open(&mut ledger, &project));
    ledger
        .assign_task(project.id, queued, id_of(&project, "zeus"))
        .expect("zeus takes it");
    let edited = edit(
        &mut ledger,
        &project,
        queued,
        &needing(Links::Set(vec![other])),
    )
    .expect("edited");
    assert_eq!(
        (
            edited.task.state.as_str(),
            edited.task.assignee,
            edited.task.blocked_by,
            edited.task.body.as_str()
        ),
        ("open", None, vec![other], "Do the work"),
        "on the board, waiting, and no warning of partial changes: nothing was begun"
    );
    // One paused with its window cannot go back to the board without losing the pause.
    let paused = open(&mut ledger, &project);
    ledger
        .assign_task(project.id, paused, id_of(&project, "zeus"))
        .expect("zeus takes it");
    ledger
        .pause_task(project.id, paused, Some("chief"), None)
        .expect("paused with its window");
    let refused = edit(
        &mut ledger,
        &project,
        paused,
        &needing(Links::Set(vec![other])),
    )
    .expect_err("paused with a window");
    assert_eq!(code(&refused), Some("invalid-transition"));
}

#[test]
fn a_task_paused_before_any_window_had_it_takes_new_needs_and_stays_paused() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let (a, b) = (open(&mut ledger, &project), open(&mut ledger, &project));
    ledger
        .pause_task(project.id, a, Some("chief"), None)
        .expect("paused");
    let edited = edit(&mut ledger, &project, a, &needing(Links::Set(vec![b]))).expect("edited");
    assert_eq!(
        (edited.task.state.as_str(), edited.task.blocked_by),
        ("paused", vec![b])
    );
}

#[test]
fn the_brief_the_tier_and_the_lists_change_together_or_not_at_all() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let (a, b) = (open(&mut ledger, &project), open(&mut ledger, &project));
    let refused = edit(
        &mut ledger,
        &project,
        a,
        &TaskEdit {
            body: Some("A new brief".into()),
            tier: Some("light".into()),
            needs: Some(Links::Set(vec![99])),
            before: Some(Links::Set(vec![b])),
        },
    )
    .expect_err("no T-99");
    assert_eq!(code(&refused), Some("unknown-task"));
    let read = ledger
        .task(project.id, a)
        .expect("a read")
        .expect("it")
        .task;
    assert_eq!(
        (
            read.body.as_str(),
            read.tier.as_deref(),
            needs(&ledger, &project, b)
        ),
        ("Do the work", Some("standard"), Vec::<i64>::new()),
        "nothing of it was kept"
    );
    let edited = edit(
        &mut ledger,
        &project,
        a,
        &TaskEdit {
            body: Some("A new brief".into()),
            tier: Some("light".into()),
            needs: None,
            before: Some(Links::Set(vec![b])),
        },
    )
    .expect("edited");
    assert_eq!(
        (
            edited.task.body.as_str(),
            edited.task.tier.as_deref(),
            edited.waiting
        ),
        ("A new brief", Some("light"), Some(vec![b]))
    );
}
