//! A restart: what was open stays suspended, with no window opened and nothing
//! delivered, until the human resumes it, and then its window opens with the
//! environment the app expects; what was on its way into a window is settled at
//! the start; an exit the host tells reaches the engine where it is read, and
//! the page is told the board moved a hundred milliseconds after the first
//! change, once.

use cf_ledger::{Ledger, NewChief, NewNote, NewProject};

use super::*;

/// The options of a daemon on `root` and the app's end of its bridge, which
/// answers each window the daemon asks it to open, with the windows it was asked
/// for as it is.
fn app_that_opens_windows(root: &Path) -> (Options, Bridge, Rc<RefCell<Vec<Value>>>) {
    let (said, exited, handles) = (Said::default(), Rc::default(), Rc::default());
    let (options, app) = options(root, &said, &exited, &handles);
    let opened: Rc<RefCell<Vec<Value>>> = Rc::default();
    let heard = Rc::clone(&opened);
    app.on("pane.open", move |_, body| {
        heard.borrow_mut().push(body.clone());
        async move { Ok(json!({ "ok": true, "id": body["id"], "generation": body["generation"] })) }
    });
    (options, app, opened)
}

/// A home with a project in its ledger that was open when the last daemon ended,
/// the ledger still open, and the project's number.
fn ledger_before_the_start(root: &Path) -> (Ledger, i64) {
    let home = root.join("consensflow");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("agents.json"),
        json!({
            "schemaVersion": 1,
            "agents": [{ "id": "mybuilder", "kind": "claude-code", "model": "fake", "workTier": "standard" }]
        })
        .to_string(),
    )
    .unwrap();
    let mut ledger = open_ledger(&home.join("consensflow.db"), LedgerOptions::default()).unwrap();
    let workspace = root.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let project = ledger
        .create_project(&NewProject {
            directory: workspace.to_string_lossy().into_owned(),
            name: "app".to_owned(),
            chief: NewChief {
                harness: "claude-code".to_owned(),
                agent: Some("mybuilder".to_owned()),
            },
            staff: Vec::new(),
            gate: false,
            wake_up: None,
            specs: None,
        })
        .unwrap();
    (ledger, project.id)
}

/// The project of [`ledger_before_the_start`], the ledger closed.
fn open_project_before_the_start(root: &Path) -> i64 {
    let (ledger, project) = ledger_before_the_start(root);
    ledger.close().unwrap();
    project
}

/// The same, with a note of the human's for the chief that was on its way into
/// the chief's window when that daemon ended: the project and the note.
fn open_project_with_a_note_on_its_way(root: &Path) -> (i64, i64) {
    let (mut ledger, project) = ledger_before_the_start(root);
    let note = ledger
        .note(
            project,
            &NewNote {
                from: Some("human".to_owned()),
                to: "chief".to_owned(),
                task: None,
                body: "Any news?".to_owned(),
            },
        )
        .unwrap();
    ledger.begin_delivery(note.id).unwrap();
    ledger.close().unwrap();
    (project, note.id)
}

#[tokio::test]
async fn what_was_open_stays_suspended_and_opens_no_window_until_the_human_resumes_it() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let project = open_project_before_the_start(root.path());
            let (options, app, opened) = app_that_opens_windows(root.path());
            let daemon = start(environment(root.path()), options).await.unwrap();
            ask(&app, "ping", json!({})).await;
            tokio::time::sleep(Duration::from_millis(300)).await;

            assert!(
                opened.borrow().is_empty(),
                "no window opens: {:?}",
                opened.borrow()
            );
            let board = ask(&app, "board.get", json!({ "project": project })).await;
            assert_eq!(board["board"]["project"]["state"], "suspended");
            // The start suspended the project and marked it for nothing: no event
            // says it is to come back, and none says it was resumed.
            let trace =
                std::fs::read_to_string(root.path().join("consensflow").join("events.jsonl"))
                    .unwrap();
            let suspended = r#""kind":"project.state","data":{"from":"open","to":"suspended"}"#;
            assert!(trace.contains(suspended), "{trace}");
            assert!(
                !trace.contains("resumeOnStart") && !trace.contains(r#""to":"open""#),
                "{trace}"
            );
            daemon.stop("SIGTERM");
            daemon.finished().await;
        })
        .await;
}

