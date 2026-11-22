//! What the staff as slots owes the dispatcher beside the placement (`tier_fallback.rs`): a
//! wait note withdrawn before the chief had it is told again, a fall's note is kept through the
//! assignment that follows it, and the backup sweep asks whether the backup may take a task
//! before it releases one (`architecture/staff-slots.md`, section 4).

use cf_engine::testing::Context;
use cf_ledger::{HoldKind, MemberEdit, NewMember, NewTask, ProjectView, TaskEdit};
use serde_json::{json, Value};

use crate::fixtures::{after, exhausted, now};

fn project_of(context: &Context, name: &str, staff: &[(&str, &[&str], &str)]) -> ProjectView {
    let members: Vec<Value> = staff
        .iter()
        .map(|(agent, roles, tier)| {
            json!({ "agent": agent, "harness": "claude-code", "roles": roles, "tier": tier })
        })
        .collect();
    context
        .open_project(json!({
            "directory": format!("/work/{name}"),
            "name": name,
            "chief": { "harness": "claude-code", "model": "apollo" },
            "staff": members,
        }))
        .unwrap()
        .expect("the project")
}

fn open(
    context: &Context,
    project: &ProjectView,
    pool: &str,
    tier: &str,
    purpose: Option<&str>,
) -> i64 {
    context
        .create_task(
            project.id,
            NewTask {
                from: "chief".to_owned(),
                pool: Some(pool.to_owned()),
                tier: Some(tier.to_owned()),
                purpose: purpose.map(str::to_owned),
                body: "Do the work".to_owned(),
                ..NewTask::default()
            },
        )
        .task
        .number
}

fn backup_of(context: &Context, project: i64, member: &str, name: &str, roles: &[&str]) {
    context
        .ledger
        .borrow_mut()
        .add_backup(
            project,
            member,
            &NewMember {
                harness: "claude-code".to_owned(),
                model: Some(name.to_owned()),
                effort: None,
                model_name: None,
                designer: false,
                roles: roles.iter().map(|role| (*role).to_owned()).collect(),
                tier: "standard".to_owned(),
            },
        )
        .expect("a backup");
}

/// The chief's notes about `task` that begin `prefix`, with their states: the inbox lists the
/// cancelled ones too.
fn notes(context: &Context, project: &ProjectView, prefix: &str) -> Vec<(String, String)> {
    let chief = context.id(project.id, "chief");
    let mut said: Vec<(String, String)> = context
        .inbox(chief)
        .into_iter()
        .filter(|message| message.kind == "note" && message.body.starts_with(prefix))
        .map(|message| (message.state, message.body))
        .collect();
    said.reverse();
    said
}

fn refuse_login(context: &Context, member: &str) {
    context.adapter.refuse(
        member,
        exhausted(Some(&now(context)), Some(&after(context, 20 * 60_000))),
    );
}

fn holder(context: &Context, project: i64, number: i64) -> String {
    let session = context
        .task(project, number)
        .task
        .assignee
        .unwrap_or_else(|| panic!("T-{number} has no assignee"));
    let owner = context
        .project(project)
        .participants
        .into_iter()
        .find(|participant| participant.handle == session)
        .and_then(|participant| participant.member_id)
        .expect("a session of a member");
    context
        .project(project)
        .participants
        .into_iter()
        .find(|participant| participant.id == owner)
        .expect("its member")
        .handle
}

