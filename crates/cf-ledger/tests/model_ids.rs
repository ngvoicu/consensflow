//! Members are named by their model's id and effort (spec `harness-models`, HMD-12), through
//! the ledger's public doors alone: a ledger 3.0.7-alpha.2 left (schema 32) opens at this
//! build's schema with every member renamed and its windows after it, tasks and messages
//! still its authors', the names left kept; and Edit member renames a member and its windows.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use cf_ledger::{
    migrate, open_ledger, MemberEdit, NewChief, NewMember, NewProject, NewTask, Options, MIGRATIONS,
};
use rusqlite::Connection;
use serde_json::{json, Value};

/// A home with the board alpha.2 left: its staff named by the name their harness's list gave
/// the model (`opus-high`), the designer on Codex's default, a window with a task and a message.
fn alpha_2_home() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    let db = Connection::open(&file).unwrap();
    migrate(&db, &MIGRATIONS[..32]).unwrap();
    db.execute_batch(
        "INSERT INTO project (id, directory, name, state, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'open', 't', 't');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, model, effort, model_name, tier, member_id, created_at, designer) VALUES
           (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, NULL, NULL, NULL, NULL, 't', 0),
           (2, 1, 'chief', 'chief', '[]', NULL, 'claude-code', 'claude-opus-5-5', 'xhigh', 'Opus', NULL, NULL, 't', 0),
           (3, 1, 'opus-high', 'worker', '[\"worker\"]', 'poseidon', 'claude-code', 'claude-opus-5-5', 'high', 'Opus', 'complex', NULL, 't', 0),
           (4, 1, 'opus-high-frosty-cliff', 'worker', '[\"worker\"]', 'poseidon', 'claude-code', 'claude-opus-5-5', 'high', 'Opus', 'complex', 3, 't', 0),
           (5, 1, 'gpt-6-astra-xhigh', 'advisor', '[\"advisor\"]', NULL, 'codex', 'gpt-6-astra', 'xhigh', 'GPT-6-Astra', 'critical', NULL, 't', 0),
           (6, 1, 'codex-default', 'designer', '[\"designer\"]', 'pygmalion', 'codex', NULL, NULL, NULL, 'light', NULL, 't', 1);
         INSERT INTO task (id, project_id, number, title, body, requester_id, assignee_id, state, created_at, updated_at)
           VALUES (1, 1, 1, 'Fix', 'Fix it', 2, 4, 'working', 't', 't');
         INSERT INTO message (id, project_id, recipient_id, sender_id, kind, task_id, body, state, created_at)
           VALUES (1, 1, 2, 4, 'result', 1, 'Done', 'queued', 't');",
    )
    .unwrap();
    (dir, file)
}

fn handles(file: &std::path::Path) -> Vec<String> {
    let db = Connection::open(file).unwrap();
    let mut read = db
        .prepare("SELECT handle FROM participant ORDER BY id")
        .unwrap();
    let rows = read.query_map([], |row| row.get(0)).unwrap();
    rows.map(Result::unwrap).collect()
}

