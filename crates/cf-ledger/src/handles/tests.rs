use rusqlite::Connection;

use super::*;
use crate::schema::{migrate, MIGRATIONS};

fn none_taken(_: &str) -> bool {
    false
}

#[test]
fn a_handle_is_the_model_s_id_and_its_effort_in_lowercase_words() {
    for (harness, model, effort, handle) in [
        (
            "claude-code",
            "claude-opus-5-5",
            Some("high"),
            "claude-opus-5-5-high",
        ),
        ("codex", "gpt-6-astra", Some("max"), "gpt-6-astra-max"),
        ("codex", "gpt-6.1-sol", Some("xhigh"), "gpt-6-1-sol-xhigh"),
        ("codex", "gpt-5.6-luna", Some("low"), "gpt-5-6-luna-low"),
        ("devin", "swe-2", Some("max"), "swe-2-max"),
        (
            "devin",
            "swe-2-lightning",
            Some("max"),
            "swe-2-lightning-max",
        ),
        (
            "devin",
            "gpt-6-astra-priority",
            Some("high"),
            "gpt-6-astra-priority-high",
        ),
        ("devin", "adaptive", None, "adaptive"),
        (
            "muse",
            "muse-spark-1.3",
            Some("high"),
            "muse-spark-1-3-high",
        ),
        // Pi's name leaves its provider out; an OpenRouter id holds a `/` of its own.
        (
            "pi",
            "anthropic/claude-opus-5-5",
            Some("high"),
            "claude-opus-5-5-high",
        ),
        (
            "pi",
            "openrouter/anthropic/claude-fable-5.1",
            Some("low"),
            "anthropic-claude-fable-5-1-low",
        ),
        (
            "pi",
            "meta/muse-spark-1.2-contributor",
            Some("xhigh"),
            "muse-spark-1-2-contributor-xhigh",
        ),
        // Whatever is no letter, digit or hyphen is a hyphen, in a run one.
        ("codex", "Model_v2:β (new)", None, "model-v2-new"),
        ("codex", "Codex default", None, "codex-default"),
        ("codex", "✓", None, "member"),
    ] {
        assert_eq!(
            model_handle(model, effort, harness, none_taken),
            handle,
            "{harness} {model} {effort:?}"
        );
    }
}

#[test]
fn a_name_that_is_taken_puts_the_provider_of_a_pi_model_in_front_then_the_harness_then_a_number() {
    let taken = [
        "claude-opus-5-5-high",
        "anthropic-claude-opus-5-5-high",
        "pi-anthropic-claude-opus-5-5-high",
    ];
    let is_taken = |h: &str| taken.contains(&h);
    // Another harness: its own in front.
    assert_eq!(
        model_handle("claude-opus-5-5", Some("high"), "devin", is_taken),
        "devin-claude-opus-5-5-high"
    );
    // Pi: its provider first, where two names would collide.
    assert_eq!(
        model_handle("openrouter/claude-opus-5-5", Some("high"), "pi", is_taken),
        "openrouter-claude-opus-5-5-high"
    );
    // Then its harness before the provider, then a number.
    assert_eq!(
        model_handle("anthropic/claude-opus-5-5", Some("high"), "pi", is_taken),
        "pi-anthropic-claude-opus-5-5-high-2"
    );
    // A name that says its effort already says it once (a Devin launch id kept whole).
    assert_eq!(
        model_handle("claude-opus-5-5-max", Some("max"), "devin", none_taken),
        "claude-opus-5-5-max"
    );
    // A Pi model with no provider is only an id.
    assert_eq!(
        model_handle("local-model", Some("low"), "pi", |h| h == "local-model-low"),
        "pi-local-model-low"
    );
}

#[test]
fn a_handle_is_short_enough_for_its_windows_and_ends_on_a_word() {
    let long = "a-very-long-model-id-that-some-harness-may-send-one-day-123456789";
    let handle = model_handle(long, Some("high"), "pi", none_taken);
    assert!(handle.len() <= MEMBER_HANDLE_MAX, "{handle}");
    assert!(!handle.ends_with('-'), "{handle}");
}

