//! Pi's adapter and its extension, as Node's Pi adapter and install suites and
//! the Pi cases of its role-skills suite held them (TEST-BDC-05, IMPL-BDC-07),
//! each case under its sentence: how a Pi window is launched, how a message
//! reaches it through the extension's inbox, what the extension says of the
//! window, and what is made for it to load. Each test gets a throwaway home and
//! a stand-in `pi` on PATH.
//!
//! A window comes only from a prepare here, where Node's tests made up a
//! launch bag: a test of a window on a known conversation prepares it as
//! that conversation resumed. The extension's part in a window is the
//! test's: it reads each record from the inbox and writes its verdict.
//! Where Node's test stood in the records of a conversation, a stand-in
//! `Records` does here.

use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use cf_base::env::Env;
use cf_base::path;
use cf_harness::contract::{
    Adapter, Admission, Agent, Held, Launch, LaunchId, Pane, Prepared, Readiness, Records, Work,
};
use cf_harness::pi::{prepare_extension, Extension, PiAdapter};
use cf_harness::records::{Item, Options, PiSettlement, Quota, Reading, Record, Role, Settlement};
use cf_harness::seams::Time;
use cf_harness::testing::{
    fake_executable, finished, AnsweringHost, Driver, Fakes, FIRST_MESSAGE_MS,
};
use cf_proto::agents::Harness;
use serde_json::{json, Value};
use tempfile::TempDir;

/// The launch's id: a uuid, as the engine mints one (Node's tests took any
/// filename-safe word).
const LAUNCH: &str = "0a1b2c3d-4e5f-4061-8a7b-9c0d1e2f3a4b";
const TASK: &str = "[ConsensFlow m-1 \u{b7} T-1 \u{b7} task from @chief]\nWrite the parser";
const MODEL: &str = "openrouter/meta/muse-spark-1.3";
const ROLE: &str = "# ConsensFlow worker\n\nRole text for the test.";

/// A throwaway home, ConsensFlow's folder and a stand-in `pi` on PATH.
struct Home {
    _dir: TempDir,
    root: String,
    vars: Vec<(String, String)>,
    /// Where the stand-in `pi` is found, once it is installed.
    executable: String,
}

impl Home {
    /// A home with a stand-in `pi` on PATH.
    fn new() -> Self {
        let mut home = Self::without_pi();
        home.executable = home.install_pi();
        home
    }

    /// A home whose PATH holds no `pi`, yet.
    fn without_pi() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("cf-pi-adapter-")
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
        Self {
            _dir: dir,
            root,
            vars,
            executable: String::new(),
        }
    }

    /// Puts a stand-in `pi` on PATH: where it is found.
    fn install_pi(&self) -> String {
        let file = Path::new(&self.root).join("bin").join("pi");
        fake_executable(&file).to_string_lossy().into_owned()
    }

    fn var(&self, name: &str) -> &str {
        let found = self.vars.iter().find(|(held, _)| held == name);
        &found.unwrap().1
    }

    fn env(&self) -> Env {
        Env::from_vars(self.vars.iter().cloned())
    }

    /// Where the launch's own files are.
    fn launch_folder(&self) -> String {
        path::join(&[self.var("CONSENSFLOW_HOME"), "integrations", "pi", LAUNCH])
    }
}

/// A launch to prepare: a worker's, unless a test changes it.
struct Request {
    resume: Option<String>,
    message: Option<String>,
    instructions: String,
    role: &'static str,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            resume: None,
            message: Some(TASK.to_owned()),
            instructions: ROLE.to_owned(),
            role: "worker",
        }
    }
}

fn prepare(adapter: &PiAdapter, request: &Request) -> Result<Prepared, String> {
    let id = LaunchId::new(LAUNCH).unwrap();
    let launch = Launch {
        id: &id,
        project: 1,
        handle: "zeus",
        role: request.role,
        directory: "/work/app",
        resume: request.resume.as_deref(),
        message: request.message.as_deref(),
        agent: Some(Agent {
            model: Some(MODEL),
            effort: None,
            thinking: Some("high"),
            designer: false,
        }),
        instructions: &request.instructions,
        first_message_ms: FIRST_MESSAGE_MS,
    };
    finished(adapter.prepare(&launch))
}

fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|&word| word.to_owned()).collect()
}

/// A record that says what the test wants of it, and what it was asked.
struct Stand {
    asked: RefCell<Vec<(Harness, String, Options)>>,
    reading: Arc<Reading>,
}

impl Records for Stand {
    fn look<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
        options: &'a Options,
    ) -> Work<'a, Arc<Reading>> {
        self.asked
            .borrow_mut()
            .push((harness, session.to_owned(), options.clone()));
        Box::pin(async { Arc::clone(&self.reading) })
    }

    fn has_transcript<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
    ) -> Work<'a, Result<bool, String>> {
        Box::pin(async { Ok(false) })
    }
}

/// An adapter whose records are `records`, its clock and randomness the
/// fakes' of `home`.
fn adapter_with(home: &Home, records: Rc<dyn Records>) -> (PiAdapter, Fakes) {
    let env = home.env();
    let fakes = Fakes::new(&env);
    let mut services = fakes.services(&env, Path::new(&home.root));
    services.records = records;
    (PiAdapter::new(&services), fakes)
}

