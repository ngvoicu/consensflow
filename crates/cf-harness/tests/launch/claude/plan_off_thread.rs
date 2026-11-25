//! A launch's plan reads the project's folder and its ancestors and writes the launch's files
//! (T-2203, round 2): on a slow disk, a folder macOS asks the person about or a share that
//! answers late, each waits with no limit, and while it did it held the daemon's one thread,
//! the passes, the API and the heartbeat with it.
//!
//! A file that is a named pipe stands for the slow one: opening it waits until the test opens
//! its other end. A plan that waits on it on its own thread keeps every timer of the engine
//! late by that much; a plan that waits for it off the thread keeps them.

use std::io::Read as _;
use std::time::{Duration, Instant};

use tokio::task::LocalSet;

use super::*;

/// How long the folder or the file is slow.
const HELD: Duration = Duration::from_millis(500);

/// How late a 10 ms timer may be while the plan waits.
const BUDGET: Duration = Duration::from_millis(150);

/// What the plan did, and how long the thread was still while it did.
struct Waited {
    plan: Result<Prepared, String>,
    still: Duration,
    took: Duration,
}

fn plan_beside_a_ticker(home: &Home, directory: &str) -> Waited {
    let adapter = home.adapter();
    let id = LaunchId::new(LAUNCH).unwrap();
    let request = Request::default();
    let launch = Launch {
        id: &id,
        project: 1,
        handle: request.handle,
        role: request.role,
        directory,
        resume: None,
        previous_gone: true,
        message: request.message.as_deref(),
        agent: request.agent,
        instructions: request.instructions,
        specs: None,
        compaction: None,
        advisor_off: false,
        suggestion_off: false,
        first_message_ms: FIRST_MESSAGE_MS,
        starts_given_up: 0,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    LocalSet::new().block_on(&runtime, async {
        let still = Rc::new(Cell::new(Duration::ZERO));
        let seen = Rc::clone(&still);
        let ticker = tokio::task::spawn_local(async move {
            let mut last = Instant::now();
            loop {
                tokio::time::sleep(Duration::from_millis(10)).await;
                let now = Instant::now();
                seen.set(
                    seen.get()
                        .max((now - last).saturating_sub(Duration::from_millis(10))),
                );
                last = now;
            }
        });
        // The ticker is running before the plan begins.
        tokio::time::sleep(Duration::from_millis(40)).await;
        let began = Instant::now();
        let plan = adapter.prepare(&launch).await;
        let took = began.elapsed();
        // The ticker says how long it stood still when it next runs.
        tokio::time::sleep(Duration::from_millis(40)).await;
        ticker.abort();
        Waited {
            plan,
            still: still.get(),
            took,
        }
    })
}

#[test]
fn a_plan_that_waits_on_a_folder_of_the_project_leaves_the_thread_going() {
    let home = Home::new();
    let project = path::join(&[&home.root, "project"]);
    fs::create_dir_all(&project).unwrap();
    // The `.mcp.json` of the project's parent: read for each ancestor, as Claude Code does.
    let gate = path::join(&[&home.root, ".mcp.json"]);
    nix::unistd::mkfifo(
        Path::new(&gate),
        nix::sys::stat::Mode::from_bits_truncate(0o600),
    )
    .unwrap();
    let opener = std::thread::spawn(move || {
        std::thread::sleep(HELD);
        fs::write(&gate, "{}").unwrap();
    });
    let waited = plan_beside_a_ticker(&home, &project);
    opener.join().unwrap();
    waited.plan.as_ref().unwrap();
    assert!(
        waited.took >= HELD / 2,
        "the plan waited for the folder ({:?})",
        waited.took
    );
    assert!(
        waited.still < BUDGET,
        "the thread stood still for {:?} while the folder was read",
        waited.still
    );
}

#[test]
fn a_plan_whose_file_is_slow_to_write_leaves_the_thread_going_and_the_write_is_whole() {
    let home = Home::new();
    let project = path::join(&[&home.root, "project"]);
    fs::create_dir_all(&project).unwrap();
    // The file the launch's own settings are written beside: its open waits until the test
    // reads the other end.
    fs::create_dir_all(home.launch_folder()).unwrap();
    let gate = format!(
        "{}.{}.tmp",
        path::join(&[&home.launch_folder(), "settings.json"]),
        std::process::id()
    );
    nix::unistd::mkfifo(
        Path::new(&gate),
        nix::sys::stat::Mode::from_bits_truncate(0o600),
    )
    .unwrap();
    let reader = std::thread::spawn(move || {
        std::thread::sleep(HELD);
        let mut text = String::new();
        fs::File::open(&gate)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        text
    });
    let waited = plan_beside_a_ticker(&home, &project);
    let written = reader.join().unwrap();
    waited.plan.as_ref().unwrap();
    assert!(
        written.contains("bypassPermissions"),
        "the file was written whole: {written}"
    );
    assert!(
        waited.took >= HELD / 2,
        "the plan waited for the file ({:?})",
        waited.took
    );
    assert!(
        waited.still < BUDGET,
        "the thread stood still for {:?} while the file was written",
        waited.still
    );
}

/// The `.mcp.json` above a home's project, as a named pipe: the plan that reads it waits until
/// the test writes it.
fn held_folder(home: &Home) -> (String, String) {
    let project = path::join(&[&home.root, "project"]);
    fs::create_dir_all(&project).unwrap();
    let gate = path::join(&[&home.root, ".mcp.json"]);
    mkfifo(&gate);
    (project, gate)
}

fn mkfifo(at: &str) {
    nix::unistd::mkfifo(
        Path::new(at),
        nix::sys::stat::Mode::from_bits_truncate(0o600),
    )
    .unwrap();
}

/// A launch of `home`'s project, as the engine makes one.
fn launch_of<'a>(id: &'a LaunchId, directory: &'a str, instructions: &'a str) -> Launch<'a> {
    let request = Request::default();
    Launch {
        id,
        project: 1,
        handle: request.handle,
        role: request.role,
        directory,
        resume: None,
        previous_gone: true,
        message: None,
        agent: request.agent,
        instructions,
        specs: None,
        compaction: None,
        advisor_off: false,
        suggestion_off: false,
        first_message_ms: FIRST_MESSAGE_MS,
        starts_given_up: 0,
    }
}

