//! Claude Code's adapter, as `tests/adapter-claude.test.mjs` holds Node's
//! (TEST-BDC-05, IMPL-BDC-06), each case under its sentence: how a Claude
//! window is launched, how a message reaches it, and what Claude's own
//! records say about it. Each test gets a throwaway home, Claude config
//! folder and a stand-in `claude` on PATH.
//!
//! A window comes only from a prepare here, where Node's tests made up a
//! launch bag: a test of a window on a known conversation prepares it as
//! that conversation resumed.

use std::cell::{Cell, RefCell};
use std::fs;
use std::rc::Rc;

use cf_base::env::Env;
use cf_base::path;
use cf_harness::claude::ClaudeAdapter;
use cf_harness::contract::{
    Adapter, Admission, Agent, Held, HostError, Launch, LaunchId, Pane, Prepared, Readiness,
    Waiting, Window,
};
use cf_harness::records::Role;
use serde_json::{json, Map, Value};
use tempfile::TempDir;

use crate::fakes::{done, fake_executable, Answering, Local};

/// The launch's id: a uuid, as the engine mints one (Node's tests took any
/// filename-safe word).
const LAUNCH: &str = "0a1b2c3d-4e5f-4061-8a7b-9c0d1e2f3a4b";
const TASK: &str = "[ConsensFlow m-1 · T-1 · task from @chief]\nWrite the parser";
/// A process id no process has.
const DEAD: u32 = 999_999;

/// A throwaway home, Claude config folder and a stand-in `claude` on PATH.
struct Home {
    _dir: TempDir,
    vars: Vec<(String, String)>,
    executable: String,
}

impl Home {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("cf-claude-adapter-")
            .tempdir()
            .unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let vars: Vec<(String, String)> = [
            ("HOME", "home"),
            ("CONSENSFLOW_HOME", "consensflow"),
            ("CLAUDE_CONFIG_DIR", "claude"),
            ("PATH", "bin"),
        ]
        .into_iter()
        .map(|(name, folder)| (name.to_owned(), path::join(&[&root, folder])))
        .collect();
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        let executable = fake_executable(&dir.path().join("bin").join("claude"));
        Self {
            _dir: dir,
            vars,
            executable: executable.to_string_lossy().into_owned(),
        }
    }

    fn var(&self, name: &str) -> &str {
        let found = self.vars.iter().find(|(held, _)| held == name);
        &found.unwrap().1
    }

    fn env(&self) -> Env {
        Env::from_vars(self.vars.iter().cloned())
    }

    /// An adapter of windows that run with this home's environment.
    fn adapter(&self) -> ClaudeAdapter {
        let env = self.env();
        ClaudeAdapter::new(env.clone(), Rc::new(Local::new(env)))
    }

    /// The transcript of `session`, in Claude's project folder for /work/app.
    fn transcript(&self, session: &str, lines: &[String]) {
        let folder = path::join(&[self.var("CLAUDE_CONFIG_DIR"), "projects", "-work-app"]);
        fs::create_dir_all(&folder).unwrap();
        let file = path::join(&[&folder, &format!("{session}.jsonl")]);
        fs::write(file, lines.concat()).unwrap();
    }

    /// Claude's own status file of the live process `pid`.
    fn status(&self, session: &str, fields: &Value, pid: u32) {
        let folder = path::join(&[self.var("CLAUDE_CONFIG_DIR"), "sessions"]);
        fs::create_dir_all(&folder).unwrap();
        let mut row = Map::new();
        row.insert("pid".to_owned(), json!(pid));
        row.insert("sessionId".to_owned(), json!(session));
        row.insert("kind".to_owned(), json!("interactive"));
        row.extend(fields.as_object().unwrap().clone());
        let file = path::join(&[&folder, &format!("{pid}.json")]);
        fs::write(file, Value::Object(row).to_string()).unwrap();
    }

    /// Where the launch's own files are.
    fn launch_folder(&self) -> String {
        path::join(&[
            self.var("CONSENSFLOW_HOME"),
            "integrations",
            "claude",
            LAUNCH,
        ])
    }
}

/// A launch to prepare: a worker's, unless a test changes it.
struct Request {
    role: &'static str,
    handle: &'static str,
    instructions: &'static str,
    resume: Option<String>,
    message: Option<String>,
    agent: Option<Agent<'static>>,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            role: "worker",
            handle: "zeus",
            instructions: "# ConsensFlow worker\n\nRole text for the test.",
            resume: None,
            message: Some(TASK.to_owned()),
            agent: Some(Agent {
                model: Some("claude-sonnet-5"),
                effort: Some("high"),
                thinking: None,
            }),
        }
    }
}

