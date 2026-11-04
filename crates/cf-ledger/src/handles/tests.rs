use rusqlite::Connection;

use super::*;
use crate::schema::{migrate, MIGRATIONS};

fn none_taken(_: &str) -> bool {
    false
}

#[test]
fn a_handle_is_the_model_s_name_and_its_effort_in_lowercase_words() {
    for (shown, effort, handle) in [
        ("Opus", Some("high"), "opus-high"),
        ("GPT-6-Astra", Some("max"), "gpt-6-astra-max"),
        ("GPT-6.1-Sol", Some("xhigh"), "gpt-6-1-sol-xhigh"),
        ("SWE-2", Some("max"), "swe-2-max"),
        ("Adaptive", None, "adaptive"),
        (
            "Claude Haiku 4.5 (latest)",
            Some("off"),
            "claude-haiku-4-5-latest-off",
        ),
        (
            "muse-spark-1.3-contributor",
            Some("max"),
            "muse-spark-1-3-contributor-max",
        ),
        ("Codex default", None, "codex-default"),
        ("Ünïcode ✓", None, "n-code"),
        ("✓", None, "member"),
    ] {
        assert_eq!(
            member_handle(shown, effort, "claude-code", none_taken),
            handle,
            "{shown}"
        );
    }
}

#[test]
fn a_taken_handle_puts_the_harness_in_front_then_a_number_after() {
    let taken = [
        "opus-high",
        "claude-code-opus-high",
        "claude-code-opus-high-2",
    ];
    assert_eq!(
        member_handle("Opus", Some("high"), "claude-code", |h| taken.contains(&h)),
        "claude-code-opus-high-3"
    );
    assert_eq!(
        member_handle("Opus", Some("high"), "devin", |h| taken.contains(&h)),
        "devin-opus-high"
    );
    // A name that says its effort already says it once.
    assert_eq!(
        member_handle("claude-opus-5-5-max", Some("max"), "devin", none_taken),
        "claude-opus-5-5-max"
    );
}

#[test]
fn a_handle_is_short_enough_for_its_windows_and_ends_on_a_word() {
    let long = "a very long model name that some harness may send one day 123456789";
    let handle = member_handle(long, Some("high"), "pi", none_taken);
    assert!(handle.len() <= MEMBER_HANDLE_MAX, "{handle}");
    assert!(!handle.ends_with('-'), "{handle}");
}

#[test]
fn a_model_is_shown_by_its_name_else_its_id_else_as_its_harness_s_default() {
    assert_eq!(
        model_shown(Some("Opus"), Some("claude-opus-5-5"), Some("claude-code")),
        "Opus"
    );
    assert_eq!(
        model_shown(None, Some("claude-opus-5-5"), Some("claude-code")),
        "claude-opus-5-5"
    );
    assert_eq!(model_shown(Some("  "), Some("m"), None), "m");
    assert_eq!(model_shown(None, None, Some("codex")), "Codex default");
}

/// A ledger 0030 left, of the board's shape before the models were the harnesses' own: a
/// chief and members of ready-made agents, a window of one with a task and a message.
fn before_0031() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    migrate(&db, &MIGRATIONS[..30]).unwrap();
    db.execute_batch(
        "INSERT INTO project (id, directory, name, state, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'open', 't', 't');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, created_at, left_at, designer) VALUES
           (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, NULL, 't', NULL, 0),
           (2, 1, 'chief', 'chief', '[]', 'apollo', 'claude-code', NULL, NULL, 't', NULL, 0),
           (3, 1, 'poseidon', 'worker', '[\"worker\"]', 'poseidon', 'claude-code', 'complex', NULL, 't', NULL, 0),
           (4, 1, 'pygmalion', 'designer', '[\"designer\"]', 'pygmalion', 'codex', 'light', NULL, 't', NULL, 1),
           (5, 1, 'sobek', 'worker', '[\"worker\"]', 'sobek', 'devin', 'light', NULL, 't', NULL, 0),
           (6, 1, 'buri', 'worker', '[\"worker\"]', 'buri', 'hermes-code', 'light', NULL, 't', 't', 0),
           (7, 1, 'artemis', 'worker', '[\"worker\"]', 'artemis', 'claude-code', 'standard', NULL, 't', NULL, 0),
           (8, 1, 'poseidon-frosty-cliff', 'worker', '[\"worker\"]', 'poseidon', 'claude-code', 'complex', 3, 't', NULL, 0),
           (9, 1, 'zeus', 'worker', '[\"worker\"]', 'zeus', 'claude-code', 'critical', NULL, 't', NULL, 0),
           (10, 1, 'apollo', 'advisor', '[\"advisor\"]', 'apollo', 'claude-code', 'critical', NULL, 't', NULL, 0),
           (11, 1, 'khepri', 'tester', '[\"tester\"]', 'khepri', 'devin', 'standard', NULL, 't', NULL, 0);
         INSERT INTO task (id, project_id, number, title, body, requester_id, assignee_id, state, created_at, updated_at)
           VALUES (1, 1, 1, 'Fix', 'Fix it', 2, 8, 'working', 't', 't');
         INSERT INTO message (id, project_id, recipient_id, sender_id, kind, task_id, body, state, created_at)
           VALUES (1, 1, 2, 8, 'result', 1, 'Done', 'queued', 't');",
    )
    .unwrap();
    db
}

