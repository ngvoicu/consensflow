//! A window whose open missed its deadline was abandoned, not stopped, and worked on
//! unseen.
//!
//! Seen on the kluris board on 2026-10-26, on 3.0.6-alpha.2, with the Mac at a load
//! of about 100 (T-668's diagnosis, `operations/work-in-flight/results/
//! codex-deadline-orphan-T-668.md` in consensflow-sme). A task message went to a Codex
//! reviewer, and the pane host took longer than the open's minute to answer
//! `pane.open`. The daemon gave up on the window at its deadline: it failed the
//! message and the task, revoked the window's token and forgot its files, but asked
//! the host to end nothing, and kept no record of the window. The host went on and
//! started it three seconds later. That window ran the brief its command line carried
//! for fourteen minutes, and its answer reached nobody. The same task, sent again,
//! opened a window on the same Codex thread, which Codex refused as open in another
//! app: the window given up on still held it.
//!
//! What should have happened: a window whose open was not answered stays the daemon's
//! until the host proves it never started or that its processes ended. The daemon asks
//! the host to end it; a host asked to end a window it has not opened yet never opens
//! it; and the task sent again waits for that, then takes its message once, on a
//! thread nobody else holds.
//!
//! The rig holds the open where the loaded Mac held it (`Rig::hold_open`): the
//! request before the host starts anything, or the host's answer once the program is
//! started; the daemon is given three seconds to open a window, not a minute
//! (`CONSENSFLOW_OPEN_DEADLINE_MS`). The stand-in Codex holds a thread as Codex does
//! (one server at a time, `fake_codex/agent.rs`). Each case checks that every process
//! of the window given up on has ended, not only what the board says: a brief that no
//! longer rides on the command line (`first-message-off-argv`) keeps the window from
//! working on it, and still leaves the window running and holding its thread.

use std::time::Duration;

use cf_e2e::files;
use cf_e2e::live::members::{descending_from, Identity, Machine, ThisMachine};
use cf_e2e::rig::{Config, Held, Project, Rig, OPEN};
use cf_e2e::rollout;
use serde_json::{json, Value};

use crate::{config, config_with_codex, secs, Outcome};

/// How long the daemon gives the pane host to answer an open, in milliseconds.
const OPEN_DEADLINE_MS: &str = "3000";

/// How long a window's processes have once the daemon gave up on it: the host gives a
/// program ten seconds to leave when asked (`CLOSE_GRACE`), and the daemon's own wait
/// is three.
const ENDED_WITHIN: Duration = Duration::from_secs(30);

/// How long a window the host was let open late is given to start, where it would.
const STARTS_WITHIN: Duration = Duration::from_secs(8);

/// The first brief, whose window is given up on.
const FIRST: &str = "FIRST_BRIEF_OK";

/// What the task is sent again with: the task message that carries it holds T-1's
/// brief first, which the stand-ins answer.
const AGAIN: &str = "SENT_AGAIN_OK";

/// A rig whose daemon gives the host three seconds to open a window.
fn start(config: Config) -> cf_e2e::Result<Rig> {
    Rig::start(
        config
            .var("CONSENSFLOW_OPEN_DEADLINE_MS", OPEN_DEADLINE_MS)
            .var("CONSENSFLOW_LAUNCH_TIMEOUT_MS", "30000"),
    )
}

/// The project of a case, its chief's window and the worker's window id.
struct Board<'a> {
    rig: &'a Rig,
    project: Project<'a>,
    chief: Value,
    tier: String,
}

impl<'a> Board<'a> {
    /// A project whose chief is the stand-in Claude, with a worker added (the rig's
    /// roster says on which harness).
    fn open(rig: &'a Rig) -> Result<Self, Box<dyn std::error::Error>> {
        let project = Project::open(rig, "chief", json!({}))?;
        let added = project.add_member("worker")?;
        let tier = added["member"]["tier"]
            .as_str()
            .unwrap_or("standard")
            .to_owned();
        let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
        Ok(Self {
            rig,
            project,
            chief,
            tier,
        })
    }

    /// The start of the id of every window of the worker's.
    fn worker(&self) -> String {
        format!("p{}-worker", self.project.id())
    }

    /// The chief gives the worker T-1, which says `words`.
    fn dispatch(&self, words: &str) -> cf_e2e::Result {
        self.rig.tell(
            self.project.id(),
            &format!("DISPATCH --tier {} Reply with exactly: {words}", self.tier),
        )
    }