#[tokio::test]
async fn a_start_settles_what_was_on_its_way_into_a_window_and_it_waits_for_the_humans_resume() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let (project, note) = open_project_with_a_note_on_its_way(root.path());
            let (options, app, opened) = app_that_opens_windows(root.path());
            let daemon = start(environment(root.path()), options).await.unwrap();
            ask(&app, "ping", json!({})).await;
            tokio::time::sleep(Duration::from_millis(300)).await;

            // The note the previous daemon was pasting is the queue's again, and
            // waits there for the human's Resume: no window opened to be given it.
            let inbox = ask(
                &app,
                "inbox.get",
                json!({ "project": project, "participant": "chief" }),
            )
            .await;
            let waiting = inbox["messages"]
                .as_array()
                .and_then(|messages| messages.iter().find(|message| message["id"] == note))
                .cloned()
                .unwrap();
            assert_eq!(waiting["state"], "queued", "{waiting}");
            assert_eq!(
                waiting["reason"], "the daemon stopped before it arrived",
                "{waiting}"
            );
            assert!(
                opened.borrow().is_empty(),
                "no window opens: {:?}",
                opened.borrow()
            );
            daemon.stop("SIGTERM");
            daemon.finished().await;
        })
        .await;
}

/// The session of the Devin window below, and the launch it ran under.
const DEVIN_SESSION: &str = "calm-river";
const DEVIN_LAUNCH: &str = "11111111-1111-4111-8111-111111111111";

/// Gap 5 of the Devin arrival gaps (`issues/devin-arrival-gaps-main-has-too.md`): the
/// previous daemon stopped while the first message of a resumed Devin window was on its
/// way, after Devin had stored it with the command it ran under, `/bypass <message>`. A
/// start sweeps the launch folders before it settles what was on its way, the launch's
/// prompt file and wire log with them; the record was read as Devin stored it, no paste,
/// and the message went back to its queue to be given again. The sweep keeps what the
/// launch proves of its first message (`cf_harness::devin::kept`), the settle reads the
/// stored message as its paste, and what was kept goes once it is settled. (Settled
/// first and swept after, a failed settle still lost the proof, and a settled reading
/// outlived the wire log it rested on: the reviews of B5's round 12.)
#[tokio::test]
async fn a_first_message_devin_stored_behind_bypass_is_settled_as_received_though_the_start_swept_its_launch(
) {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let (mut ledger, project) = ledger_before_the_start(root.path());
            let note = ledger
                .note(
                    project,
                    &NewNote {
                        from: Some("human".to_owned()),
                        to: "chief".to_owned(),
                        task: None,
                        body: "Any news?".to_owned(),
                    },
                )
                .unwrap();
            let begun = ledger.begin_delivery(note.id).unwrap();
            ledger
                .follow_conversation(note.recipient_id, "devin", DEVIN_SESSION)
                .unwrap();
            ledger.close().unwrap();
            let stored = format!(
                "/bypass {}",
                cf_engine::delivery_text::delivery_text(&begun.message, &begun.carried)
            );
            // Devin's store, the message in it as Devin stored it.
            drop(devin_store(root.path(), std::slice::from_ref(&stored)));
            // The launch's folder: its wire log names the session, and its prompt file
            // holds what it was given.
            let launch = root
                .path()
                .join("consensflow")
                .join("integrations")
                .join("devin")
                .join(DEVIN_LAUNCH);
            std::fs::create_dir_all(&launch).unwrap();
            let line = json!({
                "sessionId": DEVIN_SESSION,
                "update": { "sessionUpdate": "available_commands_update" },
            });
            std::fs::write(launch.join("wire.jsonl"), format!("{line}\n")).unwrap();
            std::fs::write(launch.join("prompt.txt"), &stored).unwrap();

            let (options, app, _) = app_that_opens_windows(root.path());
            let daemon = start(environment(root.path()), options).await.unwrap();
            ask(&app, "ping", json!({})).await;
            tokio::time::sleep(Duration::from_millis(300)).await;
            let inbox = ask(
                &app,
                "inbox.get",
                json!({ "project": project, "participant": "chief" }),
            )
            .await;
            let settled = inbox["messages"]
                .as_array()
                .and_then(|messages| messages.iter().find(|message| message["id"] == note.id))
                .cloned()
                .unwrap();
            assert_eq!(settled["state"], "delivered", "{settled}");
            assert_eq!(settled["attempts"], 1, "given once: {settled}");
            assert!(!launch.exists(), "the launch's folder went");
            let kept = root
                .path()
                .join("consensflow")
                .join("integrations")
                .join("devin-kept");
            assert_eq!(
                std::fs::read_dir(&kept).map(Iterator::count).unwrap_or(0),
                0,
                "what the sweep kept went once the start had settled"
            );
            daemon.stop("SIGTERM");
            daemon.finished().await;
        })
        .await;
}

