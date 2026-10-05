//! A restart: what was open comes back and its window opens with the environment
//! the app expects, an exit the host tells reaches the engine where it is read,
//! and the page is told the board moved a hundred milliseconds after the first
//! change, once.

use cf_ledger::{NewChief, NewProject};

use super::*;

fn open_project_before_the_start(root: &Path) -> i64 {
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
        })
        .unwrap();
    ledger.close().unwrap();
    project.id
}

#[tokio::test]
async fn what_was_open_comes_back_and_its_window_opens_with_the_environment_the_app_expects() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let project = open_project_before_the_start(root.path());
            let (said, exited, handles) = (Said::default(), Rc::default(), Rc::default());
            let (options, app) = options(root.path(), &said, &exited, &handles);
            let opened: Rc<RefCell<Vec<Value>>> = Rc::default();
            let events: Rc<RefCell<Vec<(String, Value)>>> = Rc::default();
            let heard = Rc::clone(&opened);
            app.on("pane.open", move |_, body| {
                heard.borrow_mut().push(body.clone());
                async move { Ok(json!({ "ok": true, "id": body["id"], "generation": body["generation"] })) }
            });
            let told = Rc::clone(&events);
            app.on_event("state.changed", move |body| told.borrow_mut().push(("state.changed".to_owned(), body.clone())));
            let daemon = start(environment(root.path()), options).await.unwrap();

            // The resume opens the chief's window through the host.
            wait_for(root.path(), || !opened.borrow().is_empty()).await;
            let open = opened.borrow()[0].clone();
            assert_eq!(open["id"], format!("p{project}-chief"));
            let env = &open["env"];
            let url = daemon.handle().url.trim_end_matches('/').to_owned();
            assert_eq!(env["CONSENSFLOW_URL"], url.as_str());
            assert_eq!(env["CONSENSFLOW_PROJECT"], project.to_string());
            assert_eq!(env["CONSENSFLOW_PARTICIPANT"], "chief");
            assert_eq!(env["CONSENSFLOW_NODE"], "/the/node/the/app/named");
            let delimiter = if cfg!(windows) { ';' } else { ':' };
            let bin = root.path().join("bundle").join("bin");
            assert_eq!(
                env["PATH"],
                format!("{}{delimiter}{}", bin.display(), root.path().join("bin").display())
            );
            assert!(env["CONSENSFLOW_TOKEN"].as_str().is_some_and(|token| token.len() == 64));
            // The keys the pane host reads, in the order Node sent them.
            let keys: Vec<&str> = open.as_object().unwrap().keys().map(String::as_str).collect();
            assert_eq!(keys, ["id", "generation", "cwd", "argv", "env", "dropEnv"]);
            assert_eq!(open["cwd"], root.path().join("workspace").to_string_lossy().as_ref());
            // The chief's role text names the cf of this window.
            let argv: Vec<&str> = open["argv"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
            let at = argv.iter().position(|arg| *arg == "--append-system-prompt-file").expect("a role file");
            let role = std::fs::read_to_string(argv[at + 1]).unwrap();
            // As the daemon names it: `cf.exe`, spelled with `/`, on Windows.
            let cf = machine::bundle_of(&bin.join("cf")).pane_cf;
            assert!(role.contains(&format!("Here `cf` is {cf}")), "{role}");

            // The page is told the board changed, once for what came together.
            wait_for(root.path(), || !events.borrow().is_empty()).await;
            assert_eq!(events.borrow()[0], ("state.changed".to_owned(), json!({ "reason": "core" })));
            // The ledger's own events are in the trace as they happen: the
            // start suspended the project for a resume, and the resume opened it.
            let trace = std::fs::read_to_string(root.path().join("consensflow").join("events.jsonl")).unwrap();
            let suspended = r#""kind":"project.state","data":{"from":"open","to":"suspended","resumeOnStart":true}"#;
            let opened_again = r#""kind":"project.state","data":{"from":"suspended","to":"open"}"#;
            let at = |needle: &str| trace.lines().position(|line| line.contains(needle));
            assert!(at(suspended) < at(opened_again) && at(opened_again).is_some(), "{trace}");
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
            let (said, exited, handles) = (Said::default(), Rc::default(), Rc::default());
            let (options, app) = options(root.path(), &said, &exited, &handles);
            let opened: Rc<RefCell<Vec<Value>>> = Rc::default();
            let heard = Rc::clone(&opened);
            app.on("pane.open", move |_, body| {
                heard.borrow_mut().push(body.clone());
                async move {
                    Ok(json!({ "ok": true, "id": body["id"], "generation": body["generation"] }))
                }
            });
            let daemon = start(environment(root.path()), options).await.unwrap();
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

#[tokio::test]
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
            let told: Rc<RefCell<Vec<Instant>>> = Rc::default();
            let noted = Rc::clone(&told);
            let _heard = app.on_event("state.changed", move |_| {
                noted.borrow_mut().push(Instant::now());
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
            let first = Instant::now();
            daemon.parts.engine.close_project(project).await.unwrap();
            tokio::time::sleep(Duration::from_millis(30)).await;
            daemon.parts.engine.close_project(project).await.unwrap();
            wait_for(root.path(), || !told.borrow().is_empty()).await;
            // Nothing more comes of what came together.
            tokio::time::sleep(Duration::from_millis(300)).await;
            let after = {
                let told = told.borrow();
                assert_eq!(told.len(), 1, "{told:?}");
                told[0] - first
            };
            assert!(
                after >= Duration::from_millis(100) && after < Duration::from_millis(600),
                "{after:?}"
            );
            daemon.stop("SIGTERM");
            daemon.finished().await;
        })
        .await;
}