/// An adapter that reads the conversation's own record, from the files of
/// `home`.
fn adapter(home: &Home) -> (PiAdapter, Fakes) {
    let env = home.env();
    let fakes = Fakes::new(&env);
    let services = fakes.services(&env, Path::new(&home.root));
    (PiAdapter::new(&services), fakes)
}

/// What the extension writes when its window starts on a conversation.
fn shows(home: &Home, session: &str) {
    let settled = path::join(&[&home.launch_folder(), "settled"]);
    fs::create_dir_all(&settled).unwrap();
    let file = path::join(&[&settled, &format!("{LAUNCH}.shown.json")]);
    let said = json!({ "launchId": LAUNCH, "sessionId": session });
    fs::write(file, format!("{said}\n")).unwrap();
}

/// What the extension does with each record in the inbox: takes it,
/// acknowledges it as taken, and takes it out. The text of each.
fn extension_takes(home: &Home) -> Vec<String> {
    let folder = home.launch_folder();
    let (inbox, ack) = (
        path::join(&[&folder, "inbox"]),
        path::join(&[&folder, "ack"]),
    );
    let mut names: Vec<String> = fs::read_dir(&inbox)
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".json"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    let mut texts = Vec::new();
    for name in names {
        let file = path::join(&[&inbox, &name]);
        let record: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        texts.push(record["text"].as_str().unwrap().to_owned());
        fs::create_dir_all(&ack).unwrap();
        let verdict = json!({ "id": record["id"], "admitted": true, "mode": "tui" });
        fs::write(path::join(&[&ack, &name]), verdict.to_string()).unwrap();
        fs::remove_file(file).unwrap();
    }
    texts
}

#[test]
fn launches_pi_with_its_extension_a_session_name_of_ours_and_the_task_last() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    let session = plan.native_session.clone().unwrap();
    // `cf-<project>-<handle>-` and 4 bytes in hex.
    let named = session.strip_prefix("cf-1-zeus-").unwrap();
    assert!(
        named.len() == 8
            && named
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "{session}"
    );
    let extension = plan.argv[2].clone();
    let shaped = extension.replace('\\', "/");
    let (folder, file) = shaped.split_once("extensions/pi/").unwrap();
    assert!(folder.starts_with(&home.var("CONSENSFLOW_HOME").replace('\\', "/")));
    let (hash, rest) = file.split_once('/').unwrap();
    assert!(!hash.is_empty() && hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(rest, "hosts/pi-extension/consensflow-delivery.mjs");
    let skill = path::join(&[
        &home.launch_folder(),
        "role",
        ".claude",
        "skills",
        "consensflow-worker",
        "SKILL.md",
    ]);
    let mut argv = vec![home.executable.clone(), "--extension".to_owned(), extension];
    argv.extend(["--skill".to_owned(), skill]);
    argv.extend(["--append-system-prompt".to_owned(), ROLE.to_owned()]);
    argv.extend(words(&["--session-id"]));
    argv.push(session);
    argv.extend(words(&[
        "--model",
        MODEL,
        "--thinking",
        "high",
        "--approve",
        TASK,
    ]));
    assert_eq!(plan.argv, argv);
    let root = home.launch_folder();
    let env = |name: &str| {
        let found = plan.env.iter().find(|(held, _)| held == name);
        found.map(|(_, value)| value.clone())
    };
    assert_eq!(
        env("CF_DELIVERY_INBOX"),
        Some(path::join(&[&root, "inbox"]))
    );
    assert_eq!(
        env("CF_DELIVERY_SETTLED"),
        Some(path::join(&[&root, "settled"]))
    );
    assert_eq!(env("CF_DELIVERY_LAUNCH_ID").as_deref(), Some(LAUNCH));
    assert_eq!(plan.drop_env, Vec::<String>::new());
}

#[test]
fn resumes_the_session_it_has() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let request = Request {
        resume: Some("cf-1-zeus-0000abcd".to_owned()),
        message: None,
        ..Request::default()
    };
    let plan = prepare(&adapter, &request).unwrap();
    assert_eq!(plan.native_session.as_deref(), Some("cf-1-zeus-0000abcd"));
    assert_eq!(
        plan.argv[7..],
        words(&[
            "--session-id",
            "cf-1-zeus-0000abcd",
            "--model",
            MODEL,
            "--thinking",
            "high",
            "--approve",
        ])
    );
}