/// A name holds its model when its id and effort stand in it as words: whatever is in front
/// (a harness, a provider) or after (a number, a window's two words) does not matter.
#[test]
fn a_name_holds_its_model_where_the_id_and_effort_stand_in_it_as_words() {
    let holds = |handle: &str, model: Option<&str>, _harness: &str, effort: Option<&str>| {
        names_its_model(handle, model, model, effort)
    };
    let opus = Some("claude-opus-5-5");
    assert!(holds(
        "claude-opus-5-5-high",
        opus,
        "claude-code",
        Some("high")
    ));
    assert!(holds(
        "devin-claude-opus-5-5-high",
        opus,
        "devin",
        Some("high")
    ));
    assert!(holds(
        "devin-claude-opus-5-5-high-2",
        opus,
        "devin",
        Some("high")
    ));
    assert!(holds(
        "claude-opus-5-5-high-amber-pine",
        opus,
        "claude-code",
        Some("high")
    ));
    assert!(holds(
        "anthropic-claude-opus-5-5-high",
        Some("anthropic/claude-opus-5-5"),
        "pi",
        Some("high")
    ));
    assert!(holds("adaptive", Some("adaptive"), "devin", None));
    // A name from before, or of another effort or model, holds nothing of it.
    assert!(!holds("opus-high", opus, "claude-code", Some("high")));
    assert!(!holds(
        "claude-opus-5-5-high",
        opus,
        "claude-code",
        Some("max")
    ));
    assert!(!holds(
        "claude-opus-5-5-high",
        Some("gpt-6.1-sol"),
        "codex",
        Some("high")
    ));
    // No model, no name for one to hold.
    assert!(!names_its_model("codex-default", None, None, None));
}

/// The listed id of a row that holds a launch id: from the home's list where it holds it, else
/// with the effort's word taken out of it where what is left reads as the row's label; a row
/// that holds the id its list names, or that nothing says holds another, is taken as it is.
#[test]
fn a_row_s_listed_id_is_read_from_the_list_else_from_its_launch_id_and_its_label() {
    let list: ModelsList = vec![
        (
            "swe-2-lightning".to_owned(),
            vec![
                "swe-2-high-lightning".to_owned(),
                "swe-2-max-lightning".to_owned(),
            ],
        ),
        ("adaptive".to_owned(), vec![]),
        ("twin-a".to_owned(), vec!["shared-high".to_owned()]),
        ("twin-b".to_owned(), vec!["shared-high".to_owned()]),
    ];
    let listed = |model: &str, effort: Option<&str>, label: Option<&str>| {
        listed_model(model, effort, label, &list)
    };
    assert_eq!(
        listed("swe-2-max-lightning", Some("max"), None),
        ("swe-2-lightning".to_owned(), Listed::FromList)
    );
    // A row that holds a listed id is taken as it is.
    assert_eq!(
        listed("adaptive", None, None),
        ("adaptive".to_owned(), Listed::AsTaken)
    );
    // The list has two models with that launch id: the launch id stands for it.
    assert_eq!(
        listed("shared-high", Some("high"), Some("Shared")),
        ("shared-high".to_owned(), Listed::LaunchId)
    );
    // Not in the list: the effort's word, once among the launch id's words, goes where what is
    // left is the row's label.
    assert_eq!(
        listed(
            "claude-opus-5-5-high-fast",
            Some("high"),
            Some("Claude Opus 5.5 Fast")
        ),
        ("claude-opus-5-5-fast".to_owned(), Listed::ByLabel)
    );
    assert_eq!(
        listed("swe-2-max", Some("max"), Some("SWE-2")),
        ("swe-2".to_owned(), Listed::ByLabel)
    );
    // A label that does not read so, no label, no effort, the word twice or not at all: as taken.
    for (model, effort, label) in [
        ("claude-opus-5-5-high-fast", Some("high"), Some("Opus")),
        ("claude-opus-5-5-high-fast", Some("high"), None),
        ("max-model-max", Some("max"), Some("Model")),
        ("kimi-k3", Some("high"), Some("Kimi K3")),
        ("kimi-k3", None, Some("Kimi K3")),
        // A Pi model whose id ends with the word of the effort it runs at is that id.
        (
            "openrouter/openai/o3-mini-high",
            Some("high"),
            Some("OpenAI: o3 Mini High"),
        ),
    ] {
        assert_eq!(
            listed(model, effort, label),
            (model.to_owned(), Listed::AsTaken),
            "{model}"
        );
    }
}

