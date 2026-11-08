//! `cf` run in Codex's memories folder changes nothing on the board (T-1313): that folder is the
//! working folder of Codex's memory job, a thread of the window's own Codex server that is given
//! the window's role and token (the issue `codex-memories-post-to-the-board-as-the-chief`, found
//! on the `sites` board, 2026-10-21). What reads the board, `cf hook` and a command's usage are
//! not changed by it.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_board::scripted::{reply, scripted, ScriptedApi};
use serde_json::{json, Value};

use super::{run_in, IN_CODEX_MEMORIES};

/// The words, which a Codex memory job's model reads: held as the brief for the guard gave
/// them.
const WORDS: &str =
    "This command ran in Codex's memories folder: Codex's memory job isn't on the board.";

/// A window of a board: its Codex home in a folder of its own, which has the memories folder
/// (and the project folder, a window's working folder).
struct Window {
    root: tempfile::TempDir,
    api: ScriptedApi,
    env: Env,
}

impl Window {
    fn new(replies: Vec<cf_board::scripted::Reply>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let real = fs::canonicalize(root.path()).unwrap();
        for folder in [".codex/memories/rollout_summaries", "project"] {
            fs::create_dir_all(real.join(folder)).unwrap();
        }
        let api = scripted(replies);
        let env = Env::from_vars([
            ("CONSENSFLOW_URL", api.url.as_str()),
            ("CONSENSFLOW_TOKEN", "tok"),
            ("CODEX_HOME", real.join(".codex").to_str().unwrap()),
        ]);
        Self { root, api, env }
    }

    fn at(&self, folder: &str) -> PathBuf {
        fs::canonicalize(self.root.path()).unwrap().join(folder)
    }

    fn memories(&self) -> PathBuf {
        self.at(".codex/memories")
    }