/// A participant row as the migration tests read it: its id, handle, agent, harness, model,
/// effort, model's name and tier.
type Row = (
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

#[test]
fn migration_0031_runs_each_ready_made_agent_s_member_on_its_model_and_names_it_by_it() {
    let db = before_0031();
    migrate(&db, &MIGRATIONS).unwrap();
    let rows: Vec<Row> = db
        .prepare(
            "SELECT id, handle, agent, harness, model, effort, model_name, tier FROM participant ORDER BY id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let s = |text: &str| Some(text.to_owned());
    assert_eq!(
        rows,
        [
            (1, "human".to_owned(), None, None, None, None, None, None),
            // The chief keeps its handle, and runs on its agent's model where it named none.
            (
                2,
                "chief".to_owned(),
                s("apollo"),
                s("claude-code"),
                s("claude-opus-5-5"),
                s("xhigh"),
                s("Opus"),
                None
            ),
            (
                3,
                "opus-high".to_owned(),
                s("poseidon"),
                s("claude-code"),
                s("claude-opus-5-5"),
                s("high"),
                s("Opus"),
                s("complex")
            ),
            // The image designer on Codex's default, as it ran.
            (
                4,
                "codex-default".to_owned(),
                s("pygmalion"),
                s("codex"),
                None,
                None,
                None,
                s("light")
            ),
            // Devin's model is the id its launch passed, the effort joined as it joined it.
            (
                5,
                "swe-2-max".to_owned(),
                s("sobek"),
                s("devin"),
                s("swe-2-max"),
                s("max"),
                s("SWE-2"),
                s("light")
            ),
            // An agent the catalog never had keeps its harness, no model and the name it had:
            // it needs an edit, and is never turned into its harness's default.
            (
                6,
                "buri".to_owned(),
                s("buri"),
                s("hermes-code"),
                None,
                None,
                None,
                s("light")
            ),
            (
                7,
                "opus-medium".to_owned(),
                s("artemis"),
                s("claude-code"),
                s("claude-opus-5-5"),
                s("medium"),
                s("Opus"),
                s("standard")
            ),
            // A window: its member's new handle and its own two words.
            (
                8,
                "opus-high-frosty-cliff".to_owned(),
                s("poseidon"),
                s("claude-code"),
                s("claude-opus-5-5"),
                s("high"),
                s("Opus"),
                s("complex")
            ),
            (
                9,
                "opus-max".to_owned(),
                s("zeus"),
                s("claude-code"),
                s("claude-opus-5-5"),
                s("max"),
                s("Opus"),
                s("critical")
            ),
            (
                10,
                "opus-xhigh".to_owned(),
                s("apollo"),
                s("claude-code"),
                s("claude-opus-5-5"),
                s("xhigh"),
                s("Opus"),
                s("critical")
            ),
            (
                11,
                "gpt-6-1-sol-xhigh".to_owned(),
                s("khepri"),
                s("devin"),
                s("gpt-6-1-sol-xhigh"),
                s("xhigh"),
                s("GPT-6.1 Sol"),
                s("standard")
            ),
        ]
    );
    // Every author and assignee stays: they are participants by id.
    let (assignee, sender): (i64, i64) = db
        .query_row(
            "SELECT t.assignee_id, m.sender_id FROM task t JOIN message m ON m.task_id = t.id",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((assignee, sender), (8, 8));
}

#[test]
fn migration_0031_puts_the_harness_in_front_of_a_name_two_members_share() {
    let db = Connection::open_in_memory().unwrap();
    migrate(&db, &MIGRATIONS[..30]).unwrap();
    // Horus is Claude Opus 5.5 at high on Devin, Iapetus the same on Pi; a third names an
    // agent the catalog never had.
    db.execute_batch(
        "INSERT INTO project (id, directory, name, state, created_at, updated_at)
           VALUES (1, '/w', 'w', 'open', 't', 't');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, created_at) VALUES
           (1, 1, 'horus', 'worker', '[]', 'horus', 'devin', 'complex', 't'),
           (2, 1, 'iapetus', 'worker', '[]', 'iapetus', 'pi', 'complex', 't'),
           (3, 1, 'mine', 'worker', '[]', 'mine', 'claude-code', 'complex', 't');",
    )
    .unwrap();
    migrate(&db, &MIGRATIONS).unwrap();
    let handles: Vec<String> = db
        .prepare("SELECT handle FROM participant ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        handles,
        ["claude-opus-5-5-high", "pi-claude-opus-5-5-high", "mine"]
    );
}

/// A ledger 0030 left in a file, whose participants run on agents of the human's own: one
/// they defined (`my-coder`, `my-pi`, `my-chief`), one Add member made for a model
/// (T-1143's `codex-gpt-6-astra-high`), a ready-made one the human edited (`zeus`), and one
/// that is in no file and no catalog (`ghost`); `agents` is the agents file beside it, or
/// none.
fn custom_0030(agents: Option<serde_json::Value>) -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let db = Connection::open(dir.path().join("consensflow.db")).unwrap();
    migrate(&db, &MIGRATIONS[..30]).unwrap();
    db.execute_batch(
        "INSERT INTO project (id, directory, name, state, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'open', 't', 't');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, model, tier, member_id, created_at, left_at, designer) VALUES
           (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, NULL, NULL, 't', NULL, 0),
           (2, 1, 'chief', 'chief', '[]', 'my-chief', 'claude-code', NULL, NULL, NULL, 't', NULL, 0),
           (3, 1, 'my-coder', 'worker', '[\"worker\"]', 'my-coder', 'codex', NULL, 'standard', NULL, 't', NULL, 0),
           (4, 1, 'codex-gpt-6-astra-high', 'reviewer', '[\"reviewer\"]', 'codex-gpt-6-astra-high', 'codex', 'gpt-6-astra', 'complex', NULL, 't', NULL, 0),
           (5, 1, 'my-pi', 'advisor', '[\"advisor\"]', 'my-pi', 'pi', NULL, 'critical', NULL, 't', NULL, 0),
           (6, 1, 'zeus', 'worker', '[\"worker\"]', 'zeus', 'claude-code', NULL, 'critical', NULL, 't', NULL, 0),
           (7, 1, 'ghost', 'worker', '[\"worker\"]', 'ghost', 'codex', NULL, 'light', NULL, 't', NULL, 0),
           (8, 1, 'my-coder-amber-pine', 'worker', '[\"worker\"]', 'my-coder', 'codex', NULL, 'standard', 3, 't', NULL, 0),
           (9, 1, 'ghost-brisk-birch', 'worker', '[\"worker\"]', 'ghost', 'codex', NULL, 'light', 7, 't', NULL, 0),
           (10, 1, 'my-devin', 'tester', '[\"tester\"]', 'my-devin', 'devin', NULL, 'standard', NULL, 't', NULL, 0);",
    )
    .unwrap();
    if let Some(agents) = agents {
        std::fs::write(dir.path().join("agents.json"), agents.to_string()).unwrap();
    }
    (dir, db)
}

/// The agents file of [`custom_0030`]'s human, as 3.0.6 wrote it.
fn their_agents() -> serde_json::Value {
    serde_json::json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "my-chief", "kind": "claude-code", "model": "claude-sonnet-5-5", "effort": "high", "workTier": "standard" },
            { "id": "my-coder", "kind": "codex", "model": "gpt-6.1-sol", "effort": "ultra", "workTier": "standard" },
            { "id": "codex-gpt-6-astra-high", "kind": "codex", "model": "gpt-6-astra", "effort": "high", "workTier": "complex" },
            { "id": "my-pi", "kind": "pi", "model": "anthropic/claude-opus-5-5", "thinking": "low", "workTier": "critical" },
            { "id": "zeus", "kind": "claude-code", "model": "claude-sonnet-5-5", "effort": "low", "workTier": "critical" },
            { "id": "my-devin", "kind": "devin", "model": "claude-opus-5-5", "effort": "max", "workTier": "standard" },
        ],
        "preferences": { "ownHarnessOnly": false },
    })
}