/// This Mac's Devin list as it was recorded on 2026-11-06 (68 models, 246 efforts that have a
/// launch id, 75 of them with the effort before a suffix): every launch id of it gives its
/// listed id from the list; with no list, the label-verified rule gives it for the 165 whose
/// label reads as the id once the effort's word is out, never a wrong one, and takes the other
/// 81 (a `priority` model that is called Fast, `glm-5-2` that is called High) as they are.
#[test]
fn every_launch_id_of_devin_s_recorded_list_gives_its_listed_id_from_the_list_and_none_a_wrong_one_without(
) {
    let recorded: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/devin-models-2026-11-06.json"
    ))
    .unwrap();
    let list: ModelsList = recorded["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|model| {
            (
                model["id"].as_str().unwrap().to_owned(),
                model["launchIds"]
                    .as_object()
                    .into_iter()
                    .flat_map(|ids| ids.values())
                    .map(|launch| launch.as_str().unwrap().to_owned())
                    .collect(),
            )
        })
        .collect();
    let (mut pairs, mut odd, mut by_label) = (0, 0, 0);
    for model in recorded["models"].as_array().unwrap() {
        let id = model["id"].as_str().unwrap();
        let label = model["label"].as_str();
        for (effort, launch) in model["launchIds"].as_object().into_iter().flatten() {
            let launch = launch.as_str().unwrap();
            pairs += 1;
            odd += usize::from(launch != format!("{id}-{effort}"));
            assert_eq!(
                listed_model(launch, Some(effort), label, &list),
                (id.to_owned(), Listed::FromList),
                "{launch}"
            );
            match listed_model(launch, Some(effort), label, &Vec::new()) {
                (found, Listed::ByLabel) => {
                    assert_eq!(found, id, "{launch} without the list");
                    by_label += 1;
                }
                (found, how) => assert_eq!((found.as_str(), how), (launch, Listed::AsTaken)),
            }
        }
    }
    assert_eq!((pairs, odd, by_label), (246, 75, 165));
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
    migrate(&db, &MIGRATIONS[..32]).unwrap();
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
    migrate(&db, &MIGRATIONS[..32]).unwrap();
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
    migrate(&db, &MIGRATIONS[..32]).unwrap();
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
    migrate(&db, &MIGRATIONS[..32]).unwrap();
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
    migrate(&db, &MIGRATIONS[..32]).unwrap();
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
    // Opened, the ledger is at 0033: a member is named by its model's id, and shown by it.
    assert_eq!(shown("claude-opus-5-5-max"), "claude-opus-5-5 · max");
    assert!(project
        .participants
        .iter()
        .any(|participant| participant.needs_edit));
}

