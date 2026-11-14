//! The field-found issues of a Devin or a Codex worker (`issues/field-found.md`'s Dv and Cx
//! lines), each as its regression's shape on a board whose worker is that harness's stand-in
//! (`fake-devin`, `fake-codex`): the tests run each with its harness's seat alone. The fixes are
//! on today's code, so each is a green case; its plant in `tests/plants/cli/live-matrix.toml`
//! takes the fix back out (T-1207).

use std::time::Duration;

use serde_json::Value;

use crate::live::report::Check;
use crate::matrix::observe::{Bind, Is};
use crate::matrix::oracle::{self, Judged};

use super::kit::{answered, is, run, scenario, since};
use super::{Scenario, Step};

const A: &[Bind] = &[Bind {
    name: "A",
    is: Is::Pool("worker"),
    requester: "chief",
}];

/// The task bound to `name`'s messages, of `kind`.
fn messages<'a>(judged: &'a Judged, name: &str, kind: &str) -> Vec<&'a Value> {
    judged
        .nth(name, 0)
        .and_then(|number| judged.snapshot.task(number))
        .and_then(|task| task["messages"].as_array())
        .map(|all| {
            all.iter()
                .filter(|message| message["kind"] == kind)
                .collect()
        })
        .unwrap_or_default()
}

/// The bodies of the chief's messages that hold `words`.
fn chief_messages_with<'a>(judged: &'a Judged, words: &str) -> Vec<&'a str> {
    judged
        .snapshot
        .chief_inbox
        .iter()
        .filter_map(|message| message["body"].as_str())
        .filter(|body| body.contains(words))
        .collect()
}

/// The check that the one result of the task bound to `name` says `words`.
fn result_is(judged: &Judged, name: &str, words: &str) -> Check {
    let results: Vec<&str> = messages(judged, name, "result")
        .iter()
        .filter_map(|message| message["body"].as_str())
        .collect();
    Check::new(
        name,
        &format!("its result is {words}"),
        results.last().is_some_and(|body| body.contains(words)),
        format!("{results:?}"),
    )
}

/// The file whose presence is the worker's account out of its allowance, in the project's folder.
const SPENT: &str = ".matrix-quota-spent";

/// The step that quits the app and starts it again, the human resuming the project, and its
/// end: A done.
fn restart_then_done() -> Step {
    Step {
        human: Some(|_| Ok(())),
        restarts: true,
        name: "the app is quit mid-turn and started again",
        asks: |_| String::new(),
        stand_in: |_| String::new(),
        reached: |snapshot, bindings| {
            answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot)
        },
    }
}