#[test]
fn delivers_through_the_real_channel_a_claim_then_the_extension_inbox() {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    let window = prepare(&adapter, &Request::default()).unwrap().window;
    let host = Rc::new(AnsweringHost::new(|_| Ok(json!({ "ok": true }))));
    let pane = Pane {
        id: "s1-zeus".to_owned(),
        generation: 2,
    };
    let mut driver = Driver::default();
    let mut taken = Vec::new();
    // Pi hands the text to its model's API, which refuses half a character
    // too. Node's second case began with half a surrogate pair, which it
    // dropped: a Rust text holds none.
    for (op, text) in [
        (0, "hi"),
        (1, "half  of it, \u{1b}[31mred\u{1b}[0m and 50%\r60%"),
    ] {
        let (window, host, pane, text) = (
            Rc::clone(&window),
            Rc::clone(&host),
            pane.clone(),
            text.to_owned(),
        );
        driver.begin(
            op,
            async move { window.deliver(&*host, &pane, &text).await },
        );
        assert!(driver.run().is_empty(), "waits for the extension's verdict");
        // The extension's part, which the clock lets it do between two looks.
        taken.extend(extension_takes(&home));
        assert!(fakes.time.fire_next(fakes.time.wall_ms() + 10));
        assert_eq!(
            driver.run(),
            [(op, Ok(Admission::Admitted { queued: true }))],
            "admitted, in the queue"
        );
    }
    let claim = (
        "pane.claim".to_owned(),
        json!({ "pane": "s1-zeus", "generation": 2 }),
    );
    assert_eq!(*host.asked.borrow(), [claim.clone(), claim]);
    assert_eq!(
        taken,
        [
            "hi",
            "half  of it, \u{241b}[31mred\u{241b}[0m and 50%\u{240d}60%"
        ]
    );
}

/// The reading of a conversation holding `items`, its turn `settlement`.
fn reading(items: Vec<Item>, settlement: Settlement, quota: Option<Arc<Quota>>) -> Arc<Reading> {
    Arc::new(Reading::Known(Record {
        items,
        in_flight: false,
        asking: false,
        failed: false,
        quota,
        settlement,
    }))
}

/// The reading of a conversation whose turn settled: one user message and
/// the assistant's answer.
fn settled_record(text: &str) -> Arc<Reading> {
    let item = |id: &str, role| Item {
        id: Arc::from(id),
        role,
        text: Arc::from(text),
        complete: true,
        at: None,
        commentary: false,
    };
    let items = vec![item("1", Role::User), item("2", Role::Assistant)];
    reading(items, Settlement::Settled, None)
}

#[test]
fn follows_the_window_to_the_conversation_a_new_or_resume_left_it_on_holding_until_it_names_one() {
    let home = Home::new();
    let stand = Rc::new(Stand {
        asked: RefCell::new(Vec::new()),
        reading: settled_record("hello"),
    });
    let (adapter, _fakes) = adapter_with(&home, Rc::clone(&stand) as Rc<dyn Records>);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    let (window, native) = (plan.window, plan.native_session.unwrap());
    let host = AnsweringHost::new(|_| Ok(json!({ "ok": true })));
    let pane = Pane {
        id: "s1-zeus".to_owned(),
        generation: 1,
    };
    let ready = || finished(window.ready(&host, &pane)).unwrap();
    // Until its extension starts, Pi has not said which conversation it shows.
    let before = finished(window.observe()).unwrap();
    assert!(before.unnamed);
    let held = before.waiting.unwrap().reason.unwrap();
    assert!(held.contains("Pi has not said"), "{held}");
    assert_eq!(ready(), Readiness::Held(Held::Because(held)));
    shows(&home, &native);
    assert!(finished(window.observe()).unwrap().settled);
    assert_eq!(ready(), Readiness::Ready);

    // /new: the extension says the window shows another conversation.
    let fresh = "0199a6f0-4cc1-7d3e-9f7a-3c5b2e1d0a98";
    shows(&home, fresh);
    let observed = finished(window.observe()).unwrap();
    assert_eq!(observed.switched.as_deref(), Some(fresh));
    assert!(!observed.settled);
    assert_eq!(ready(), Readiness::Held(Held::ShowsAnother));

    // The dispatcher follows the window: the new conversation's record is read.
    window.follow(fresh);
    let followed = finished(window.observe()).unwrap();
    assert_eq!(followed.switched, None);
    assert_eq!(stand.asked.borrow().last().unwrap().1, fresh);
    assert_eq!(ready(), Readiness::Ready);
}

#[test]
fn reads_each_turn_s_end_from_the_extension_s_settled_marker() {
    let home = Home::new();
    let quota = Arc::new(Quota::Exhausted {
        at: None,
        resets_at: Some("2026-09-22T10:00:00.000Z".to_owned()),
    });
    let stand = Rc::new(Stand {
        asked: RefCell::new(Vec::new()),
        reading: reading(Vec::new(), Settlement::Unknown, Some(Arc::clone(&quota))),
    });
    let (adapter, _fakes) = adapter_with(&home, Rc::clone(&stand) as Rc<dyn Records>);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    let observed = finished(plan.window.observe()).unwrap();
    assert!(observed.settled, "no turn begun, nothing in flight");
    assert_eq!(observed.quota, Some(quota));
    let asked = stand.asked.borrow();
    let (harness, session, options) = &asked[0];
    assert_eq!(
        (*harness, session.as_str()),
        (Harness::Pi, plan.native_session.as_deref().unwrap())
    );
    assert_eq!(
        options.pi_settlement,
        Some(PiSettlement {
            directory: Some(path::join(&[&home.launch_folder(), "settled"])),
            launch_id: Some(LAUNCH.to_owned()),
        })
    );
}

mod install;
mod roles;