#[test]
fn a_wait_note_withdrawn_before_the_chief_had_it_is_told_again_once() {
    let context = Context::new();
    let project = project_of(
        &context,
        "app",
        &[
            ("zeus", &["worker"], "standard"),
            ("hera", &["reviewer"], "standard"),
        ],
    );
    context.pass().unwrap();
    context.pass().unwrap();
    // The chief is busy: what it is written waits queued.
    context.adapter.with("chief", |agent| agent.settled = false);
    // A review is created while a reviewer is seated; none is left before dispatch.
    let review = open(&context, &project, "reviewer", "standard", None);
    context
        .ledger
        .borrow_mut()
        .remove_member(project.id, "hera")
        .unwrap();
    context.pass().unwrap();
    let first = notes(&context, &project, &format!("T-{review} waits for"));
    assert_eq!(first.len(), 1, "{first:?}");
    // The person pauses and resumes the task before the chief had the note.
    context
        .ledger
        .borrow_mut()
        .pause_task(project.id, review, Some("human"), None)
        .unwrap();
    context
        .ledger
        .borrow_mut()
        .resume_task(project.id, review, Some("human"), "Go on")
        .unwrap();
    let after_resume = notes(&context, &project, &format!("T-{review} waits for"));
    assert_eq!(
        after_resume
            .iter()
            .filter(|(state, _)| state == "cancelled")
            .count(),
        1,
        "the resume withdrew the note: {after_resume:?}"
    );
    context.pass().unwrap();
    let told_again = notes(&context, &project, &format!("T-{review} waits for"));
    assert_eq!(
        told_again
            .iter()
            .filter(|(state, _)| state != "cancelled")
            .count(),
        1,
        "the next pass queues one note: {told_again:?}"
    );
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        notes(&context, &project, &format!("T-{review} waits for")).len(),
        told_again.len(),
        "the pass after tells nothing"
    );
    // The chief rests: it receives that one note.
    context.adapter.with("chief", |agent| agent.settled = true);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let given = notes(&context, &project, &format!("T-{review} waits for"));
    assert_eq!(
        given
            .iter()
            .filter(|(state, _)| state == "delivered" || state == "read")
            .count(),
        1,
        "{given:?}"
    );
}

#[test]
fn the_note_of_where_a_task_fell_stays_queued_through_the_assignment_that_follows_it() {
    let context = Context::new();
    let project = project_of(
        &context,
        "app",
        &[
            ("zeus", &["tester"], "complex"),
            ("anubis", &["tester"], "standard"),
        ],
    );
    let number = open(&context, &project, "tester", "complex", None);
    context
        .ledger
        .borrow_mut()
        .remove_member(project.id, "zeus")
        .unwrap();
    context.pass().unwrap();
    assert!(context.task(project.id, number).task.assignee.is_some());
    let said = notes(&context, &project, &format!("T-{number} asked for"));
    assert_eq!(said.len(), 1, "{said:?}");
    assert_ne!(
        said[0].0, "cancelled",
        "the assignment's withdrawal of the wait notes keeps the fall's: {said:?}"
    );
    assert!(
        said[0].1.contains("@anubis"),
        "it names the holder: {:?}",
        said[0].1
    );
    // The chief rests and receives it: the note that was kept is the one that reaches it.
    context.adapter.with("chief", |agent| agent.settled = true);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let given = notes(&context, &project, &format!("T-{number} asked for"));
    assert_eq!(given.len(), 1, "{given:?}");
    assert!(
        given[0].0 == "delivered" || given[0].0 == "read",
        "the chief received the fall note: {given:?}"
    );
    assert!(given[0].1.contains("@anubis"), "{given:?}");
}