/// The scenarios of the Devin and Codex workers' field lines.
#[allow(clippy::too_many_lines)]
pub fn seat_fields() -> Vec<Scenario> {
    vec![
        // a-devin-compaction-leaves-a-waiting-task-without-its-answer (T-602): the Devin worker
        // asks, its turn is compacted, and the chief's answer goes in once.
        scenario(
            "field-devin-compaction",
            vec![run!(
                "A asks and is compacted",
                "task add --tier standard :: SLEEP 5 COMPACT CF ask :: Which colour? REPLY SLEEP 20 Reply with exactly: BLUE_DONE",
                |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot)
            )],
            A,
            |judged| {
                let answers = messages(judged, "A", "answer");
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    Check::new(
                        "A",
                        "the chief's answer went into its window once",
                        answers.len() == 1 && answers[0]["state"] != "queued",
                        format!("{:?}", answers.iter().map(|answer| answer["state"].clone()).collect::<Vec<_>>()),
                    ),
                    result_is(judged, "A", "BLUE_DONE"),
                ]
            },
            &[],
            &[],
        ),
        // windows-devin-worker-held-for-background-work: the Devin worker's turn leaves a command
        // that goes on reporting; the task is done with its answer, not held, and the close that
        // ended the command says so.
        scenario(
            "field-devin-command-left",
            vec![run!(
                "A",
                "task add --tier standard :: BACKGROUND 600 Reply with exactly: LEFT_IT",
                // The board settles once the result is in, which can be before the window's close
                // says what it ended: the run waits for that note too (T-1190), and its deadline
                // makes a note that never comes a red of the check below.
                |snapshot, bindings| answered(snapshot, bindings, "A", 0)
                    && crate::matrix::run::settled(snapshot)
                    && snapshot.chief_inbox.iter().any(|message| {
                        message["body"].as_str().is_some_and(|body| body.contains("which its close ended"))
                    })
            )],
            A,
            |judged| {
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    result_is(judged, "A", "LEFT_IT"),
                    // The stand-in's command runs in a shell on both systems (Git Bash on Windows, as
                    // Devin's commands do): a command under `cmd.exe` is no turn's work to the host, and
                    // the close ended it unsaid (T-1207, T-1218).
                    Check::new(
                        "A",
                        "the chief was told what its window's close ended",
                        chief_messages_with(judged, "which its close ended").len() == 1,
                        crate::live::cut(&chief_messages_with(judged, "which its close ended").join(" | "), 200),
                    ),
                ]
            },
            &[],
            &[],
        ),
        // field-reports-2026-10-15 B1: the app restarted mid-turn; the Devin worker's window
        // comes back on its conversation with the permission it had, and its tool writes.
        scenario(
            "field-devin-resumed-writes",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: SLEEP 600 FILE resumed.txt Reply with exactly: T1_DONE",
                    |snapshot, bindings| since(snapshot, bindings, "A", "working").is_some_and(|seconds| seconds >= 3)
                ),
                restart_then_done(),
            ],
            A,
            |judged| {
                let made = std::fs::read_to_string(judged.folder.join("resumed.txt")).unwrap_or_default();
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    Check::new("A", "its resumed turn's tool wrote its file", made == "made by the turn's tool\n", format!("{made:?}")),
                ]
            },
            &[],
            &[],
        )
        .waiting(Duration::from_secs(90)),
        // devin-reopened-window-reads-working: the app restarted mid-turn, the Devin worker's
        // window, stamped as Devin 3000.11 stamps, comes back replaying its last turn; it reads at
        // rest and takes the note to go on, once.
        scenario(
            "field-devin-reopened",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: SLEEP 600 Reply with exactly: T1_DONE",
                    |snapshot, bindings| since(snapshot, bindings, "A", "working").is_some_and(|seconds| seconds >= 3)
                ),
                restart_then_done(),
            ],
            A,
            |judged| {
                let go_on: Vec<&Value> = messages(judged, "A", "note")
                    .into_iter()
                    .filter(|note| note["body"].as_str().is_some_and(|body| body.contains("where you stopped")))
                    .collect();
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    Check::new(
                        "A",
                        "its window was told once to go on where it stopped",
                        go_on.len() == 1 && go_on[0]["state"] != "queued",
                        format!("{:?}", go_on.iter().map(|note| note["state"].clone()).collect::<Vec<_>>()),
                    ),
                ]
            },
            &[],
            &[],
        )
        .with_vars(&[("FAKE_DEVIN_STAMPS", "1")], &[])
        .waiting(Duration::from_secs(90)),
        // devin-quota-turn-end-not-read: the Devin worker's turn ends for its account's quota; the
        // task waits for the account, and once the human logs it in again the work goes on.
        scenario(
            "field-devin-quota",
            vec![
                Step {
                    human: Some(|members| crate::door::write(members.folder().join(SPENT), "")),
                    restarts: false,
                    name: "the worker's account is out",
                    asks: |_| String::new(),
                    stand_in: |_| String::new(),
                    reached: |_, _| true,
                },
                run!(
                    "A",
                    "task add --tier standard :: SLEEP 3 QUOTA-OUT Reply with exactly: WORKER_OK",
                    |snapshot, bindings| is(snapshot, bindings, "A", 0, &["paused", "failed"]) && crate::matrix::run::settled(snapshot)
                ),
                Step {
                    human: Some(|members| {
                        let spent = members.folder().join(SPENT);
                        super::taken(&spent)?;
                        members.page("member.back", serde_json::json!({ "participant": "matrix-worker" })).map(|_| ())
                    }),
                    restarts: false,
                    name: "the human logs the worker into another account, and tries it again",
                    asks: |_| String::new(),
                    stand_in: |_| String::new(),
                    reached: |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot),
                },
            ],
            A,
            |judged| {
                let waits = chief_messages_with(judged, " waits with @");
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    Check::new("A", "the chief was told once that it waits for the account", waits.len() == 1, format!("{} note(s)", waits.len())),
                    Check::new("A", "nothing said it failed", chief_messages_with(judged, " failed").is_empty(), "the chief's messages"),
                ]
            },
            &[],
            &[],
        )
        .with_vars(&[("CF_TEST_QUOTA_OUT", "matrix-worker"), ("CF_TEST_QUOTA_FILE", SPENT)], &[]),
        // a-slow-login-shell-costs-the-path: the app's daemon starts on the Finder's PATH, its login
        // shell too slow; the Codex worker, found only in the system's folder of programs as
        // Homebrew's is, still opens and answers.
        scenario(
            "field-codex-slow-login-shell",
            vec![run!(
                "A",
                "task add --tier standard :: Reply with exactly: CODEX_OK",
                |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot)
            )],
            A,
            |judged| {
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    result_is(judged, "A", "CODEX_OK"),
                    Check::new(
                        "A",
                        "nothing said its harness is not installed",
                        chief_messages_with(judged, "not installed").is_empty() && chief_messages_with(judged, "the launch failed").is_empty(),
                        "the chief's messages",
                    ),
                ]
            },
            &[],
            &[],
        )
        .started_by_the_finder(),
    ]
}
