//! Codex's adapter and its channel, as Node's Codex adapter suite, its channel
//! suite and the Codex cases of its role-skills suite held them (TEST-BDC-05,
//! IMPL-BDC-07), each case under its sentence: how a Codex window is launched
//! under the supervisor (here), how it is followed (`windows`), how a message
//! reaches it through the supervisor's broker (`channel`), and the role it is
//! given (`roles`). Each test gets a throwaway home and a stand-in `codex` on
//! PATH, whose answers (its native queue, its version) and whose app-server are
//! scripted. Asked anything else, the stand-in is not there: a launch that asks
//! it more fails. The channel's cases against a broker on real sockets and the
//! machine's own clock are in `tests/channels/codex.rs`.
//!
//! A window comes only from a prepare here, where Node's tests made up a
//! launch bag: a test of a window on a known thread prepares it as that
//! thread resumed. The broker's part is the test's: the loopback answers
//! each question of the window, and a message it was handed is what the test
//! reads from what the loopback was asked. Where Node's channel test took a
//! target of any shape, a Rust target has only the shapes that can be
//! written: a launch that is not a Codex one, or has no broker, or a text
//! that is none, cannot be made, and the pane, the thread and the claim are
//! the cases left.

mod channel;
mod roles;
mod windows;

use std::fs;
use std::path::Path;
use std::rc::Rc;

use cf_base::env::Env;
use cf_base::path;
use cf_harness::codex::CodexAdapter;
use cf_harness::contract::{Adapter, Agent, Launch, LaunchId, Pane, Prepared, Records};
use cf_harness::testing::{
    fake_executable, finished, named, ChildScript, Ends, Fakes, Sent, Served,
};
use serde_json::{json, Value};
use tempfile::TempDir;

/// The launch's id: a uuid, as the engine mints one (Node's tests took any
/// filename-safe word).
const LAUNCH: &str = "0a1b2c3d-4e5f-4061-8a7b-9c0d1e2f3a4b";
const THREAD: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";
const NEXT: &str = "7c9e6679-7425-40de-944b-e07fc1f90ae7";
const TASK: &str = "[ConsensFlow m-1 \u{b7} T-1 \u{b7} task from @chief]\nWrite the parser";
const ROLE: &str = "# ConsensFlow worker\n\nRole text for the test.";
const USAGE: &str = "Usage: codex queue --thread <id> --message <text>\n";
const LUNA: Agent = Agent {
    model: Some("gpt-5.6-luna"),
    effort: Some("low"),
    thinking: None,
    designer: false,
};

/// A throwaway home, ConsensFlow's folder and a stand-in `codex` on PATH.
struct Home {
    _dir: TempDir,
    root: String,
    vars: Vec<(String, String)>,
    /// Where the stand-in `codex` is found.
    executable: String,
}

impl Home {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("cf-codex-adapter-")
            .tempdir()
            .unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let vars: Vec<(String, String)> = [
            ("HOME", "home"),
            ("CONSENSFLOW_HOME", "consensflow"),
            ("PATH", "bin"),
        ]
        .into_iter()
        .map(|(name, folder)| (name.to_owned(), path::join(&[&root, folder])))
        .collect();
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        let file = dir.path().join("bin").join("codex");
        let executable = fake_executable(&file).to_string_lossy().into_owned();
        Self {
            _dir: dir,
            root,
            vars,
            executable,
        }
    }

    fn env(&self) -> Env {
        Env::from_vars(self.vars.iter().cloned())
    }
}

/// A launch to prepare: a worker's, unless a test changes it.
struct Request {
    role: &'static str,
    resume: Option<String>,
    message: Option<String>,
    agent: Option<Agent<'static>>,
    instructions: String,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            role: "worker",
            resume: None,
            message: Some(TASK.to_owned()),
            agent: Some(LUNA),
            instructions: ROLE.to_owned(),
        }
    }
}

impl Request {
    /// The chief's: no model of its own, no first message.
    fn chief() -> Self {
        Self {
            role: "chief",
            message: None,
            agent: None,
            ..Self::default()
        }
    }

    fn resuming(thread: &str) -> Self {
        Self {
            resume: Some(thread.to_owned()),
            message: None,
            ..Self::default()
        }
    }
}

fn prepare(adapter: &CodexAdapter, request: &Request) -> Result<Prepared, String> {
    let id = LaunchId::new(LAUNCH).unwrap();
    let launch = Launch {
        id: &id,
        project: 1,
        handle: "diana",
        role: request.role,
        directory: "/work/app",
        resume: request.resume.as_deref(),
        message: request.message.as_deref(),
        agent: request.agent,
        instructions: &request.instructions,
    };
    finished(adapter.prepare(&launch))
}