impl Request {
    /// A chief's launch: no model of its own, no first message.
    fn chief() -> Self {
        Self {
            role: "chief",
            handle: "chief",
            agent: None,
            message: None,
            ..Self::default()
        }
    }

    /// A window on the conversation `session`, opened again.
    fn resumed(session: &str) -> Self {
        Self {
            resume: Some(session.to_owned()),
            message: None,
            ..Self::default()
        }
    }
}

fn prepare(adapter: &ClaudeAdapter, request: &Request) -> Result<Prepared, String> {
    let id = LaunchId::new(LAUNCH).unwrap();
    let launch = Launch {
        id: &id,
        project: 1,
        handle: request.handle,
        role: request.role,
        directory: "/work/app",
        resume: request.resume.as_deref(),
        message: request.message.as_deref(),
        agent: request.agent,
        instructions: request.instructions,
    };
    done(adapter.prepare(&launch))
}

/// The window of a prepared launch.
fn window(adapter: &ClaudeAdapter, request: &Request) -> Rc<dyn Window> {
    prepare(adapter, request).unwrap().window
}

/// One Claude transcript line, in the shape Claude Code writes, at second `n`.
fn record(session: &str, n: u32, fields: &Value) -> String {
    let mut line = Map::new();
    line.insert("sessionId".to_owned(), json!(session));
    line.insert("version".to_owned(), json!("2.1.277"));
    line.insert(
        "timestamp".to_owned(),
        json!(format!("2026-09-19T12:00:{n:02}.000Z")),
    );
    line.insert("uuid".to_owned(), json!(format!("{session}-{n}")));
    line.extend(fields.as_object().unwrap().clone());
    format!("{}\n", Value::Object(line))
}

fn user_line(session: &str, n: u32, text: &str) -> String {
    record(
        session,
        n,
        &json!({ "type": "user", "message": { "role": "user", "content": text } }),
    )
}

fn answer_line(session: &str, n: u32, text: &str) -> String {
    let message = json!({
        "id": format!("{session}-message-{n}"),
        "role": "assistant",
        "content": [{ "type": "text", "text": text }],
        "stop_reason": "end_turn",
    });
    record(
        session,
        n,
        &json!({ "type": "assistant", "message": message }),
    )
}

fn stop_line(session: &str, n: u32) -> String {
    let fields = json!({
        "type": "system",
        "subtype": "stop_hook_summary",
        "preventedContinuation": false,
        "hookCount": 1,
    });
    record(session, n, &fields)
}

fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|&word| word.to_owned()).collect()
}

/// This test's own process: a live one.
fn me() -> u32 {
    std::process::id()
}

/// A live process of the test's own, other than the test, ended with it.
struct Other(std::process::Child);