/// Calliope's P3 on T-697 (and Sol's P1 on B5's round 12): what was kept of Devin's launches
/// whose folders went is the proof a settle reads, so a settle that failed, or panicked,
/// leaves it for the next start's, and only one that settled lets it go.
#[tokio::test]
async fn what_was_kept_of_swept_launches_outlives_a_settle_that_failed_and_goes_with_one_that_settled(
) {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().to_string_lossy().into_owned();
    let errors = Errors::new(
        Rc::new(Log::new(root.path())),
        Rc::new(Trace::new(root.path())),
    );
    let kept = root.path().join("integrations").join("devin-kept");
    std::fs::create_dir_all(&kept).unwrap();
    let file = kept.join(format!("{DEVIN_LAUNCH}.json"));
    std::fs::write(&file, r#"{"sessions":["calm-river"],"prompt":"the note"}"#).unwrap();
    let refused = || {
        cf_engine::seams::EngineError::from(cf_base::refusal::Refusal::new(
            "database-full",
            "database or disk is full",
        ))
    };
    settle(async { Err(refused()) }, &home, &errors).await;
    assert!(file.exists(), "a settle that failed keeps it");
    settle(
        async {
            if file.exists() {
                panic!("a bug in the settle");
            }
            Ok(())
        },
        &home,
        &errors,
    )
    .await;
    assert!(file.exists(), "and so does one that panicked");
    let log = std::fs::read_to_string(root.path().join("daemon.log")).unwrap_or_default();
    assert_eq!(
        log.matches("settling what was on its way failed").count(),
        2,
        "{log}"
    );
    settle(async { Ok(()) }, &home, &errors).await;
    assert!(!file.exists(), "one that settled lets it go");
}

/// Devin's store of its sessions under `root`'s home, with the session [`DEVIN_SESSION`]
/// and its user's messages `said`, each a node below the one before.
fn devin_store(root: &Path, said: &[String]) -> rusqlite::Connection {
    let data = if cfg!(windows) {
        root.join("home").join("AppData").join("Roaming")
    } else {
        root.join("home").join(".local").join("share")
    };
    let folder = data.join("devin").join("cli");
    std::fs::create_dir_all(&folder).unwrap();
    let store = rusqlite::Connection::open(folder.join("sessions.db")).unwrap();
    store
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, main_chain_id TEXT);
             CREATE TABLE IF NOT EXISTS message_nodes (row_id INTEGER PRIMARY KEY, session_id TEXT,
               node_id TEXT, parent_node_id TEXT, chat_message TEXT, created_at TEXT)",
        )
        .unwrap();
    for text in said {
        devin_says(&store, text);
    }
    store
}

/// A user's message `text` in Devin's store, below the last, the session's head now.
fn devin_says(store: &rusqlite::Connection, text: &str) {
    let count: i64 = store
        .query_row("SELECT count(*) FROM message_nodes", [], |row| row.get(0))
        .unwrap();
    let node = format!("n-{}", count + 1);
    let parent = (count > 0).then(|| format!("n-{count}"));
    let message =
        json!({ "message_id": format!("u-{}", count + 1), "role": "user", "content": text });
    store
        .execute(
            "INSERT INTO message_nodes (session_id, node_id, parent_node_id, chat_message, created_at)
             VALUES (?, ?, ?, ?, '2026-10-26T10:00:00Z')",
            (DEVIN_SESSION, &node, parent, message.to_string()),
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO sessions VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET main_chain_id = ?2",
            (DEVIN_SESSION, &node),
        )
        .unwrap();
}