#[test]
fn a_task_that_waited_for_a_slot_and_then_fell_to_a_joiner_withdraws_its_wait_note_and_tells_the_chief_where_it_went(
) {
    let context = Context::new();
    // Only a Critical tester: a task asked at complex waits, told the way on.
    let project = project_of(&context, "app", &[("zeus", &["tester"], "critical")]);
    context.pass().unwrap();
    context.pass().unwrap();
    // The chief is busy: what it is written waits queued.
    context.adapter.with("chief", |agent| agent.settled = false);
    let number = open(&context, &project, "tester", "complex", None);
    context.pass().unwrap();
    let waited = notes(&context, &project, &format!("T-{number} waits for"));
    assert_eq!(waited.len(), 1, "{waited:?}");
    assert_eq!(waited[0].0, "queued", "{waited:?}");
    assert!(context.task(project.id, number).task.assignee.is_none());
    // A standard tester joins: the same task falls to it.
    context
        .ledger
        .borrow_mut()
        .add_member(
            project.id,
            &NewMember {
                harness: "claude-code".to_owned(),
                model: None,
                effort: None,
                model_name: Some("anubis".to_owned()),
                designer: false,
                roles: vec!["tester".to_owned()],
                tier: "standard".to_owned(),
            },
        )
        .unwrap();
    context.pass().unwrap();
    assert_eq!(holder(&context, project.id, number), "anubis");
    let waited = notes(&context, &project, &format!("T-{number} waits for"));
    assert!(
        waited.iter().all(|(state, _)| state == "cancelled"),
        "the wait note is withdrawn once the task has a holder: {waited:?}"
    );
    let fell = notes(&context, &project, &format!("T-{number} asked for"));
    assert_eq!(fell.len(), 1, "{fell:?}");
    assert_ne!(fell[0].0, "cancelled", "{fell:?}");
    assert!(fell[0].1.contains("@anubis"), "{fell:?}");
    // The chief rests: it receives the fall note, and never the wait note.
    context.adapter.with("chief", |agent| agent.settled = true);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let fell = notes(&context, &project, &format!("T-{number} asked for"));
    assert!(
        fell[0].0 == "delivered" || fell[0].0 == "read",
        "the chief received the named fall note: {fell:?}"
    );
    let waited = notes(&context, &project, &format!("T-{number} waits for"));
    assert!(
        waited.iter().all(|(state, _)| state == "cancelled"),
        "{waited:?}"
    );
}

/// A second project with a free member: every pass must end well and give out its task,
/// whatever the first project's sweep did. Its task is given ([`second_task`]) just before
/// the pass that is the first project's decisive one, so that no ordinary pass before it has
/// already given it out: a pass that stopped at the first project would leave it open.
fn second_project(context: &Context) -> ProjectView {
    project_of(context, "other", &[("ares", &["worker"], "standard")])
}

/// The second project's open task, created now.
fn second_task(context: &Context, other: &ProjectView) -> i64 {
    let task = open(context, other, "worker", "standard", None);
    assert!(
        context.task(other.id, task).task.assignee.is_none(),
        "the second project's task is open until the pass"
    );
    task
}

/// After one pass, the second project's task is given out, which a pass that ended at the
/// first project's refusal would not have done.
fn second_was_given_out(context: &Context, other: &ProjectView, task: i64) {
    assert!(
        context.task(other.id, task).task.assignee.is_some(),
        "the second project's task was given out in that same pass"
    );
}

#[test]
fn a_tier_moved_while_a_task_is_held_keeps_it_in_its_members_window_and_ends_no_pass() {
    let context = Context::new();
    let project = project_of(&context, "app", &[("zeus", &["worker"], "standard")]);
    backup_of(&context, project.id, "zeus", "gaia", &["worker"]);
    let other = second_project(&context);
    let work = open(&context, &project, "worker", "standard", None);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(holder(&context, project.id, work), "zeus");
    // The member's tier is edited while it holds the task; its backup follows.
    context
        .ledger
        .borrow_mut()
        .edit_member(
            project.id,
            "zeus",
            &MemberEdit {
                harness: "claude-code".to_owned(),
                model: None,
                effort: None,
                model_name: Some("zeus".to_owned()),
                tier: "complex".to_owned(),
            },
        )
        .unwrap();
    refuse_login(&context, "zeus");
    // One pass ends well on both sides: the first task is held in its member's window, and the
    // second project's task, created just now, is given out.
    let other_task = second_task(&context, &other);
    context.pass().unwrap();
    let task = context.task(project.id, work).task;
    assert_eq!(
        task.state, "paused",
        "the first task stays in its member's window, held"
    );
    assert!(task.held_until.is_some());
    assert_eq!(holder(&context, project.id, work), "zeus");
    second_was_given_out(&context, &other, other_task);
    context.pass().unwrap();
    assert_eq!(holder(&context, project.id, work), "zeus");
}