impl Other {
    #[allow(clippy::disallowed_methods)] // The test starts what it ends.
    fn start() -> Self {
        let child = if cfg!(windows) {
            std::process::Command::new("ping")
                .args(["-n", "60", "127.0.0.1"])
                .stdout(std::process::Stdio::null())
                .spawn()
        } else {
            std::process::Command::new("sleep").arg("60").spawn()
        };
        Self(child.unwrap())
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Other {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn launches_a_fresh_worker_on_its_own_session_id_in_full_permission_mode_with_the_task_last() {
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let session = plan.native_session.clone().unwrap();
    assert!(
        session.len() == 36
            && session
                .bytes()
                .all(|byte| byte == b'-' || byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{session}"
    );
    let settings = path::join(&[&home.launch_folder(), "settings.json"]);
    let roles = path::join(&[&home.launch_folder(), "role"]);
    let skill = path::join(&[
        &roles,
        ".claude",
        "skills",
        "consensflow-worker",
        "SKILL.md",
    ]);
    let mut argv = vec![
        home.executable.clone(),
        "--settings".to_owned(),
        settings.clone(),
    ];
    argv.extend(["--add-dir".to_owned(), roles]);
    argv.extend(["--append-system-prompt-file".to_owned(), skill]);
    argv.extend(words(&[
        "--system-prompt-snapshot",
        "off",
        "--strict-mcp-config",
        "--no-chrome",
        "--session-id",
    ]));
    argv.push(session);
    argv.extend(words(&[
        "--model",
        "claude-sonnet-5",
        "--effort",
        "high",
        "--permission-mode",
        "bypassPermissions",
        TASK,
    ]));
    assert_eq!(plan.argv, argv);
    assert!(plan.env.is_empty(), "cf stays usable inside the window");
    assert_eq!(plan.drop_env, ["ANTHROPIC_API_KEY"]);
    let written: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(written["permissions"]["defaultMode"], "bypassPermissions");
    assert_eq!(written["skipDangerousModePermissionPrompt"], true);
    // Messages from other Claude sessions keep Claude's own approval hold:
    // ConsensFlow pastes its messages, so nothing of its own comes that way.
    assert!(written.get("crossSessionInbound").is_none());
    assert_eq!(
        written["hooks"]["Stop"],
        json!([{ "hooks": [{ "type": "command", "command": "exit 0" }] }])
    );
    assert_eq!(
        written["hooks"]["PreToolUse"],
        json!([{
            "matcher": "AskUserQuestion",
            "hooks": [{ "type": "command", "command": "cf hook claude", "timeout": 3600 }],
        }]),
        "Claude's question tool is answered from the board, for an hour, then in the window"
    );
}

#[test]
fn keeps_a_member_away_from_the_humans_connectors_and_browser_the_chief_keeps_them() {
    let home = Home::new();
    let adapter = home.adapter();
    let member = prepare(&adapter, &Request::default()).unwrap();
    let has = |plan: &Prepared, flag: &str| plan.argv.iter().any(|arg| arg == flag);
    assert!(has(&member, "--strict-mcp-config") && has(&member, "--no-chrome"));
    let chief = prepare(&adapter, &Request::chief()).unwrap();
    assert!(!has(&chief, "--strict-mcp-config") && !has(&chief, "--no-chrome"));
    // The chief asks the human in its own window: Claude's own question
    // dialog, no hook putting it on the board.
    let at = chief
        .argv
        .iter()
        .position(|arg| arg == "--settings")
        .unwrap();
    let settings: Value =
        serde_json::from_str(&fs::read_to_string(&chief.argv[at + 1]).unwrap()).unwrap();
    assert_eq!(settings["hooks"]["PreToolUse"], json!([]));
}

#[test]
fn resumes_a_conversation_on_the_session_it_already_has() {
    let home = Home::new();
    let session = "0f8fad5b-d9cb-469f-a165-70867728950e";
    home.transcript(session, &[user_line(session, 1, "Write the parser")]);
    let plan = prepare(&home.adapter(), &Request::resumed(session)).unwrap();
    assert_eq!(plan.native_session.as_deref(), Some(session));
    assert_eq!(
        plan.argv[11..],
        words(&[
            "--resume",
            session,
            "--model",
            "claude-sonnet-5",
            "--effort",
            "high",
            "--permission-mode",
            "bypassPermissions",
        ])
    );
}

#[test]
fn starts_afresh_under_the_same_id_a_conversation_claude_never_kept_instead_of_resuming_nothing() {
    let home = Home::new();
    // A window opened by hand and lost before anything was said in it:
    // Claude kept no record, and `--resume` would say "No conversation found".
    let session = "6a2f41ac-8c1d-4c55-9b4e-2f1e8a3d9c70";
    let request = Request {
        message: Some("Review T-1".to_owned()),
        ..Request::resumed(session)
    };
    let plan = prepare(&home.adapter(), &request).unwrap();
    assert_eq!(plan.native_session.as_deref(), Some(session));
    assert!(!plan.argv.iter().any(|arg| arg == "--resume"));
    let at = plan
        .argv
        .iter()
        .position(|arg| arg == "--session-id")
        .unwrap();
    assert_eq!(plan.argv[at..at + 2], words(&["--session-id", session]));
    assert_eq!(
        plan.argv.last().map(String::as_str),
        Some("Review T-1"),
        "the brief goes in as its first message"
    );
}

#[test]
fn gives_a_chief_its_role_instructions_and_no_model_of_its_own() {
    let home = Home::new();
    let request = Request {
        instructions: "CHIEF INSTRUCTIONS",
        ..Request::chief()
    };
    let plan = prepare(&home.adapter(), &request).unwrap();
    let at = plan
        .argv
        .iter()
        .position(|arg| arg == "--append-system-prompt-file")
        .unwrap();
    let file = plan.argv[at + 1].replace('\\', "/");
    assert!(
        file.contains(&format!("integrations/claude/{LAUNCH}/role/"))
            && file.ends_with("consensflow-chief/SKILL.md"),
        "{file}"
    );
    assert_eq!(
        fs::read_to_string(&plan.argv[at + 1]).unwrap(),
        "CHIEF INSTRUCTIONS"
    );
    assert!(!plan.argv.iter().any(|arg| arg == "--model"));
}

#[test]
fn refuses_to_launch_when_claude_is_not_installed() {
    let home = Home::new();
    let mut vars = home.vars.clone();
    vars.retain(|(name, _)| name != "PATH");
    vars.push(("PATH".to_owned(), "/nowhere".to_owned()));
    let env = Env::from_vars(vars);
    let adapter = ClaudeAdapter::new(env.clone(), Rc::new(Local::new(env)));
    let Err(refused) = prepare(&adapter, &Request::default()) else {
        panic!("a launch with no claude");
    };
    assert!(refused.contains("claude is not installed"), "{refused}");
}

#[test]
fn reads_the_conversation_from_the_transcript_and_waits_for_claude_itself_to_say_the_window_is_idle(
) {
    let home = Home::new();
    let session = "1b4e28ba-2fa1-41d2-883f-0016d3cca427";
    home.transcript(
        session,
        &[
            user_line(session, 1, TASK),
            answer_line(session, 2, "Parser done"),
            stop_line(session, 3),
        ],
    );
    let window = window(&home.adapter(), &Request::resumed(session));
    // A transcript that settled before this window opened is not the window
    // at its prompt: Claude's own status says when it is.
    let observed = done(window.observe()).unwrap();
    assert!(!observed.settled, "no word from Claude yet");
    home.status(session, &json!({ "status": "idle" }), me());
    let observed = done(window.observe()).unwrap();
    assert!(observed.settled);
    assert_eq!(observed.waiting, None);
    let items: Vec<(Role, &str, bool)> = observed
        .items()
        .iter()
        .map(|item| (item.role, &*item.text, item.complete))
        .collect();
    assert_eq!(
        items,
        [
            (Role::User, TASK, true),
            (Role::Assistant, "Parser done", true)
        ]
    );

    let waiting = json!({ "status": "waiting", "waitingFor": "permission to run a command" });
    home.status(session, &waiting, me());
    let observed = done(window.observe()).unwrap();
    assert!(!observed.settled);
    assert_eq!(
        observed.waiting,
        Some(Waiting {
            reason: Some("permission to run a command".to_owned())
        })
    );

    home.status(session, &json!({ "status": "busy" }), me());
    assert!(!done(window.observe()).unwrap().settled);
}

#[test]
fn counts_a_new_window_with_no_transcript_yet_as_idle_once_claude_says_so() {
    let home = Home::new();
    let session = "e2c56db5-dffb-48d2-b060-d0f5a71096e0";
    let window = window(&home.adapter(), &Request::resumed(session));
    let before = done(window.observe()).unwrap();
    assert!(
        !before.settled && before.items().is_empty(),
        "still starting"
    );
    home.status(session, &json!({ "status": "idle" }), me());
    let after = done(window.observe()).unwrap();
    assert!(after.settled && after.items().is_empty());
}

/// A host whose window can always be read, with no paste on its way.
fn readable() -> Answering<impl Fn(&str) -> Result<Value, HostError>> {
    Answering::new(|op| {
        Ok(if op == "pane.snapshot" {
            json!({ "ok": true, "pasteInFlight": false })
        } else {
            json!({ "ok": false })
        })
    })
}

#[test]
fn follows_the_window_to_the_conversation_a_clear_or_resume_left_it_on() {
    let home = Home::new();
    let first = "1b4e28ba-2fa1-41d2-883f-0016d3cca427";
    let cleared = "7c9e6679-7425-40de-944b-e07fc1f90ae7";
    let host = readable();
    let pane = Pane {
        id: "s1-chief".to_owned(),
        generation: 1,
    };
    home.transcript(
        first,
        &[
            user_line(first, 1, "hello"),
            answer_line(first, 2, "Hi"),
            stop_line(first, 3),
        ],
    );
    let window = window(&home.adapter(), &Request::resumed(first));
    home.status(first, &json!({ "status": "idle" }), me());
    assert!(done(window.observe()).unwrap().settled);
    assert_eq!(done(window.ready(&host, &pane)), Ok(Readiness::Ready));

    // /clear: the window's own Claude process now names another conversation.
    home.status(cleared, &json!({ "status": "idle" }), me());
    let observed = done(window.observe()).unwrap();
    assert_eq!(observed.switched.as_deref(), Some(cleared));
    assert!(!observed.settled);
    let texts: Vec<&str> = observed.items().iter().map(|item| &*item.text).collect();
    assert_eq!(texts, ["hello", "Hi"], "the old conversation's last look");
    assert_eq!(
        done(window.ready(&host, &pane)),
        Ok(Readiness::Held(Held::ShowsAnother))
    );

    // The dispatcher follows the window: the launch names the new conversation.
    window.follow(cleared);
    home.transcript(
        cleared,
        &[
            user_line(cleared, 1, "fresh start"),
            answer_line(cleared, 2, "Ready"),
            stop_line(cleared, 3),
        ],
    );
    let followed = done(window.observe()).unwrap();
    assert_eq!(followed.switched, None);
    assert!(followed.settled);
    let texts: Vec<&str> = followed.items().iter().map(|item| &*item.text).collect();
    assert_eq!(texts, ["fresh start", "Ready"]);
    assert_eq!(done(window.ready(&host, &pane)), Ok(Readiness::Ready));
}

#[test]
fn follows_a_clear_before_the_first_look_by_the_status_file_named_after_the_windows_process() {
    let home = Home::new();
    let adapter = home.adapter();
    let first = "1b4e28ba-2fa1-41d2-883f-0016d3cca427";
    let cleared = "7c9e6679-7425-40de-944b-e07fc1f90ae7";
    // The pane host named the window's process, whose Claude was cleared
    // before ConsensFlow first looked; another Claude still shows the
    // launch's conversation (the human resumed it elsewhere).
    let other = Other::start();
    home.status(cleared, &json!({ "status": "idle" }), me());
    home.status(first, &json!({ "status": "idle" }), other.pid());
    let named = window(&adapter, &Request::resumed(first));
    named.opened(Some(me()));
    let observed = done(named.observe()).unwrap();
    assert_eq!(observed.switched.as_deref(), Some(cleared));
    // Unnamed, the window's process is the first whose file names the
    // launch's conversation, as it always was: here, the wrong one.
    let guessed = done(window(&adapter, &Request::resumed(first)).observe()).unwrap();
    assert_eq!((guessed.switched, guessed.settled), (None, true));
}

#[test]
fn takes_the_first_status_file_naming_the_launchs_conversation_when_none_is_named_after_the_windows_process(
) {
    let home = Home::new();
    let first = "1b4e28ba-2fa1-41d2-883f-0016d3cca427";
    let cleared = "7c9e6679-7425-40de-944b-e07fc1f90ae7";
    // Claude may run as another process than the pane's own child (one it
    // starts in its place): no status file is named after the child.
    let window = window(&home.adapter(), &Request::resumed(first));
    window.opened(Some(DEAD));
    home.status(first, &json!({ "status": "idle" }), me());
    assert!(done(window.observe()).unwrap().settled);
    // That process is the window's from then on: a /clear in it is followed.
    home.status(cleared, &json!({ "status": "idle" }), me());
    let observed = done(window.observe()).unwrap();
    assert_eq!(observed.switched.as_deref(), Some(cleared));
}

#[test]
fn gives_the_window_text_it_can_take_and_leaves_a_paste_the_bridge_lost_uncertain() {
    let home = Home::new();
    let adapter = home.adapter();
    // Node's case began with half a surrogate pair, which it dropped: a Rust
    // text holds none.
    let text = "half  of it, \u{1b}[31mred\u{1b}[0m and 50%\r60%";
    let taken = "half  of it, \u{241b}[31mred\u{241b}[0m and 50%\u{240d}60%";
    let request = Request {
        message: Some(text.to_owned()),
        ..Request::default()
    };
    let plan = prepare(&adapter, &request).unwrap();
    assert_eq!(plan.argv.last().map(String::as_str), Some(taken));
    let answer: RefCell<Result<Value, HostError>> = RefCell::new(Ok(json!({ "ok": true })));
    let host = Answering::new(|_| answer.borrow().clone());
    let pane = Pane {
        id: "s1-zeus".to_owned(),
        generation: 7,
    };
    let deliver = || done(plan.window.deliver(&host, &pane, text));
    assert_eq!(deliver(), Admission::Admitted { queued: false });
    let pasted = host.asked.borrow().last().unwrap().1["body"].clone();
    assert_eq!(pasted, taken);
    let uncertain = |reason: &str| Admission::Uncertain {
        reason: reason.to_owned(),
    };
    // The bridge's own deadline, or its end, after the paste went out.
    *answer.borrow_mut() = Ok(json!({ "ok": false, "error": "deadline" }));
    assert_eq!(deliver(), uncertain("deadline"));
    *answer.borrow_mut() = Err(HostError {
        error: Some("eof".to_owned()),
        message: "eof".to_owned(),
    });
    assert_eq!(deliver(), uncertain("eof"));
    // The host's word: refused before a byte, or failed once bytes went out.
    *answer.borrow_mut() = Ok(json!({
        "ok": false, "admitted": false, "bytesWritten": 0, "error": "stale", "cause": "stale pane",
    }));
    assert_eq!(
        deliver(),
        Admission::Refused {
            reason: "stale pane".to_owned()
        }
    );
    *answer.borrow_mut() = Ok(json!({
        "ok": false, "admitted": null, "error": "uncertain", "cause": "broken pipe",
    }));
    assert_eq!(deliver(), uncertain("broken pipe"));
}

#[test]
fn pastes_a_message_into_the_window_waiting_for_a_paste_on_its_way_and_for_what_the_human_has_not_sent(
) {
    let home = Home::new();
    let paste_in_flight = Cell::new(false);
    let unsent = Cell::new(false);
    let host = Answering::new(|op| {
        Ok(match op {
            "pane.snapshot" => json!({
                "ok": true, "pasteInFlight": paste_in_flight.get(), "unsent": unsent.get(),
            }),
            "pane.write_paste" => json!({ "ok": true }),
            _ => json!({ "ok": false, "error": "unexpected" }),
        })
    });
    let pane = Pane {
        id: "s1-zeus".to_owned(),
        generation: 7,
    };
    let window = window(
        &home.adapter(),
        &Request::resumed("e2c56db5-dffb-48d2-b060-d0f5a71096e0"),
    );
    assert_eq!(done(window.ready(&host, &pane)), Ok(Readiness::Ready));
    assert_eq!(
        done(window.deliver(&host, &pane, "hello")),
        Admission::Admitted { queued: false }
    );
    assert_eq!(
        host.asked.borrow().last().unwrap(),
        &(
            "pane.write_paste".to_owned(),
            json!({ "id": "s1-zeus", "generation": 7, "body": "hello" })
        )
    );
    paste_in_flight.set(true);
    assert_eq!(
        done(window.ready(&host, &pane)),
        Ok(Readiness::Held(Held::Because(
            "a paste is on its way to the window".to_owned()
        )))
    );
    // What the human typed and has not sent holds the paste: never pasted
    // into their text (the owner's choice, 2026-10-03).
    paste_in_flight.set(false);
    unsent.set(true);
    assert_eq!(
        done(window.ready(&host, &pane)),
        Ok(Readiness::Held(Held::Unsent))
    );
}

#[test]
fn reports_a_refused_request_as_exhausted_quota_with_the_reset_its_text_names() {
    let home = Home::new();
    let session = "2c1a6b64-0d2c-4f4e-9a7b-6f1c5f2e8d90";
    let refused = json!({
        "type": "assistant",
        "isApiErrorMessage": true,
        "apiErrorStatus": 429,
        "error": "rate_limit",
        "message": {
            "id": format!("{session}-message-2"),
            "role": "assistant",
            "content": [{ "type": "text", "text": "You've hit your limit. Resets in 2 hours." }],
        },
    });
    home.transcript(
        session,
        &[user_line(session, 1, TASK), record(session, 2, &refused)],
    );
    let window = window(&home.adapter(), &Request::resumed(session));
    let observed = done(window.observe()).unwrap();
    assert_eq!(
        serde_json::to_value(&observed.quota).unwrap(),
        json!({
            "state": "exhausted",
            "at": "2026-09-19T12:00:02.000Z",
            "resetsAt": "2026-09-19T14:00:02.000Z",
        })
    );
    home.transcript(
        session,
        &[
            user_line(session, 1, "hello"),
            answer_line(session, 2, "Hi"),
            stop_line(session, 3),
        ],
    );
    assert_eq!(done(window.observe()).unwrap().quota, None);
}