/// An adapter whose records are `records`, its clock, randomness and programs
/// the fakes' of `home`: a Codex that has the native queue (or not, as
/// `queue` says), and an app-server that gives `instructions`. One launch's
/// worth of the stand-in's answers, and more app-servers, for a test that
/// prepares again: a probe's answer is kept.
fn adapter_with(
    home: &Home,
    records: Option<Rc<dyn Records>>,
    instructions: &str,
    queue: bool,
) -> (CodexAdapter, Fakes) {
    let env = home.env();
    let fakes = Fakes::new(&env);
    let mut services = fakes.services(&env, Path::new(&home.root));
    if let Some(records) = records {
        services.records = records;
    }
    let answer = |text: &str| Ok(text.to_owned());
    let processes = &fakes.processes;
    processes.run_answer(
        "codex queue --help",
        if queue {
            answer(USAGE)
        } else {
            Err(cf_harness::seams::processes::Failed {
                message: "Command failed: codex queue --help\n".to_owned(),
                code: Some(2),
                killed: false,
                stdout: String::new(),
            })
        },
    );
    processes.run_answer("codex --version", answer("codex-cli 0.150.0\n"));
    for _ in 0..8 {
        app_server(&fakes, instructions);
    }
    (CodexAdapter::new(&services), fakes)
}

/// The adapter of a Codex with the native queue, reading the records of `home`.
fn adapter(home: &Home) -> (CodexAdapter, Fakes) {
    adapter_with(home, None, "", true)
}

/// An app-server that answers the role's dialogue with `instructions`.
fn app_server(fakes: &Fakes, instructions: &str) {
    let config =
        json!({ "id": 2, "result": { "config": { "developer_instructions": instructions } } });
    fakes.processes.child(
        "codex",
        ChildScript {
            lines: vec![r#"{"id":1,"result":{}}"#.to_owned(), config.to_string()],
            ends: Ends::Asked,
        },
    );
}

/// What the broker says: the window shows `session` (a thread, or none) and
/// would take a message, or not.
fn shows(session: Option<&str>, available: bool) -> Served {
    let word = json!({ "launchId": LAUNCH, "sessionId": session, "available": available });
    Served::Head {
        status: 200,
        body: Sent::Now(word.to_string().into_bytes()),
    }
}

fn replies(body: &str) -> Served {
    Served::Head {
        status: 200,
        body: Sent::Now(body.as_bytes().to_vec()),
    }
}

/// The launch arguments without the role text, which every window carries as
/// `-c developer_instructions=…`.
fn without_role(argv: &[String]) -> Vec<String> {
    let at = argv
        .iter()
        .position(|arg| arg.starts_with("developer_instructions="))
        .expect("the role text rides along");
    assert_eq!(argv[at - 1], "-c");
    let mut rest = argv.to_vec();
    rest.drain(at - 1..=at);
    rest
}

fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|&word| word.to_owned()).collect()
}

/// A pane of the window `diana`.
fn pane() -> Pane {
    Pane {
        id: "s1-diana".to_owned(),
        generation: 2,
    }
}

#[test]
fn runs_codex_under_its_supervisor_in_full_permission_mode_with_the_task_last() {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    assert_eq!(plan.native_session, None, "the broker names the thread");
    let spawned: Vec<String> = fakes
        .processes
        .take_spawned()
        .iter()
        .map(|(program, _)| named(program))
        .collect();
    assert_eq!(spawned, ["codex app-server"]);
    let mut expected = vec![
        path::join(&[
            &home.root,
            "bundle",
            "bin",
            if cfg!(windows) { "cf.exe" } else { "cf" },
        ]),
        "codex-session".to_owned(),
        home.executable.clone(),
    ];
    expected.extend(words(&[
        "--enable",
        "default_mode_request_user_input",
        "-c",
        "suppress_unstable_features_warning=true",
        "-c",
        "check_for_update_on_startup=false",
        "-c",
        "allow_login_shell=false",
        "--model",
        "gpt-5.6-luna",
        "-c",
        "model_reasoning_effort=\"low\"",
        "--dangerously-bypass-approvals-and-sandbox",
        TASK,
    ]));
    assert_eq!(without_role(&plan.argv), expected);
    let [(name, bridge)] = &plan.env[..] else {
        panic!("{:?}", plan.env);
    };
    assert_eq!(name, "CF_CODEX_SESSION_BRIDGE");
    let told: Value = serde_json::from_str(bridge).unwrap();
    assert_eq!(told["launchId"], LAUNCH);
    assert_eq!(plan.drop_env, ["OPENAI_API_KEY"]);
}