/// The exit path of gap 5 (Calliope's P3 on T-697): a Devin chief comes back on its
/// conversation, and its first message, the handoff of a switch, goes behind `/bypass` in
/// its launch's prompt file. Devin stores it with the command kept, and the window ends before
/// the daemon read its record again: the exit removes the launch's folder before it reads
/// the record for the message it had on its way. What the folder proved is kept with it, so
/// the handoff is settled as received, not given back to go in again. (On Unix: the
/// stand-in Devin is a shell script.)
#[cfg(unix)]
#[tokio::test]
async fn a_devin_chiefs_first_message_behind_bypass_is_received_though_its_window_closed_while_delivering(
) {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let home = root.path().join("consensflow");
            std::fs::create_dir_all(&home).unwrap();
            std::fs::write(
                home.join("agents.json"),
                json!({
                    "schemaVersion": 1,
                    "agents": [{ "id": "mydevin", "kind": "devin", "model": "swe-2", "workTier": "standard" }]
                })
                .to_string(),
            )
            .unwrap();
            let mut ledger = open_ledger(&home.join("consensflow.db"), LedgerOptions::default()).unwrap();
            let workspace = root.path().join("workspace");
            std::fs::create_dir_all(&workspace).unwrap();
            let project = ledger
                .create_project(&NewProject {
                    directory: workspace.to_string_lossy().into_owned(),
                    name: "app".to_owned(),
                    chief: NewChief {
                        harness: "devin".to_owned(),
                        agent: Some("mydevin".to_owned()),
                    },
                    staff: Vec::new(),
                    gate: false,
                    wake_up: None,
                    specs: None,
                })
                .unwrap()
                .id;
            // The handoff of a switch to Devin, still on its way to the chief.
            let note = ledger
                .note(
                    project,
                    &NewNote {
                        from: None,
                        to: "chief".to_owned(),
                        task: None,
                        body: "You are the chief now of app, switched in from Claude Code."
                            .to_owned(),
                    },
                )
                .unwrap();
            ledger
                .follow_conversation(note.recipient_id, "devin", DEVIN_SESSION)
                .unwrap();
            ledger.close().unwrap();
            // The conversation the chief comes back on, which Devin holds.
            let store = devin_store(root.path(), &["What is on the board?".to_owned()]);
            // A Devin to be found on PATH, which says its version.
            let bin = root.path().join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            let devin = bin.join("devin");
            std::fs::write(
                &devin,
                "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'devin 3000.11.205 (stand-in)'; exit 0; fi\nsleep 600\n",
            )
            .unwrap();
            std::fs::set_permissions(
                &devin,
                std::os::unix::fs::PermissionsExt::from_mode(0o755),
            )
            .unwrap();

            let (options, app, opened) = app_that_opens_windows(root.path());
            let daemon = start(environment(root.path()), options).await.unwrap();
            ask(&app, "project.resume", json!({ "project": project })).await;
            wait_for(root.path(), || !opened.borrow().is_empty()).await;
            let pane = opened.borrow()[0].clone();
            wait_for(root.path(), || daemon.parts.engine.pane(note.recipient_id).is_some()).await;
            let launches = home.join("integrations").join("devin");
            let launch = std::fs::read_dir(&launches)
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            let prompt = std::fs::read_to_string(launch.join("prompt.txt")).unwrap();
            assert!(prompt.starts_with("/bypass [ConsensFlow m-"), "{prompt}");
            let line = json!({
                "sessionId": DEVIN_SESSION,
                "update": { "sessionUpdate": "available_commands_update" },
            });
            // The clock stands from here: nothing of the daemon looks again before the exit.
            tokio::time::pause();
            std::fs::write(launch.join("wire.jsonl"), format!("{line}\n")).unwrap();
            devin_says(&store, &prompt);
            app.event(
                "pane.exit",
                json!({ "id": pane["id"], "generation": pane["generation"] }),
            );
            assert_eq!(ask(&app, "ping", json!({})).await, json!({ "ok": true }));
            tokio::time::resume();
            let mut settled = Value::Null;
            let until = Instant::now() + Duration::from_secs(10);
            while Instant::now() < until {
                let inbox = ask(
                    &app,
                    "inbox.get",
                    json!({ "project": project, "participant": "chief" }),
                )
                .await;
                settled = inbox["messages"]
                    .as_array()
                    .and_then(|messages| messages.iter().find(|message| message["id"] == note.id))
                    .cloned()
                    .unwrap();
                if settled["state"] != "delivering" {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            assert_eq!(settled["state"], "delivered", "{settled}");
            assert_eq!(settled["attempts"], 1, "given once: {settled}");
            assert!(!launch.exists(), "the launch's folder went with its window");
            daemon.stop("SIGTERM");
            daemon.finished().await;
        })
        .await;
}