/// A ledger 3.0.7-alpha.2 left (schema 32), as a board of it looks: members named by the
/// name their harness's list gave their model, one window each of two of them, the image
/// designer on Codex's default, a member that needs an edit, Devin's launch ids, a Pi member
/// whose name will collide, and a task and a message whose authors are windows.
fn alpha_2() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    migrate(&db, &MIGRATIONS[..32]).unwrap();
    db.execute_batch(
        "INSERT INTO project (id, directory, name, state, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'open', 't', 't'), (2, '/work/other', 'other', 'open', 't', 't');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, model, effort, model_name, tier, member_id, created_at, left_at, designer, needs_edit) VALUES
           (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, NULL, NULL, NULL, NULL, 't', NULL, 0, 0),
           (2, 1, 'chief', 'chief', '[]', NULL, 'claude-code', 'claude-opus-5-5', 'xhigh', 'Opus', NULL, NULL, 't', NULL, 0, 0),
           (3, 1, 'opus-high', 'worker', '[\"worker\"]', 'poseidon', 'claude-code', 'claude-opus-5-5', 'high', 'Opus', 'complex', NULL, 't', NULL, 0, 0),
           (4, 1, 'opus-high-frosty-cliff', 'worker', '[\"worker\"]', 'poseidon', 'claude-code', 'claude-opus-5-5', 'high', 'Opus', 'complex', 3, 't', NULL, 0, 0),
           (5, 1, 'gpt-6-astra-xhigh', 'advisor', '[\"advisor\"]', 'asteria', 'codex', 'gpt-6-astra', 'xhigh', 'GPT-6-Astra', 'critical', NULL, 't', NULL, 0, 0),
           (6, 1, 'swe-2-lightning-max', 'worker', '[\"worker\"]', NULL, 'devin', 'swe-2-max-lightning', 'max', 'SWE-2 Lightning', 'light', NULL, 't', NULL, 0, 0),
           (7, 1, 'claude-opus-5-5-fast-high', 'tester', '[\"tester\"]', NULL, 'devin', 'claude-opus-5-5-high-fast', 'high', 'Claude Opus 5.5 Fast', 'standard', NULL, 't', 't', 0, 0),
           (8, 1, 'codex-default', 'designer', '[\"designer\"]', 'pygmalion', 'codex', NULL, NULL, NULL, 'light', NULL, 't', NULL, 1, 0),
           (9, 1, 'buri', 'worker', '[\"worker\"]', 'buri', 'hermes-code', NULL, NULL, NULL, 'light', NULL, 't', NULL, 0, 1),
           (10, 1, 'claude-opus-5-5-high', 'reviewer', '[\"reviewer\"]', NULL, 'pi', 'anthropic/claude-opus-5-5', 'high', 'Claude Opus 5.5', 'complex', NULL, 't', NULL, 0, 0),
           (11, 2, 'human', 'human', '[]', NULL, NULL, NULL, NULL, NULL, NULL, NULL, 't', NULL, 0, 0),
           (12, 2, 'opus-high', 'worker', '[\"worker\"]', NULL, 'claude-code', 'claude-opus-5-5', 'high', 'Opus', 'complex', NULL, 't', NULL, 0, 0);
         INSERT INTO task (id, project_id, number, title, body, requester_id, assignee_id, state, created_at, updated_at)
           VALUES (1, 1, 1, 'Fix', 'Fix it', 2, 4, 'working', 't', 't');
         INSERT INTO message (id, project_id, recipient_id, sender_id, kind, task_id, body, state, created_at)
           VALUES (1, 1, 2, 4, 'result', 1, 'Done', 'queued', 't');
         INSERT INTO model_list (harness, models, read_at, how) VALUES
           ('devin', '[{\"id\":\"swe-2-lightning\",\"label\":\"SWE-2 Lightning\",\"efforts\":[\"high\",\"max\"],\"launchIds\":{\"high\":\"swe-2-high-lightning\",\"max\":\"swe-2-max-lightning\"}}]', 1, 'devin models list');",
    )
    .unwrap();
    db
}

/// What the rename left of each participant: id, handle, model's name, the names it left.
type Named = (i64, String, Option<String>, Vec<String>);

