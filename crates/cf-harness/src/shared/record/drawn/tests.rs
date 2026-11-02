use serde_json::json;

use super::*;

fn item(id: &str) -> Item {
    Item {
        id: Arc::from(id),
        role: Role::Assistant,
        text: Arc::from(id),
        complete: true,
        at: None,
        commentary: false,
        synthetic: false,
        compaction: false,
        detail: None,
    }
}

fn ids(timeline: Timeline<'_>, from: usize) -> Vec<(String, bool)> {
    timeline
        .from(from)
        .map(|(item, beside)| (item.id.to_string(), beside))
        .collect()
}

#[test]
fn a_header_says_a_call_in_one_line_of_at_most_256_characters() {
    assert_eq!(&*header(Action::Shell("cargo test")), "$ cargo test");
    assert_eq!(
        &*header(Action::Read {
            path: "src/a.rs",
            from: Some(10),
            lines: Some(5)
        }),
        "read src/a.rs:10-14"
    );
    assert_eq!(
        &*header(Action::Read {
            path: "a",
            from: None,
            lines: None
        }),
        "read a"
    );
    assert_eq!(&*header(Action::Edit("a.rs")), "edit a.rs");
    assert_eq!(&*header(Action::Write("b.md")), "write b.md");
    assert_eq!(
        &*header(Action::Other {
            name: "Grep",
            about: Some("fn main")
        }),
        "Grep fn main"
    );
    assert_eq!(
        &*header(Action::Other {
            name: "advisor",
            about: Some("  ")
        }),
        "advisor"
    );
    // A command of two lines says its first, and that there is more.
    assert_eq!(&*header(Action::Shell("cd a\nmake")), "$ cd a…");
    let long = "x".repeat(400);
    let said = header(Action::Shell(&long));
    assert_eq!(said.chars().count(), HEADER_MAX);
    assert!(said.ends_with('…'));
    // Characters, not bytes.
    let wide = "é".repeat(300);
    assert_eq!(header(Action::Write(&wide)).chars().count(), HEADER_MAX);
}

#[test]
fn took_is_the_time_between_two_times_and_none_backwards() {
    assert_eq!(took(Some(1_000), Some(2_500)), Some(1_500));
    assert_eq!(took(Some(2_500), Some(1_000)), None);
    assert_eq!(took(None, Some(1)), None);
}

#[test]
fn an_items_kind_is_its_details_else_its_roles() {
    let mut said = item("a");
    assert_eq!(said.kind(), Kind::Text);
    said.role = Role::User;
    assert_eq!(said.kind(), Kind::Prompt);
    said.compaction = true;
    assert_eq!(said.kind(), Kind::Compaction);
    said.compaction = false;
    said.role = Role::Custom;
    assert_eq!(said.kind(), Kind::Notice);
    said.role = Role::Tool;
    assert_eq!(said.kind(), Kind::Tool);
    said.detail = Some(Arc::new(Detail::of(Kind::Thinking)));
    assert_eq!(said.kind(), Kind::Thinking);
}

#[test]
fn an_items_detail_is_never_said_in_its_reading() {
    let mut said = item("a");
    let plain = serde_json::to_value(&said).unwrap();
    said.detail = Some(Arc::new(Detail {
        header: Some(Arc::from("$ ls")),
        status: Some(Status::Failed),
        took_ms: Some(3),
        ..Detail::of(Kind::Tool)
    }));
    assert_eq!(serde_json::to_value(&said).unwrap(), plain);
}

#[test]
fn the_items_beside_are_never_said_in_a_records_reading() {
    let mut record = Record::new();
    record.items = vec![item("a")];
    let plain = serde_json::to_value(&record).unwrap();
    record.beside = vec![(
        0,
        beside_item(
            Arc::from("t"),
            Role::Custom,
            "hm",
            None,
            Detail::of(Kind::Thinking),
        ),
    )];
    assert_eq!(serde_json::to_value(&record).unwrap(), plain);
}

#[test]
fn a_timeline_merges_the_items_beside_where_they_stand_from_any_place() {
    // The timeline t0 a b t1 t2 c t3: each item beside by the place among the items it
    // stands before.
    let items = vec![item("a"), item("b"), item("c")];
    let beside = vec![
        (0, item("t0")),
        (2, item("t1")),
        (2, item("t2")),
        (3, item("t3")),
    ];
    let mut record = Record::new();
    record.items = items;
    record.beside = beside;
    let timeline = Timeline::of(&record);
    assert_eq!(timeline.len(), 7);
    let whole: Vec<&str> = ["t0", "a", "b", "t1", "t2", "c", "t3"].into();
    for from in 0..=7 {
        let read = ids(timeline, from);
        let expected: Vec<(String, bool)> = whole[from..]
            .iter()
            .map(|id| (id.to_string(), id.starts_with('t')))
            .collect();
        assert_eq!(read, expected, "from {from}");
    }
    assert!(ids(timeline, 9).is_empty());
    assert!(Timeline::none().is_empty());
}

#[test]
fn a_beside_item_is_complete_and_a_compactions_is_marked_one() {
    let thought = beside_item(
        Arc::from("t"),
        Role::Custom,
        "hm",
        Some(json!("2026-11-02T00:00:00Z")),
        Detail::of(Kind::Thinking),
    );
    assert!(thought.complete && !thought.compaction);
    assert_eq!(thought.kind(), Kind::Thinking);
    let compacted = beside_item(
        Arc::from("c"),
        Role::Custom,
        "",
        None,
        Detail::of(Kind::Compaction),
    );
    assert!(compacted.compaction);
}

#[test]
fn a_call_to_a_tool_this_module_does_not_know_is_headed_by_what_it_was_given() {
    let guessed = |name: &str, arguments: Value| guess(name, Some(&arguments)).to_string();
    assert_eq!(
        guessed("exec_command", json!({ "cmd": "ls nope", "workdir": "/w" })),
        "$ ls nope"
    );
    assert_eq!(
        guessed("shell", json!({ "command": ["bash", "-lc", "cargo test"] })),
        "$ cargo test"
    );
    assert_eq!(
        guessed("shell", json!({ "command": ["git", "status"] })),
        "$ git status"
    );
    // Arguments as JSON text, as Codex writes them.
    assert_eq!(guessed("exec_command", json!("{\"cmd\":\"pwd\"}")), "$ pwd");
    assert_eq!(
        guessed(
            "read_file",
            json!({ "path": "a.md", "start_line": 3, "end_line": 9 })
        ),
        "read a.md:3-9"
    );
    assert_eq!(guessed("view", json!({ "file_path": "a.md" })), "read a.md");
    assert_eq!(
        guessed("str_replace_edit", json!({ "path": "a.rs" })),
        "edit a.rs"
    );
    assert_eq!(
        guessed("apply_patch", json!({ "path": "a.rs" })),
        "edit a.rs"
    );
    assert_eq!(
        guessed("write_file", json!({ "file_path": "b.md", "content": "x" })),
        "write b.md"
    );
    assert_eq!(
        guessed("grep", json!({ "pattern": "fn main", "path": "src" })),
        "grep fn main"
    );
    assert_eq!(
        guessed("web_search", json!({ "query": "rust" })),
        "web_search rust"
    );
    assert_eq!(
        guessed("list_dir", json!({ "path": "src" })),
        "list_dir src"
    );
    assert_eq!(guess("todo", None).to_string(), "todo");
    assert_eq!(guessed("todo", json!("not json")), "todo");
}