    /// The chief sends T-1 back to the worker with `words`, as the note of its failure
    /// says to.
    fn reopen(&self, words: &str) -> Result<(), Box<dyn std::error::Error>> {
        let ran = self.rig.cf_in_window(
            &self.chief,
            [
                "task",
                "reopen",
                "T-1",
                &format!("Reply with exactly: {words}"),
            ],
        )?;
        assert_eq!(ran.code, Some(0), "{ran}");
        Ok(())
    }

    fn fails(&self) -> cf_e2e::Result {
        self.rig.wait_for("T-1 to fail", secs(90), || {
            Ok(self.project.task(1)?["state"] == "failed")
        })
    }

    /// How many results of T-1 the chief was sent.
    fn results(&self) -> cf_e2e::Result<usize> {
        Ok(self
            .project
            .inbox("chief")?
            .iter()
            .filter(|message| message["kind"] == "result")
            .count())
    }

    /// Waits for T-1's result number `count`.
    fn answered(&self, count: usize) -> cf_e2e::Result {
        self.rig
            .wait_for(&format!("T-1's result number {count}"), secs(90), || {
                Ok(self.results()? >= count)
            })
    }

    /// The worker's windows the daemon asked for, in order.
    fn worker_windows(&self) -> Vec<Value> {
        let worker = self.worker();
        self.rig
            .open_frames()
            .into_iter()
            .filter(|frame| {
                frame["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with(&worker))
            })
            .collect()
    }
}

/// Whether the host answered an open of `window` (`id`, `generation`) with a process.
fn started(rig: &Rig, window: &Value) -> bool {
    rig.window_pids()
        .iter()
        .any(|pid| pid["id"] == window["id"] && pid["generation"] == window["generation"])
}

/// The window's own process and every process that descends from it, as they run now.
fn tree_of(root: Identity) -> cf_e2e::Result<Vec<Identity>> {
    let all = ThisMachine.processes()?;
    let mut tree = vec![root];
    tree.extend(
        descending_from(&all, root)
            .into_iter()
            .map(|process| process.identity),
    );
    Ok(tree)
}

/// Those of `tree` that still run.
fn still_running(tree: &[Identity]) -> cf_e2e::Result<Vec<Identity>> {
    let mut running = Vec::new();
    for identity in tree {
        if ThisMachine.identity(identity.pid)? == Some(*identity) {
            running.push(*identity);
        }
    }
    Ok(running)
}

/// The window held: its `pane.open`'s `id` and `generation`.
fn held(rig: &Rig) -> cf_e2e::Result<Value> {
    rig.wait_for("the worker's open to be held", secs(30), || {
        Ok(rig.open_held().is_some())
    })?;
    Ok(rig.open_held().unwrap_or_default())
}

/// The processes of the window held by its answer: the host started it, and the
/// answer that says which process it is waits in the rig.
fn held_tree(rig: &Rig, window: &Value) -> cf_e2e::Result<Vec<Identity>> {
    rig.wait_for("the held window's process", secs(10), || {
        Ok(rig.window_process(window).is_some())
    })?;
    let root = rig
        .window_process(window)
        .unwrap_or(Identity { pid: 0, started: 0 });
    tree_of(root)
}

#[test]
fn an_open_given_up_on_before_the_host_started_it_never_starts_and_the_task_sent_again_takes_its_message_once(
) -> Outcome {
    let rig = start(config())?;
    let board = Board::open(&rig)?;
    rig.hold_open(&board.worker(), Held::TheRequest);
    board.dispatch(FIRST)?;
    let given_up = held(&rig)?;
    board.fails()?;
    assert!(
        !started(&rig, &given_up),
        "the host had the open of {given_up} only once the case let it go"
    );

    board.reopen(AGAIN)?;
    board.answered(1)?;
    let windows = board.worker_windows();
    assert_eq!(windows.len(), 2, "{windows:?}");
    let again = &windows[1];
    assert_ne!(again["generation"], given_up["generation"], "{again}");

    // The open the loaded host took so long over reaches it at last, after the window
    // that took the task sent again.
    assert!(rig.let_open_go());
    let late = rig.wait_for("the late open to start a window", STARTS_WITHIN, || {
        Ok(started(&rig, &given_up)
            || !rig
                .transcript(&session_of(&given_up_frame(&rig, &given_up)))
                .is_empty())
    });
    assert!(
        late.is_err(),
        "the window the daemon gave up on started once the host had its open, and ran its brief: {given_up}"
    );
    // The task sent again was taken once, in the window that opened for it.
    assert_eq!(board.results()?, 1);
    assert_eq!(
        rig.transcript(&session_of(again))
            .matches("Reply with exactly: SENT_AGAIN_OK")
            .count(),
        1
    );
    rig.close()?;
    Ok(())
}

#[test]
fn an_open_whose_answer_came_after_its_deadline_has_every_process_of_its_window_ended() -> Outcome {
    let rig = start(config())?;
    let board = Board::open(&rig)?;
    rig.hold_open(&board.worker(), Held::TheAnswer);
    board.dispatch(FIRST)?;
    let given_up = held(&rig)?;
    let tree = held_tree(&rig, &given_up)?;
    board.fails()?;
    rig.wait_for(
        "every process of the window given up on to end",
        ENDED_WITHIN,
        || Ok(still_running(&tree)?.is_empty()),
    )?;

    board.reopen(AGAIN)?;
    board.answered(1)?;
    // The answer of the window given up on comes after the newer window's, and is
    // nothing to it.
    assert!(rig.let_open_go());
    std::thread::sleep(secs(2));
    assert_eq!(board.results()?, 1);
    let windows = board.worker_windows();
    assert_eq!(windows.len(), 2, "{windows:?}");
    assert_eq!(
        rig.transcript(&session_of(&windows[1]))
            .matches("Reply with exactly: SENT_AGAIN_OK")
            .count(),
        1
    );
    rig.close()?;
    Ok(())
}

/// The `pane.open` of the window `window` (`id`, `generation`): what it was opened with.
fn given_up_frame(rig: &Rig, window: &Value) -> Value {
    rig.open_frames()
        .into_iter()
        .find(|frame| frame["id"] == window["id"] && frame["generation"] == window["generation"])
        .unwrap_or_default()
}

/// The conversation the stand-in Claude of the window `frame` opens records.
fn session_of(frame: &Value) -> String {
    let argv = frame["argv"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    argv.iter()
        .position(|word| word == "--session-id")
        .and_then(|at| argv.get(at + 1))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The roster of the Codex case: the chief on the stand-in Claude, the worker on the
/// stand-in Codex.
fn write_codex_roster(rig: &Rig) -> cf_e2e::Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "worker", "kind": "codex", "model": "fake", "workTier": "standard" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))
}