    /// The command `words` as run with `folder` as its working folder: what it exited with, and
    /// said on each stream.
    fn run(&self, folder: Option<&Path>, words: &[&str]) -> (u8, String, String) {
        let args: Vec<OsString> = words.iter().map(OsString::from).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_in(
            folder,
            &self.env,
            &args,
            &mut io::empty(),
            &mut out,
            &mut err,
        )
        .unwrap();
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    /// The requests the board was asked, as `METHOD path`.
    fn asked(&self) -> Vec<String> {
        self.api
            .received()
            .into_iter()
            .map(|received| format!("{} {}", received.method, received.path))
            .collect()
    }
}

/// Every command that changes the board, written right: the board's write verbs, `cf task`'s
/// moves among them.
const WRITES: [&[&str]; 14] = [
    &[
        "task",
        "add",
        "--tier",
        "standard",
        "Consolidate the memories",
    ],
    &[
        "task", "add", "--self", "--needs", "T-3", "--tier", "standard", "Mine",
    ],
    &["task", "accept", "T-3"],
    &["task", "done", "T-3", "the result"],
    &["task", "finish", "T-3", "the result"],
    &["task", "cancel", "T-3"],
    &["task", "reopen", "T-3", "a follow-up"],
    &["task", "edit", "T-3", "a new brief"],
    &["task", "pause", "T-3"],
    &["task", "resume", "T-3", "go on"],
    &["note", "to the chief"],
    &["ask", "which one?"],
    &["tell", "T-3", "wait"],
    &["answer", "m-12", "this one"],
];

/// The usage of the commands above, which `--help` and `-h` answer.
const HELPS: [&[&str]; 9] = [
    &["--help"],
    &["task", "--help"],
    &["task", "add", "--help"],
    &["task", "accept", "-h"],
    &["task", "finish", "--help"],
    &["note", "--help"],
    &["ask", "-h"],
    &["tell", "--help"],
    &["answer", "--help"],
];

#[test]
fn a_command_that_changes_the_board_is_refused_in_the_memories_folder_in_the_words_and_asks_nothing(
) {
    assert_eq!(IN_CODEX_MEMORIES, WORDS);
    let window = Window::new(vec![reply(200, json!({}))]);
    for folder in [
        window.memories(),
        window.at(".codex/memories/rollout_summaries"),
        // A folder the job has not made yet.
        window.at(".codex/memories/extensions/x"),
    ] {
        for words in WRITES {
            let (code, out, err) = window.run(Some(&folder), words);
            assert_eq!(code, 1, "{words:?} in {}: {err}", folder.display());
            assert_eq!(err, format!("cf: {WORDS}\n"), "{words:?}");
            assert_eq!(out, "", "{words:?}");
        }
    }
    // Not one request that changes anything was sent (the board was asked nothing at all).
    assert_eq!(window.asked(), Vec::<String>::new());
}

#[test]
fn the_same_commands_are_taken_from_the_project_and_from_where_the_system_gave_no_folder() {
    for folder in [Some("project"), Some(""), None] {
        let window = Window::new(vec![reply(
            200,
            json!({ "message": { "id": 1, "recipient": "chief" } }),
        )]);
        let at = folder.map(|folder| window.at(folder));
        let (code, out, err) = window.run(at.as_deref(), &["note", "to the chief"]);
        assert_eq!(code, 0, "{folder:?}: {err}");
        assert!(out.contains("noted to @chief"), "{out}");
        assert_eq!(window.asked(), ["POST /api/notes"], "{folder:?}");
    }
}

#[test]
fn what_reads_the_board_the_usage_of_a_command_and_a_hook_are_allowed_in_the_memories_folder() {
    let window = Window::new(vec![
        reply(
            200,
            json!({ "participant": { "handle": "chief", "role": "chief" }, "project": { "name": "p" }, "task": null }),
        ),
        reply(200, json!({ "members": [] })),
        reply(200, json!({ "messages": [] })),
        reply(200, json!({ "open": [], "lanes": [] })),
    ]);
    let memories = window.memories();
    for words in [&["whoami"][..], &["staff"], &["inbox"], &["task", "list"]] {
        let (code, _, err) = window.run(Some(&memories), words);
        assert_eq!(code, 0, "{words:?}: {err}");
    }
    assert_eq!(
        window.asked(),
        [
            "GET /api/whoami",
            "GET /api/staff",
            "GET /api/inbox",
            "GET /api/tasks"
        ]
    );
    // The usage of each write is its usage, and asks nothing: the guard is no help's.
    for help in HELPS {
        let (code, out, err) = window.run(Some(&memories), help);
        assert_eq!(code, 0, "{help:?}: {err}");
        assert!(out.contains("cf "), "{help:?}: {out}");
    }
    assert_eq!(window.asked().len(), 4, "the usages asked nothing");
    // A hook is no board command: it says what it says, and exits 0, there as elsewhere.
    for harness in ["claude", "devin", "codex", "nothing"] {
        let (inside, said, _) = window.run(Some(&memories), &["hook", harness]);
        let (outside, same, _) = window.run(Some(&window.at("project")), &["hook", harness]);
        assert_eq!((inside, said), (outside, same), "cf hook {harness}");
        assert_eq!(inside, 0, "cf hook {harness}");
    }
}

/// A message that waits to be pasted, as `cf inbox read` prints it whole and acknowledges.
fn waiting_message() -> Value {
    json!({ "message": {
        "id": 12, "kind": "answer", "state": "queued", "taskNumber": 3, "sender": "chief",
        "body": "JSON\nand then YAML",
    } })
}

#[test]
fn an_inbox_read_there_prints_the_message_and_does_not_receive_it_for_the_window() {
    // Outside, printing a message whole receives it, which is a write to the board.
    let outside = Window::new(vec![reply(200, waiting_message()), reply(200, json!({}))]);
    let (code, out, _) = outside.run(Some(&outside.at("project")), &["inbox", "read", "m-12"]);
    assert_eq!(code, 0);
    assert!(out.contains("and then YAML"), "{out}");
    assert_eq!(
        outside.asked(),
        ["GET /api/inbox/12", "POST /api/answers/read"]
    );

    // In the memories folder it is read the same, and received by no one: the message stays
    // for the window it was sent to, which has not read it.
    let inside = Window::new(vec![reply(200, waiting_message()), reply(200, json!({}))]);
    let (code, out, _) = inside.run(Some(&inside.memories()), &["inbox", "read", "m-12"]);
    assert_eq!(code, 0);
    assert!(out.contains("and then YAML"), "{out}");
    assert_eq!(inside.asked(), ["GET /api/inbox/12"]);
}

#[test]
fn a_board_command_of_a_window_with_a_codex_home_elsewhere_runs_in_the_folder_it_always_did() {
    // The window's Codex home is its own: the default `~/.codex/memories` is no job's folder.
    let window = Window::new(vec![reply(
        200,
        json!({ "message": { "id": 1, "recipient": "chief" } }),
    )]);
    let elsewhere = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(elsewhere.path()).unwrap();
    fs::create_dir_all(home.join(".codex/memories")).unwrap();
    let env = Env::from_vars([
        ("CONSENSFLOW_URL", window.api.url.as_str()),
        ("CONSENSFLOW_TOKEN", "tok"),
        ("CODEX_HOME", window.at("project").to_str().unwrap()),
        ("HOME", home.to_str().unwrap()),
    ]);
    let args: Vec<OsString> = ["note", "x"].iter().map(OsString::from).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_in(
        Some(&home.join(".codex/memories")),
        &env,
        &args,
        &mut io::empty(),
        &mut out,
        &mut err,
    )
    .unwrap();
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    assert_eq!(window.asked(), ["POST /api/notes"]);
}