/// A participant as the custom-agent tests read it: id, handle, model, effort, model's
/// name, whether it needs an edit.
type RunsOn = (
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    bool,
);

/// Each participant as the tests read it.
fn runs_on(db: &Connection) -> Vec<RunsOn> {
    db.prepare(
        "SELECT id, handle, model, effort, model_name, needs_edit FROM participant ORDER BY id",
    )
    .unwrap()
    .query_map([], |row| {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get::<_, i64>(5)? == 1,
        ))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

#[test]
fn migration_0031_runs_each_agent_of_the_human_s_own_on_what_the_agents_file_saved_for_it() {
    let (_dir, db) = custom_0030(Some(their_agents()));
    migrate(&db, &MIGRATIONS).unwrap();
    let s = |text: &str| Some(text.to_owned());
    assert_eq!(
        runs_on(&db),
        [
            (1, "human".to_owned(), None, None, None, false),
            // The chief keeps the model and effort its agent saved.
            (
                2,
                "chief".to_owned(),
                s("claude-sonnet-5-5"),
                s("high"),
                None,
                false
            ),
            (
                3,
                "gpt-6-1-sol-ultra".to_owned(),
                s("gpt-6.1-sol"),
                s("ultra"),
                None,
                false
            ),
            // Add member's agent for a model: the effort it saved beside the model.
            (
                4,
                "gpt-6-astra-high".to_owned(),
                s("gpt-6-astra"),
                s("high"),
                None,
                false
            ),
            // Pi's thinking is the effort it runs at.
            (
                5,
                "anthropic-claude-opus-5-5-low".to_owned(),
                s("anthropic/claude-opus-5-5"),
                s("low"),
                None,
                false
            ),
            // A ready-made agent the human edited runs on the edit, not the catalog's.
            (
                6,
                "claude-sonnet-5-5-low".to_owned(),
                s("claude-sonnet-5-5"),
                s("low"),
                None,
                false
            ),
            // An agent no file and no catalog knows is kept as it was, and needs an edit.
            (7, "ghost".to_owned(), None, None, None, true),
            (
                8,
                "gpt-6-1-sol-ultra-amber-pine".to_owned(),
                s("gpt-6.1-sol"),
                s("ultra"),
                None,
                false
            ),
            (9, "ghost-brisk-birch".to_owned(), None, None, None, true),
            // Devin's agent named the model and its effort apart, and its launch joined them.
            (
                10,
                "claude-opus-5-5-max".to_owned(),
                s("claude-opus-5-5-max"),
                s("max"),
                None,
                false
            ),
        ]
    );
}

#[test]
fn migration_0031_keeps_an_agent_it_cannot_read_as_it_was_and_turns_none_into_a_default() {
    // No agents file beside the ledger: the human's own agents are read nowhere.
    let (_dir, db) = custom_0030(None);
    migrate(&db, &MIGRATIONS).unwrap();
    let s = |text: &str| Some(text.to_owned());
    let rows = runs_on(&db);
    let row = |id: i64| rows.iter().find(|row| row.0 == id).unwrap().clone();
    for id in [3, 5, 7, 8, 9, 10] {
        let (_, handle, model, effort, _, needs_edit) = row(id);
        assert!(needs_edit, "{handle} needs an edit");
        assert_eq!((model, effort), (None, None), "{handle} runs on no default");
        assert!(!handle.contains("default"), "{handle} keeps its name");
    }
    assert_eq!(row(3).1, "my-coder");
    // T-1143's member holds its model, but not the effort only its agent said: it needs one.
    assert_eq!(
        row(4),
        (
            4,
            "codex-gpt-6-astra-high".to_owned(),
            s("gpt-6-astra"),
            None,
            None,
            true
        )
    );
    // A ready-made agent runs on the catalog's, as it ran.
    assert_eq!(
        row(6),
        (
            6,
            "opus-max".to_owned(),
            s("claude-opus-5-5"),
            s("max"),
            s("Opus"),
            false
        )
    );
    // The chief runs on what its harness's record says it ran on last (CHF-05): it is marked
    // for Switch chief to pick a model for it, and keeps its row as it was.
    assert_eq!(row(2), (2, "chief".to_owned(), None, None, None, true));
}

#[test]
fn a_member_that_needs_an_edit_is_shown_on_no_model_never_its_harness_s_default() {
    let (dir, db) = custom_0030(None);
    migrate(&db, &MIGRATIONS).unwrap();
    drop(db);
    let ledger = crate::open_ledger(
        &dir.path().join("consensflow.db"),
        crate::Options::default(),
    )
    .unwrap();
    let project = ledger.project(1).unwrap().unwrap();
    let shown = |handle: &str| {
        crate::handles::runs_on(
            project
                .participants
                .iter()
                .find(|participant| participant.handle == handle)
                .unwrap(),
        )
    };
    assert_eq!(shown("ghost"), NEEDS_AN_EDIT);
    assert_eq!(shown("opus-max"), "Opus · max");
    assert!(project
        .participants
        .iter()
        .any(|participant| participant.needs_edit));
}