#[tokio::test]
async fn the_humans_resume_opens_the_chiefs_window_with_the_environment_the_app_expects() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let project = open_project_before_the_start(root.path());
            let (options, app, opened) = app_that_opens_windows(root.path());
            let events: Rc<RefCell<Vec<(String, Value)>>> = Rc::default();
            let told = Rc::clone(&events);
            app.on_event("state.changed", move |body| {
                told.borrow_mut()
                    .push(("state.changed".to_owned(), body.clone()))
            });
            let daemon = start(environment(root.path()), options).await.unwrap();

            // The human's Resume opens the chief's window through the host.
            let resumed = ask(&app, "project.resume", json!({ "project": project })).await;
            assert_eq!(resumed["ok"], true, "{resumed}");
            wait_for(root.path(), || !opened.borrow().is_empty()).await;
            let open = opened.borrow()[0].clone();
            assert_eq!(open["id"], format!("p{project}-chief"));
            let env = &open["env"];
            let url = daemon.handle().url.trim_end_matches('/').to_owned();
            assert_eq!(env["CONSENSFLOW_URL"], url.as_str());
            assert_eq!(env["CONSENSFLOW_PROJECT"], project.to_string());
            assert_eq!(env["CONSENSFLOW_PARTICIPANT"], "chief");
            let delimiter = if cfg!(windows) { ';' } else { ':' };
            let bin = root.path().join("bundle").join("bin");
            assert_eq!(
                env["PATH"],
                format!(
                    "{}{delimiter}{}",
                    bin.display(),
                    root.path().join("bin").display()
                )
            );
            assert!(env["CONSENSFLOW_TOKEN"]
                .as_str()
                .is_some_and(|token| token.len() == 64));
            // The keys the pane host reads, in the order Node sent them.
            let keys: Vec<&str> = open
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(keys, ["id", "generation", "cwd", "argv", "env", "dropEnv"]);
            assert_eq!(
                open["cwd"],
                root.path().join("workspace").to_string_lossy().as_ref()
            );
            // The chief's role text names the cf of this window.
            let argv: Vec<&str> = open["argv"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect();
            let at = argv
                .iter()
                .position(|arg| *arg == "--append-system-prompt-file")
                .expect("a role file");
            let role = std::fs::read_to_string(argv[at + 1]).unwrap();
            // As the daemon names it: `cf.exe`, spelled with `/`, on Windows.
            let cf = machine::bundle_of(&bin.join("cf")).pane_cf;
            assert!(role.contains(&format!("Here `cf` is {cf}")), "{role}");

            // The page is told the board changed, once for what came together.
            wait_for(root.path(), || !events.borrow().is_empty()).await;
            assert_eq!(
                events.borrow()[0],
                ("state.changed".to_owned(), json!({ "reason": "core" }))
            );
            // The ledger's own events are in the trace as they happen: the
            // start suspended the project, and the human's resume opened it.
            let trace =
                std::fs::read_to_string(root.path().join("consensflow").join("events.jsonl"))
                    .unwrap();
            let suspended = r#""kind":"project.state","data":{"from":"open","to":"suspended"}"#;
            let opened_again = r#""kind":"project.state","data":{"from":"suspended","to":"open"}"#;
            let at = |needle: &str| trace.lines().position(|line| line.contains(needle));
            assert!(
                at(suspended) < at(opened_again) && at(opened_again).is_some(),
                "{trace}"
            );
            daemon.stop("SIGTERM");
            daemon.finished().await;
        })
        .await;
}