/// One thread, the daemon's.
fn on_one_thread<T>(scene: impl std::future::Future<Output = T>) -> T {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    LocalSet::new().block_on(&runtime, scene)
}

/// One project's folder that stops answering does not keep the window of another from being
/// planned (T-2232, finding 3).
#[test]
fn a_launch_of_another_project_is_planned_while_one_waits_on_its_folder() {
    let (held, other) = (Home::new(), Home::new());
    let (project, gate) = held_folder(&held);
    let other_project = path::join(&[&other.root, "project"]);
    fs::create_dir_all(&other_project).unwrap();
    let (held_adapter, other_adapter) = (held.adapter(), other.adapter());
    let id = LaunchId::new(LAUNCH).unwrap();
    on_one_thread(async {
        let first = launch_of(&id, &project, "role");
        let second = launch_of(&id, &other_project, "role");
        let waiting = held_adapter.prepare(&first);
        let planned = other_adapter.prepare(&second);
        tokio::pin!(waiting);
        let planned = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! {
                biased;
                _ = &mut waiting => panic!("the held launch ended"),
                planned = planned => planned,
            }
        })
        .await;
        // Let the held launch end whatever the other came to.
        std::thread::spawn(move || fs::write(gate, "{}").unwrap());
        planned
            .expect("the other project's launch waited behind the held one")
            .unwrap();
        waiting.await.unwrap();
    });
}

