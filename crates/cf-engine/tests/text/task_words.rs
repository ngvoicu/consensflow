//! The words ConsensFlow sets in and before a task's own — the lead a critical
//! task is given with, the labels a resume, a release and a tell add to a
//! brief, and what is kept of a window that ended — held to the member texts'
//! bans (`support::MEMBER_BANS`). A critical task's lead once sent its answers
//! to a "coordinator", a word the role texts ban, and forbade work on
//! specifications a project may not have (T-1060): no banned word comes back
//! in a lead the role tests never read.

use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewNote, NewProject, NewTask, Options, RESUME_WORDS,
};
use serde_json::json;

use super::support::{found, MEMBER_BANS};

/// A project at /work/app: apollo its chief, zeus a critical worker, diana a
/// standard one.
fn world() -> (tempfile::TempDir, Ledger, i64) {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), Options::default()).unwrap();
    let member = |agent: &str, tier: &str| NewMember {
        model: None,
        effort: None,
        model_name: Some(agent.into()),
        harness: "claude-code".into(),
        designer: false,
        roles: vec!["worker".into()],
        tier: tier.into(),
    };
    let project = ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("apollo".into()),
                effort: None,
                model_name: None,
            },
            staff: vec![member("zeus", "critical"), member("diana", "standard")],
            gate: false,
            wake_up: None,
            specs: None,
            staff_rules: None,
            compaction: None,
            advisor_tool: false,
        })
        .unwrap();
    (dir, ledger, project.id)
}

/// The id of the participant whose handle is `handle`, in `project`.
fn id_of(ledger: &Ledger, project: i64, handle: &str) -> i64 {
    ledger
        .project(project)
        .unwrap()
        .unwrap()
        .participants
        .iter()
        .find(|participant| participant.handle == handle)
        .unwrap_or_else(|| panic!("no @{handle}"))
        .id
}

/// A task of the `worker` pool at `tier` is opened in `project`; its number.
fn add_task(
    ledger: &mut Ledger,
    project: i64,
    tier: &str,
    purpose: Option<&str>,
    body: &str,
) -> i64 {
    ledger
        .create_task(
            project,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some(tier.into()),
                purpose: purpose.map(str::to_owned),
                body: body.into(),
                ..NewTask::default()
            },
        )
        .unwrap()
        .task
        .number
}

/// Message `id` is pasted and proved: its task is at work.
fn deliver(ledger: &mut Ledger, id: i64) {
    ledger.begin_delivery(id).unwrap();
    ledger
        .confirm_delivery(id, Some(&json!({ "item": format!("i-{id}") })))
        .unwrap();
}

/// The task's body on the board.
fn brief_of(ledger: &Ledger, project: i64, number: i64) -> String {
    ledger.task(project, number).unwrap().unwrap().task.body
}

/// A critical task's lead, in the role texts' own words: the work changes no
/// project file — true of every project, specs or none, where the old words forbade
/// specifications outright — and its findings go to the chief.
#[test]
fn a_critical_task_s_lead_says_the_task_changes_no_project_file_and_answers_to_the_chief() {
    let (_dir, mut ledger, project) = world();
    let task = add_task(
        &mut ledger,
        project,
        "critical",
        Some("hard-problem"),
        "Why is it slow?",
    );
    let zeus = id_of(&ledger, project, "zeus");
    let assigned = ledger.assign_task(project, task, zeus).unwrap();
    assert_eq!(
        assigned.message.unwrap().body,
        "Critical work: hard-problem. It changes no project file: return analysis, evidence and recommendations to the chief.\n\nWhy is it slow?"
    );
}

/// Every lead ConsensFlow adds to a task, produced as a window reads it, holds
/// the bans the member texts keep: no old command, no "the human", no
/// task-giving, no "PM" or "coordinator", no verdict.
#[test]
fn the_words_consensflow_adds_to_a_task_hold_the_member_texts_bans() {
    let (_dir, mut ledger, project) = world();
    let mut added: Vec<String> = Vec::new();

    // A critical task's lead, queued with its brief for a member's session.
    let task = add_task(
        &mut ledger,
        project,
        "critical",
        Some("hard-problem"),
        "Why is it slow?",
    );
    let assigned = ledger
        .assign_task(project, task, id_of(&ledger, project, "zeus"))
        .unwrap();
    let window = assigned.task.assignee.unwrap();
    let brief = assigned.message.unwrap();
    added.push(brief.body);
    deliver(&mut ledger, brief.id);

    // The words a window is resumed with: the chief's, and the daemon's fixed
    // ones when a quota hold ends.
    ledger
        .pause_task(project, task, Some("chief"), None)
        .unwrap();
    let resumed = ledger
        .resume_task(project, task, Some("chief"), "Take v2")
        .unwrap()
        .message
        .unwrap();
    added.push(resumed.body);
    deliver(&mut ledger, resumed.id);
    ledger
        .hold_task(project, task, "2026-10-10T15:00:00.000Z", "out of quota")
        .unwrap();
    let resumed = ledger
        .resume_task(project, task, None, RESUME_WORDS)
        .unwrap()
        .message
        .unwrap();
    added.push(resumed.body);
    deliver(&mut ledger, resumed.id);

    // What a release puts back on the board: the rows its window never read,
    // kept under their labels, and who it is reassigned from.
    ledger
        .note(
            project,
            &NewNote {
                from: Some("chief".into()),
                to: window.clone(),
                task: Some(task),
                body: "Mind the tests".into(),
            },
        )
        .unwrap();
    added.push(
        ledger
            .release_task(project, task, "ran out of quota")
            .unwrap()
            .task
            .body,
    );

    // The tell an open task keeps in its brief for whichever window takes it.
    let told = add_task(&mut ledger, project, "standard", None, "Parser");
    added.push(
        ledger
            .tell_open(project, told, "chief", "Prefer the fast path")
            .unwrap()
            .body,
    );

    // The rows a session that ended kept, folded into the brief its task goes
    // back to the board with.
    let moved = add_task(&mut ledger, project, "standard", None, "Loader");
    let assigned = ledger
        .assign_task(project, moved, id_of(&ledger, project, "diana"))
        .unwrap();
    let ended = assigned.task.assignee.unwrap();
    deliver(&mut ledger, assigned.message.unwrap().id);
    ledger
        .note(
            project,
            &NewNote {
                from: Some("chief".into()),
                to: ended.clone(),
                task: Some(moved),
                body: "Mind the tests".into(),
            },
        )
        .unwrap();
    ledger
        .pause_task(project, moved, Some("chief"), None)
        .unwrap();
    ledger.end_session(project, &ended, "human").unwrap();
    let resumed = ledger
        .resume_task(project, moved, Some("chief"), "Back on")
        .unwrap();
    assert!(resumed.message.is_none(), "the task goes back to the board");
    added.push(brief_of(&ledger, project, moved));

    // Every lead is here, or the bans were read of nothing.
    for lead in [
        "Critical work: hard-problem.",
        "Resumed: Take v2",
        "Resumed: Go on where you stopped.",
        "Kept from before, never delivered to @",
        "Reassigned from @",
        "Told by @chief before a window had it:",
        "Resumed after a pause, in a fresh window",
    ] {
        assert!(
            added.iter().any(|text| text.contains(lead)),
            "nothing added reads `{lead}`"
        );
    }
    for text in &added {
        for (pattern, why) in MEMBER_BANS {
            assert!(!found(pattern, text), "a member {why}:\n{text}");
        }
    }
}