#[test]
fn resumes_its_thread() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::resuming(THREAD)).unwrap();
    assert_eq!(plan.native_session.as_deref(), Some(THREAD));
    assert_eq!(
        without_role(&plan.argv)[2..],
        [
            home.executable.as_str(),
            "--enable",
            "default_mode_request_user_input",
            "-c",
            "suppress_unstable_features_warning=true",
            "-c",
            "check_for_update_on_startup=false",
            "-c",
            "allow_login_shell=false",
            "resume",
            THREAD,
            "--model",
            "gpt-5.6-luna",
            "-c",
            "model_reasoning_effort=\"low\"",
            "--dangerously-bypass-approvals-and-sandbox",
        ]
    );
}

#[test]
fn opens_an_image_agent_s_window_on_codex_s_own_model_whose_image_tool_draws_no_model_or_effort_of_its_agent(
) {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let designer = Agent {
        model: Some("codex-image"),
        effort: Some("high"),
        thinking: None,
        designer: true,
    };
    let fresh = Request {
        agent: Some(designer),
        ..Request::default()
    };
    let plan = prepare(&adapter, &fresh).unwrap();
    let tail: Vec<String> = without_role(&plan.argv)[2..].to_vec();
    assert_eq!(
        tail,
        [
            home.executable.as_str(),
            "--enable",
            "default_mode_request_user_input",
            "-c",
            "suppress_unstable_features_warning=true",
            "-c",
            "check_for_update_on_startup=false",
            "-c",
            "allow_login_shell=false",
            "--dangerously-bypass-approvals-and-sandbox",
            TASK,
        ]
    );
    let resumed = Request {
        agent: Some(designer),
        ..Request::resuming(THREAD)
    };
    let again = prepare(&adapter, &resumed).unwrap();
    let argv = without_role(&again.argv);
    assert_eq!(
        argv[argv.len() - 3..],
        [
            "resume",
            THREAD,
            "--dangerously-bypass-approvals-and-sandbox"
        ]
    );
    assert_eq!(plan.drop_env, ["OPENAI_API_KEY"]);
}

#[test]
fn starts_a_member_with_codex_s_mcp_servers_as_it_starts_the_chief_neither_command_line_switches_them_off(
) {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let member = prepare(&adapter, &Request::default()).unwrap();
    let chief = prepare(&adapter, &Request::chief()).unwrap();
    // What a command line could say of them: a flag, or a `-c` setting, that names MCP.
    let switches = |plan: &Prepared| -> Vec<String> {
        let argv = without_role(&plan.argv);
        let settings = argv
            .iter()
            .enumerate()
            .filter(|&(at, arg)| arg.starts_with('-') || (at > 0 && argv[at - 1] == "-c"));
        settings
            .map(|(_, setting)| setting.clone())
            .filter(|setting| setting.to_ascii_lowercase().contains("mcp"))
            .collect()
    };
    assert_eq!(switches(&chief), Vec::<String>::new());
    assert_eq!(switches(&member), switches(&chief));
}

#[test]
fn lists_no_mcp_server_of_codex_s_for_a_member_s_launch_there_is_none_to_switch_off() {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    // A Codex that has servers and would list them if it were asked.
    let listing = json!([
        { "name": "cua_repl", "transport": { "type": "stdio", "command": "cua", "args": [] } },
        {
            "name": "idea",
            "transport": { "type": "streamable_http", "url": "http://127.0.0.1:64342/stream" },
        },
        { "name": "computer-history" },
    ]);
    fakes
        .processes
        .always_answer("codex mcp list --json", Ok(listing.to_string()));
    prepare(&adapter, &Request::default()).unwrap();
    let ran: Vec<String> = fakes
        .processes
        .take_ran()
        .iter()
        .map(|(program, _)| named(program))
        .collect();
    assert_eq!(ran, ["codex queue --help"], "all it is asked is its queue");
}

#[test]
fn gives_the_chief_no_question_tool_it_asks_the_human_in_plain_words_in_its_window() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let member = prepare(&adapter, &Request::default()).unwrap();
    assert!(
        member
            .argv
            .iter()
            .any(|arg| arg == "default_mode_request_user_input"),
        "a member asks the board"
    );
    let chief = prepare(&adapter, &Request::chief()).unwrap();
    assert!(!chief
        .argv
        .iter()
        .any(|arg| arg == "default_mode_request_user_input"));
    assert!(!chief
        .argv
        .iter()
        .any(|arg| arg == "suppress_unstable_features_warning=true"));
}

#[test]
fn refuses_to_open_a_codex_without_the_native_queue_naming_its_version() {
    let home = Home::new();
    let (adapter, _fakes) = adapter_with(&home, None, "", false);
    assert_eq!(
        prepare(&adapter, &Request::default()).err().as_deref(),
        Some("Codex 0.150.0 has no native queue, which ConsensFlow needs to reach its window: update Codex.")
    );
}
