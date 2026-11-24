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
    // The launch's own settings file: its open waits until the test reads the other end.
    fs::create_dir_all(home.launch_folder()).unwrap();
    let gate = path::join(&[&home.launch_folder(), "settings.json"]);
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