#[test]
fn critical_takes_only_critical_work_when_the_sweep_hands_a_task_to_a_backup() {
    let context = Context::new();
    let project = project_of(
        &context,
        "app",
        &[
            ("hera", &["researcher", "reviewer"], "critical"),
            ("ares", &["advisor"], "critical"),
        ],
    );
    backup_of(&context, project.id, "hera", "gaia", &["researcher"]);
    backup_of(&context, project.id, "ares", "luna", &["advisor"]);
    let other = second_project(&context);
    let research = open(
        &context,
        &project,
        "researcher",
        "critical",
        Some("important-question"),
    );
    let review = open(
        &context,
        &project,
        "reviewer",
        "critical",
        Some("critical-review"),
    );
    let advice = open(
        &context,
        &project,
        "advisor",
        "critical",
        Some("hard-problem"),
    );
    for _ in 0..3 {
        context.pass().unwrap();
    }
    for number in [research, review, advice] {
        assert!(
            context.task(project.id, number).task.assignee.is_some(),
            "T-{number} is at work"
        );
    }
    let windows_before = context.project(project.id).participants.len();
    // What the update and an older fall leave behind: a research task with no tier, and an
    // advisor's at Critical with no purpose.
    {
        let ledger = context.ledger.borrow();
        let db = ledger.sqlite();
        db.execute(
            "UPDATE task SET tier = NULL, purpose = NULL WHERE number = ?",
            [research],
        )
        .unwrap();
        db.execute("UPDATE task SET purpose = NULL WHERE number = ?", [advice])
            .unwrap();
    }
    refuse_login(&context, "hera");
    refuse_login(&context, "ares");
    let other_task = second_task(&context, &other);
    context.pass().unwrap();
    second_was_given_out(&context, &other, other_task);
    for _ in 0..2 {
        context.pass().unwrap();
    }
    // The research task and the advisor's stay held in their members' windows, none
    // released and no new window opened for them.
    assert_eq!(holder(&context, project.id, research), "hera");
    assert_eq!(holder(&context, project.id, advice), "ares");
    // The review, at Critical with its purpose, starts over in its backup's new window, its
    // critical line first.
    assert_eq!(holder(&context, project.id, review), "gaia");
    let brief = context
        .inbox(context.id(
            project.id,
            &context.task(project.id, review).task.assignee.unwrap(),
        ))
        .into_iter()
        .find(|message| message.kind == "task")
        .expect("its brief")
        .body;
    assert!(
        brief.starts_with("Critical work: critical-review."),
        "{brief}"
    );
    assert_eq!(
        context.project(project.id).participants.len(),
        windows_before + 1,
        "one new window, for the review alone"
    );
    second_was_given_out(&context, &other, other_task);
}