#[test]
fn an_alpha_2_ledger_opens_with_each_member_named_by_its_model_s_id_and_its_windows_after_it() {
    let (_dir, file) = alpha_2_home();
    let ledger = open_ledger(&file, Options::default()).expect("the ledger opens");
    // The names the migration made, the chief's and the human's as they were, the designer on
    // Codex's default as it was.
    let view = ledger.project(1).unwrap().unwrap();
    assert_eq!(
        view.participants
            .iter()
            .map(|p| p.handle.as_str())
            .collect::<Vec<_>>(),
        [
            "human",
            "chief",
            "claude-opus-5-5-high",
            "claude-opus-5-5-high-frosty-cliff",
            "gpt-6-astra-xhigh",
            "codex-default"
        ]
    );
    // History keeps what each was called: the nickname in its row, the name before in the list.
    let say = |handle: &str| -> Value {
        let found = view
            .participants
            .iter()
            .find(|p| p.handle == handle)
            .unwrap();
        serde_json::to_value(found).unwrap()
    };
    assert_eq!(say("claude-opus-5-5-high")["agent"], "poseidon");
    assert_eq!(
        say("claude-opus-5-5-high")["formerHandles"],
        json!(["opus-high"])
    );
    assert_eq!(
        say("claude-opus-5-5-high-frosty-cliff")["formerHandles"],
        json!(["opus-high-frosty-cliff"])
    );
    // What the board shows of the model is its id; and the card says no more of a name that
    // holds it, and says what the designer runs on.
    assert_eq!(say("claude-opus-5-5-high")["modelName"], "claude-opus-5-5");
    assert_eq!(say("claude-opus-5-5-high").get("nameApart"), None);
    assert_eq!(say("codex-default")["nameApart"], true);
    // The task and the message keep their authors, by id.
    let thread = ledger.task(1, 1).unwrap().unwrap();
    assert_eq!(
        thread.task.assignee.as_deref(),
        Some("claude-opus-5-5-high-frosty-cliff")
    );
    // And it opens again as it was: nothing is renamed twice.
    drop(ledger);
    open_ledger(&file, Options::default()).expect("again");
    assert_eq!(handles(&file)[2], "claude-opus-5-5-high");
}

#[test]
fn edit_member_names_a_member_and_its_windows_by_the_model_it_runs_now() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(
        &dir.path().join("consensflow.db"),
        Options {
            names: Box::new(|| "amber-pine".to_owned()),
            ..Options::default()
        },
    )
    .unwrap();
    let member = NewMember {
        harness: "claude-code".into(),
        model: Some("claude-opus-5-5".into()),
        effort: Some("high".into()),
        model_name: Some("claude-opus-5-5".into()),
        designer: false,
        roles: vec!["worker".into()],
        tier: "complex".into(),
    };
    let project = ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("claude-opus-5-5".into()),
                effort: None,
            },
            staff: vec![member],
            gate: false,
            wake_up: None,
            specs: None,
        })
        .unwrap();
    let member_id = project
        .participants
        .iter()
        .find(|p| p.handle == "claude-opus-5-5-high")
        .expect("named by its id and effort")
        .id;
    ledger
        .create_task(
            project.id,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("complex".into()),
                body: "Fix the build".into(),
                ..NewTask::default()
            },
        )
        .unwrap();
    ledger.assign_task(project.id, 1, member_id).unwrap();
    let edited = ledger
        .edit_member(
            project.id,
            "claude-opus-5-5-high",
            &MemberEdit {
                harness: "claude-code".into(),
                model: Some("claude-fable-5-1".into()),
                effort: Some("max".into()),
                model_name: Some("claude-fable-5-1".into()),
                tier: "critical".into(),
            },
        )
        .unwrap();
    let edited = serde_json::to_value(&edited).unwrap();
    assert_eq!(edited["handle"], "claude-fable-5-1-max");
    assert_eq!(edited["formerHandles"], json!(["claude-opus-5-5-high"]));
    let view = ledger.project(project.id).unwrap().unwrap();
    let window = view
        .participants
        .iter()
        .find(|p| p.member_id == Some(member_id))
        .unwrap();
    assert_eq!(window.handle, "claude-fable-5-1-max-amber-pine");
    assert_eq!(
        serde_json::to_value(window).unwrap()["formerHandles"],
        json!(["claude-opus-5-5-high-amber-pine"])
    );
    // A member that needs only another tier keeps its name; the chief and the human theirs.
    let kept = ledger
        .edit_member(
            project.id,
            "claude-fable-5-1-max",
            &MemberEdit {
                harness: "claude-code".into(),
                model: Some("claude-fable-5-1".into()),
                effort: Some("max".into()),
                model_name: Some("claude-fable-5-1".into()),
                tier: "light".into(),
            },
        )
        .unwrap();
    assert_eq!(kept.handle, "claude-fable-5-1-max");
    assert!(view.participants.iter().any(|p| p.handle == "chief"));
}