/// What the worker's thread was told by its user, in its one rollout.
fn codex_told(rig: &Rig) -> cf_e2e::Result<Vec<String>> {
    let home = std::path::PathBuf::from(rig.var("CODEX_HOME").unwrap_or_default());
    let mut told = Vec::new();
    for found in rollout::rollouts(&home) {
        told.extend(rollout::said_by(&rollout::read(&found)?, "user"));
    }
    Ok(told)
}

// The stand-in Codex is a script, so Unix's.
#[cfg(unix)]
#[test]
fn a_codex_thread_a_window_given_up_on_held_is_free_for_the_task_sent_again() -> Outcome {
    let rig = start(config_with_codex())?;
    write_codex_roster(&rig)?;
    let board = Board::open(&rig)?;
    // T-1's first window works and closes: the thread has a rollout to resume.
    board.dispatch(FIRST)?;
    board.answered(1)?;
    rig.wait_for("the worker's first window to close", secs(60), || {
        Ok(board.project.lane(
            board.project.task(1)?["assignee"]
                .as_str()
                .unwrap_or_default(),
        )?["activity"]["state"]
            == "closed")
    })?;

    // T-1 sent back: its window resumes the thread, and its open is answered late.
    rig.hold_open(&board.worker(), Held::TheAnswer);
    board.reopen("SECOND_OK")?;
    let given_up = held(&rig)?;
    let tree = held_tree(&rig, &given_up)?;
    assert!(
        tree.len() > 1,
        "the supervisor runs Codex's server and TUI: {tree:?}"
    );
    board.fails()?;

    // Sent back again at once, it resumes the same thread, which nobody holds now: the
    // window given up on held it, and Codex gave the kluris retry the thread read-only.
    board.reopen(AGAIN)?;
    board.answered(2)?;
    assert_eq!(
        still_running(&tree)?,
        Vec::<Identity>::new(),
        "every process of the window given up on ended"
    );
    assert!(rig.let_open_go());
    std::thread::sleep(secs(2));
    assert_eq!(board.results()?, 2);
    let words = format!("Reply with exactly: {AGAIN}");
    assert_eq!(
        codex_told(&rig)?
            .iter()
            .filter(|said| said.contains(&words))
            .count(),
        1,
        "the thread took the task sent again once"
    );
    rig.close()?;
    Ok(())
}