/// A plan that does not end in its time fails the launch, in words that name what it was doing
/// (T-2232, finding 3). The stand-in waits 3 s, well past the 300 ms this plan is given.
#[test]
fn a_plan_that_does_not_end_in_its_time_fails_naming_its_step() {
    let mut home = Home::new();
    home.vars
        .push(("CONSENSFLOW_PLAN_DEADLINE_MS".to_owned(), "300".to_owned()));
    let (project, gate) = held_folder(&home);
    let adapter = home.adapter();
    let id = LaunchId::new(LAUNCH).unwrap();
    let opener = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));
        fs::write(gate, "{}").unwrap();
    });
    let (answer, took) = on_one_thread(async {
        let launch = launch_of(&id, &project, "role");
        let began = Instant::now();
        let answer = adapter.prepare(&launch).await.map(|_| ());
        (answer, began.elapsed())
    });
    opener.join().unwrap();
    let said = answer.expect_err("the launch did not fail");
    assert!(took < Duration::from_secs(2), "given up after {took:?}");
    assert_eq!(
        said,
        "reading the project's folder and writing the launch's files took more than 1 s"
    );
}

/// The trust watch's read of the project's local settings, made as its window is watched, does
/// not hold the thread (T-2232, finding 1: where the owner's freeze sat).
#[test]
fn the_trust_watch_reads_the_project_settings_off_the_thread() {
    let home = Home::new();
    let project = path::join(&[&home.root, "project"]);
    fs::create_dir_all(path::join(&[&project, ".claude"])).unwrap();
    let adapter = home.adapter();
    let id = LaunchId::new(LAUNCH).unwrap();
    let window = on_one_thread(async {
        let launch = launch_of(&id, &project, "role");
        adapter.prepare(&launch).await.unwrap().window
    });
    let gate = path::join(&[&project, ".claude", "settings.local.json"]);
    mkfifo(&gate);
    let opener = std::thread::spawn(move || {
        std::thread::sleep(HELD);
        fs::write(gate, r#"{"enabledMcpjsonServers":["one"]}"#).unwrap();
    });
    let (chosen, still) = on_one_thread(async {
        let still = Rc::new(Cell::new(Duration::ZERO));
        let seen = Rc::clone(&still);
        let ticker = tokio::task::spawn_local(async move {
            let mut last = Instant::now();
            loop {
                tokio::time::sleep(Duration::from_millis(10)).await;
                let now = Instant::now();
                seen.set(
                    seen.get()
                        .max((now - last).saturating_sub(Duration::from_millis(10))),
                );
                last = now;
            }
        });
        tokio::time::sleep(Duration::from_millis(40)).await;
        let chosen = window.servers_chosen().await.unwrap();
        tokio::time::sleep(Duration::from_millis(40)).await;
        ticker.abort();
        (chosen, still.get())
    });
    opener.join().unwrap();
    assert_eq!(chosen.used, ["one"]);
    assert!(still < BUDGET, "the thread stood still for {still:?}");
}

/// A launch's own file is written beside and renamed into place: a stop while it is written
/// leaves no half a file (T-2232, finding 7). The stand-in holds the file written beside.
#[test]
fn a_launch_file_is_in_place_only_once_it_is_whole() {
    let home = Home::new();
    let project = path::join(&[&home.root, "project"]);
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(home.launch_folder()).unwrap();
    let settings = path::join(&[&home.launch_folder(), "settings.json"]);
    let beside = format!("{settings}.{}.tmp", std::process::id());
    mkfifo(&beside);
    let seen = {
        let (settings, beside) = (settings.clone(), beside.clone());
        std::thread::spawn(move || {
            std::thread::sleep(HELD);
            let early = Path::new(&settings).exists();
            if early {
                // Written in place: nothing opens the file beside to write it.
                return (early, String::new());
            }
            let mut text = String::new();
            fs::File::open(beside)
                .unwrap()
                .read_to_string(&mut text)
                .unwrap();
            (early, text)
        })
    };
    let adapter = home.adapter();
    let id = LaunchId::new(LAUNCH).unwrap();
    on_one_thread(async {
        let launch = launch_of(&id, &project, "role");
        adapter.prepare(&launch).await.map(|_| ())
    })
    .unwrap();
    let (early, text) = seen.join().unwrap();
    assert!(!early, "the file was in place while it was being written");
    assert!(text.contains("bypassPermissions"), "{text}");
    assert!(Path::new(&settings).exists(), "it is in place once written");
}