/// Waits for `condition`, ten seconds at most; past them it fails with the
/// daemon's log and trace on `root`, which say what stopped it.
async fn wait_for(root: &Path, condition: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !condition() {
        if Instant::now() >= until {
            let read = |name: &str| {
                std::fs::read_to_string(root.join("consensflow").join(name)).unwrap_or_default()
            };
            panic!(
                "timed out waiting; the daemon's log:\n{}\nits trace:\n{}",
                read("daemon.log"),
                read("events.jsonl")
            );
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn an_exit_the_host_tells_reaches_the_engine_where_it_is_read() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let project = open_project_before_the_start(root.path());
            let (options, app, opened) = app_that_opens_windows(root.path());
            let daemon = start(environment(root.path()), options).await.unwrap();
            ask(&app, "project.resume", json!({ "project": project })).await;
            wait_for(root.path(), || !opened.borrow().is_empty()).await;
            let chief = {
                let found = daemon
                    .parts
                    .ledger
                    .borrow()
                    .project(project)
                    .unwrap()
                    .unwrap();
                found
                    .participants
                    .iter()
                    .find(|p| p.handle == "chief")
                    .unwrap()
                    .id
            };
            let pane = opened.borrow()[0].clone();
            wait_for(root.path(), || daemon.parts.engine.pane(chief).is_some()).await;
            // The host says the window ended: the engine knows before the next frame.
            app.event(
                "pane.exit",
                json!({ "id": pane["id"], "generation": pane["generation"] }),
            );
            assert_eq!(ask(&app, "ping", json!({})).await, json!({ "ok": true }));
            assert!(
                daemon.parts.engine.pane(chief).is_none(),
                "the exit was handled where it was read"
            );
            daemon.stop("SIGTERM");
            daemon.finished().await;
        })
        .await;
}

/// Lets what is ready run, without the clock moving: the telling goes from the
/// engine through the bridge to the app in several turns.
async fn turns() {
    for _ in 0..100 {
        tokio::task::yield_now().await;
    }
}

// On the runtime's paused clock, which moves only when nothing else can run: the
// wait is read to the millisecond and no machine can stretch it. (On the wall
// clock a loaded Windows runner once read 1.09 s from the first change to the
// telling, T-431.)
#[tokio::test(start_paused = true)]
async fn the_page_is_told_the_board_moved_a_hundred_milliseconds_after_the_first_change_and_once() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let project = open_project_before_the_start(root.path());
            // A project the human closed: nothing of it resumes, so nothing
            // moves of itself.
            let database = root.path().join("consensflow").join("consensflow.db");
            let mut ledger = open_ledger(&database, LedgerOptions::default()).unwrap();
            ledger.set_project_state(project, "suspended").unwrap();
            ledger.close().unwrap();
            let (said, exited, handles) = (Said::default(), Rc::default(), Rc::default());
            let (options, app) = options(root.path(), &said, &exited, &handles);
            let told: Rc<RefCell<Vec<tokio::time::Instant>>> = Rc::default();
            let noted = Rc::clone(&told);
            let _heard = app.on_event("state.changed", move |_| {
                noted.borrow_mut().push(tokio::time::Instant::now());
            });
            let daemon = start(environment(root.path()), options).await.unwrap();
            ask(&app, "ping", json!({})).await;
            tokio::time::sleep(Duration::from_millis(300)).await;
            assert!(
                told.borrow().is_empty(),
                "nothing moved: {:?}",
                told.borrow()
            );

            // Two changes a moment apart are one telling, the wait after the first.
            let first = tokio::time::Instant::now();
            daemon.parts.engine.close_project(project).await.unwrap();
            tokio::time::sleep(Duration::from_millis(30)).await;
            daemon.parts.engine.close_project(project).await.unwrap();
            tokio::time::sleep(Duration::from_millis(69)).await;
            assert!(
                told.borrow().is_empty(),
                "told before the wait was over, 99 ms after the first change: {:?}",
                told.borrow()
            );
            tokio::time::sleep(Duration::from_millis(1)).await;
            turns().await;
            let after = {
                let told = told.borrow();
                assert_eq!(told.len(), 1, "100 ms after the first change: {told:?}");
                told[0] - first
            };
            assert_eq!(after, Duration::from_millis(100));
            // Nothing more comes of what came together.
            tokio::time::sleep(Duration::from_millis(300)).await;
            assert_eq!(told.borrow().len(), 1, "{:?}", told.borrow());
            daemon.stop("SIGTERM");
            daemon.finished().await;
        })
        .await;
}