#[test]
fn a_task_taken_back_after_its_member_moved_to_critical_waits_and_goes_after_the_edit_that_names_critical(
) {
    let context = Context::new();
    let project = project_of(&context, "app", &[("calliope", &["reviewer"], "standard")]);
    backup_of(&context, project.id, "calliope", "gaia", &["reviewer"]);
    let other = second_project(&context);
    let review = open(&context, &project, "reviewer", "standard", None);
    context.pass().unwrap();
    context.pass().unwrap();
    let first_window = context.task(project.id, review).task.assignee.unwrap();
    let delivered: Vec<String> = context
        .inbox(context.id(project.id, &first_window))
        .into_iter()
        .filter(|message| message.kind == "task")
        .map(|message| message.body)
        .collect();
    // The only reviewer and its backup are moved to Critical; the login is refused.
    context
        .ledger
        .borrow_mut()
        .edit_member(
            project.id,
            "calliope",
            &MemberEdit {
                harness: "claude-code".to_owned(),
                model: None,
                effort: None,
                model_name: Some("calliope".to_owned()),
                tier: "critical".to_owned(),
            },
        )
        .unwrap();
    refuse_login(&context, "calliope");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        holder(&context, project.id, review),
        "calliope",
        "the task stays held"
    );
    // The person's Reassign puts it on the board, where it waits, told the way on once.
    context
        .ledger
        .borrow_mut()
        .release_task(project.id, review, "by @human")
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let said = notes(&context, &project, &format!("T-{review} waits for"));
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].1.contains(&format!(
            "cf task edit T-{review} --tier critical --purpose <purpose>"
        )),
        "{}",
        said[0].1
    );
    // An edit of its brief is still refused: a window had it.
    let refused = context
        .ledger
        .borrow_mut()
        .edit_task_with(
            project.id,
            review,
            "chief",
            &TaskEdit {
                body: Some("Another brief".to_owned()),
                ..TaskEdit::default()
            },
        )
        .unwrap_err();
    assert_eq!(refused.code(), Some("already-given"));
    // The edit that only routes is taken, and the same task reaches the Critical window.
    context
        .ledger
        .borrow_mut()
        .edit_task_with(
            project.id,
            review,
            "chief",
            &TaskEdit {
                tier: Some("critical".to_owned()),
                purpose: Some("critical-review".to_owned()),
                ..TaskEdit::default()
            },
        )
        .expect("the edit that only routes");
    let other_task = second_task(&context, &other);
    context.pass().unwrap();
    second_was_given_out(&context, &other, other_task);
    assert_eq!(
        holder(&context, project.id, review),
        "gaia",
        "the same pass put the task in its Critical window"
    );
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(holder(&context, project.id, review), "gaia");
    let window = context.task(project.id, review).task.assignee.unwrap();
    let brief = context
        .inbox(context.id(project.id, &window))
        .into_iter()
        .find(|message| message.kind == "task")
        .expect("its brief")
        .body;
    assert!(
        brief.starts_with("Critical work: critical-review."),
        "{brief}"
    );
    // The first window's delivered brief is as it was.
    let now: Vec<String> = context
        .inbox(context.id(project.id, &first_window))
        .into_iter()
        .filter(|message| message.kind == "task")
        .map(|message| message.body)
        .collect();
    assert_eq!(now, delivered);
    second_was_given_out(&context, &other, other_task);
    let _ = HoldKind::Quota;
}

#[test]
fn a_task_from_before_with_no_tier_takes_its_roles_home_tier_and_goes_to_its_slot_or_falls() {
    let context = Context::new();
    let project = project_of(
        &context,
        "app",
        &[
            ("argus", &["researcher"], "standard"),
            ("daedalus", &["architect"], "light"),
        ],
    );
    let research = open(&context, &project, "researcher", "light", None);
    let frame = open(&context, &project, "architect", "light", None);
    // Tasks of a ledger from before the roles had tiers: given with none.
    context
        .ledger
        .borrow()
        .sqlite()
        .execute(
            "UPDATE task SET tier = NULL WHERE number IN (?, ?)",
            [research, frame],
        )
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    // The researcher's home is standard, held: it goes to its slot. The architect's is
    // complex, which nobody holds: it falls to the light one, and the chief is told whose.
    let research_task = context.task(project.id, research).task;
    assert_eq!(
        (
            research_task.tier.as_deref(),
            research_task.assignee.is_some()
        ),
        (Some("standard"), true)
    );
    let frame_task = context.task(project.id, frame).task;
    assert_eq!(
        (frame_task.tier.as_deref(), frame_task.assignee.is_some()),
        (Some("light"), true)
    );
    let said = notes(&context, &project, &format!("T-{frame} asked for"));
    assert_eq!(
        said.iter().map(|(_, body)| body.as_str()).collect::<Vec<_>>(),
        [format!(
            "T-{frame} asked for a complex architect; none is on the staff, so it goes to the light tier: @daedalus."
        )]
    );
    let events: Vec<String> = context
        .ledger
        .borrow()
        .sqlite()
        .prepare("SELECT data FROM event WHERE kind = 'task.retiered' ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(
        events
            .iter()
            .any(|data| data.contains("\"from\":null") && data.contains("\"to\":\"standard\"")),
        "the home tier is logged: {events:?}"
    );
}