fn named(db: &Connection) -> Vec<Named> {
    db.prepare("SELECT id, handle, model_name, former_handles FROM participant ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            let former: String = row.get(3)?;
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                serde_json::from_str(&former).unwrap(),
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn migration_0033_names_every_member_with_a_model_by_its_id_and_its_windows_follow() {
    let db = alpha_2();
    migrate(&db, &MIGRATIONS).unwrap();
    let s = |text: &str| Some(text.to_owned());
    let was = |names: &[&str]| {
        names
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        named(&db),
        [
            (1, "human".to_owned(), None, vec![]),
            // The chief keeps its handle, and its row's model's name as it was.
            (2, "chief".to_owned(), s("Opus"), vec![]),
            (
                3,
                "claude-opus-5-5-high".to_owned(),
                s("claude-opus-5-5"),
                was(&["opus-high"])
            ),
            // Its window: its member's new name and its own two words; the name it left kept.
            (
                4,
                "claude-opus-5-5-high-frosty-cliff".to_owned(),
                s("claude-opus-5-5"),
                was(&["opus-high-frosty-cliff"])
            ),
            (
                5,
                "gpt-6-astra-xhigh".to_owned(),
                s("gpt-6-astra"),
                was(&[])
            ),
            // Devin's launch id (the effort before the suffix) gives the listed id from the list.
            (
                6,
                "swe-2-lightning-max".to_owned(),
                s("swe-2-lightning"),
                was(&[])
            ),
            // A member that left, and a launch id the list does not have: the effort's word goes.
            (
                7,
                "claude-opus-5-5-fast-high".to_owned(),
                s("claude-opus-5-5-fast"),
                was(&[])
            ),
            // No model, or an edit needed: the name stays until it has one.
            (8, "codex-default".to_owned(), None, vec![]),
            (9, "buri".to_owned(), None, vec![]),
            // Pi's name leaves its provider out; the name is taken, so the provider goes in front.
            (
                10,
                "anthropic-claude-opus-5-5-high".to_owned(),
                s("anthropic/claude-opus-5-5"),
                was(&["claude-opus-5-5-high"])
            ),
            (11, "human".to_owned(), None, vec![]),
            // The same model in another project is named the same: names are a project's.
            (
                12,
                "claude-opus-5-5-high".to_owned(),
                s("claude-opus-5-5"),
                was(&["opus-high"])
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
    assert_eq!((assignee, sender), (4, 4));
}

#[test]
fn migration_0033_counts_what_it_renamed_and_how_each_devin_id_was_found() {
    let db = alpha_2();
    migrate(&db, &MIGRATIONS[..32]).unwrap();
    db.execute_batch("BEGIN").unwrap();
    db.execute_batch(MIGRATIONS[32]).unwrap();
    let done = name_members_by_id(&db).unwrap();
    db.execute_batch("COMMIT").unwrap();
    assert_eq!(
        done,
        Renamed {
            members: 6,
            changed: 3,
            windows: 1,
            by_list: 1,
            by_label: 1,
            by_launch_id: 0
        }
    );
}

/// A ledger of alpha.1's shape goes through 0031 (the name its harness's list gave the
/// model) and 0033 (its id): history keeps the nickname in `agent` and the name between in
/// `former_handles`, in the order the member went by them.
#[test]
fn an_alpha_1_ledger_goes_by_the_names_of_both_migrations_and_keeps_both_earlier_ones() {
    let db = before_0031();
    migrate(&db, &MIGRATIONS).unwrap();
    let row = |id: i64| -> (String, Option<String>, Vec<String>) {
        db.query_row(
            "SELECT handle, agent, former_handles FROM participant WHERE id = ?",
            [id],
            |row| {
                let former: String = row.get(2)?;
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    serde_json::from_str(&former).unwrap(),
                ))
            },
        )
        .unwrap()
    };
    // @poseidon, then @opus-high, now @claude-opus-5-5-high; its window the same.
    assert_eq!(
        row(3),
        (
            "claude-opus-5-5-high".to_owned(),
            Some("poseidon".to_owned()),
            vec!["opus-high".to_owned()]
        )
    );
    assert_eq!(
        row(8),
        (
            "claude-opus-5-5-high-frosty-cliff".to_owned(),
            Some("poseidon".to_owned()),
            vec!["opus-high-frosty-cliff".to_owned()]
        )
    );
    // Devin's `swe-2-max` is the listed `swe-2` at max, and its name was already that.
    assert_eq!(row(5).0, "swe-2-max");
    assert_eq!(row(5).2, Vec::<String>::new());
    // The designer and the member that needs an edit keep the names they had.
    assert_eq!(row(4).0, "codex-default");
    assert_eq!(row(6).0, "buri");
}
