//! LMX-14, window life: the field-found issues of a window's own life on a board (background work
//! at a turn's end, a Close and Resume, an app restarted mid-turn, an allowance used up, a window
//! that closes by itself, a trust question, a stop pressed into a turn that is over, a chief
//! switched), each as its regression's shape on Claude-shaped stand-ins (the brain's
//! `issues/field-found.md` names each line's scenario). The fixes are on today's code, so each
//! is a green case; its plant in `tests/plants/cli/live-matrix.toml` takes the fix back out.

use std::time::Duration;

use serde_json::{json, Value};

use crate::live::members::Members;
use crate::live::report::Check;
use crate::matrix::observe::{Bind, Is, Snapshot};
use crate::matrix::oracle::{self, Judged};
use crate::matrix::seating::Role;
use crate::Result;

use super::kit::{answered, is, run, scenario, since};
use super::{Scenario, Step};

const A: &[Bind] = &[Bind {
    name: "A",
    is: Is::Pool("worker"),
    requester: "chief",
}];

/// The chief's messages about the task bound to `name`, of `kind`.
fn about<'a>(judged: &'a Judged, name: &str, kind: &str) -> Vec<&'a Value> {
    let number = judged.nth(name, 0);
    judged
        .snapshot
        .chief_inbox
        .iter()
        .filter(|message| {
            message["kind"] == kind && number.is_some_and(|n| message["taskNumber"] == n)
        })
        .collect()
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

/// The check that no message to the chief holds any of `words`.
fn told_none_of(judged: &Judged, who: &str, what: &str, words: &[&str]) -> Check {
    let found: Vec<&str> = words
        .iter()
        .flat_map(|words| chief_messages_with(judged, words))
        .collect();
    Check::new(
        who,
        what,
        found.is_empty(),
        crate::live::cut(&found.join(" | "), 240),
    )
}

/// The user's turns of the chief's own record that hold `words`.
fn chief_given(judged: &Judged, words: &str) -> usize {
    judged
        .records
        .items(judged.seating.seat(Role::Chief).kind(), "chief")
        .unwrap_or_default()
        .iter()
        .filter(|item| item.role == "user" && item.text.contains(words))
        .count()
}

/// The pane of the lane `handle`, as the board names it.
fn pane_of(members: &Members, handle: &str) -> Result<Value> {
    Ok(members.lane(handle)?["pane"].clone())
}

/// The handle of the window that has task 1 (A, the first task of these scenarios).
fn a_window(members: &Members) -> Result<String> {
    Ok(members.task(1)?["assignee"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

/// Whether the chief's lane is at rest, and whether it shows no window.
fn chief_lane(snapshot: &Snapshot) -> Option<&Value> {
    snapshot
        .lanes
        .iter()
        .find(|lane| lane["participant"]["handle"] == "chief")
}

/// A step of the human's on the page.
fn human(
    name: &'static str,
    act: fn(&Members) -> Result<()>,
    reached: fn(&Snapshot, &crate::matrix::observe::Bindings) -> bool,
) -> Step {
    Step {
        human: Some(act),
        restarts: false,
        name,
        asks: |_| String::new(),
        stand_in: |_| String::new(),
        reached,
    }
}

/// What makes a turn's end of a worker's a hold, in the field's words: a mutation run in the
/// background (another Mac, 2026-10-15, A2).
const GOING: &str = "The mutation run is still going in the background. I will write up the result once it finishes";
const REPORT: &str = "All 33 mutants are killed.";

/// The file whose presence is an account out of its allowance, beside the windows' folder (the
/// stand-in reads it relative to its own: the project's folder).
const SPENT: &str = ".matrix-quota-spent";

/// The scenarios of LMX-14.
#[allow(clippy::too_many_lines)]
pub fn window_life() -> Vec<Scenario> {
    vec![
        // A1: a worker's build leaves a daemon of its own running, as Gradle does: the task is
        // done with the build, and no background work is told.
        scenario(
            "field-builds-daemon",
            vec![run!(
                "A",
                "task add --tier standard :: BUILD 90 BUILD_OK",
                |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot)
            )],
            A,
            |judged| {
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    Check::new(
                        "A",
                        "its result is the build's words",
                        about(judged, "A", "result").first().is_some_and(|message| message["body"] == "BUILD_OK"),
                        format!("{:?}", about(judged, "A", "result").first().map(|message| message["body"].clone())),
                    ),
                    Check::new("A", "nothing was told of it besides its result", about(judged, "A", "note").is_empty(), format!("{} note(s)", about(judged, "A", "note").len())),
                    told_none_of(judged, "A", "no message said background work runs", &["background work", "background job"]),
                ]
            },
            &[],
            &[],
        ),
        // A2 and A3: a worker leaves its mutation run in the background and ends its turn; the
        // turn is no result: the chief is told it goes on, then given the result once the run ends.
        scenario(
            "field-run-in-background",
            vec![run!(
                "A",
                "task add --tier standard :: CLAUDE-JOB 16 The mutation run is still going in the background. I will write up the result once it finishes :: All 33 mutants are killed.",
                |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot)
            )],
            A,
            |judged| {
                let kinds: Vec<String> = judged
                    .nth("A", 0)
                    .and_then(|number| judged.snapshot.task(number))
                    .and_then(|task| task["messages"].as_array().cloned())
                    .unwrap_or_default()
                    .iter()
                    .filter(|message| message["recipient"] == "chief" || message["to"] == "chief")
                    .filter_map(|message| message["kind"].as_str().map(str::to_owned))
                    .filter(|kind| kind != "task")
                    .collect();
                let notes = about(judged, "A", "note");
                let results = about(judged, "A", "result");
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    Check::new(
                        "A",
                        "the turn that left the run going was told as a note, not given as its result",
                        notes.len() == 1 && notes[0]["body"].as_str().is_some_and(|body| body.contains(GOING)),
                        format!("{} note(s); thread {kinds:?}", notes.len()),
                    ),
                    Check::new(
                        "A",
                        "its one result is the report the run's end woke, after the note",
                        results.len() == 1
                            && results[0]["body"] == REPORT
                            && notes.first().zip(results.first()).is_some_and(|(note, result)| note["id"].as_i64() < result["id"].as_i64()),
                        format!("{:?}", results.iter().map(|message| message["body"].clone()).collect::<Vec<_>>()),
                    ),
                ]
            },
            &[],
            &[],
        ),
        // The user's Claude status-line script (a shell and git), run by the harness for as long
        // as the window lives, is no background work of the task's (on Unix: the stand-in runs no
        // status line on Windows).
        scenario(
            "field-status-line",
            vec![run!(
                "A",
                "task add --tier standard :: SLEEP 4 Reply with exactly: WORKER_OK",
                |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot)
            )],
            A,
            |judged| {
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    told_none_of(judged, "A", "no message said the status line is background work", &["background work", "statusline", "status line"]),
                ]
            },
            &[],
            &[],
        )
        .with_vars(&[("CF_TEST_STATUS_LINE", "own")], &[]),
        // A chief at rest, a job of its own in the background, its conversation compacted while
        // idle; the human closes the project and resumes it. The chief is told nothing of a turn
        // cut, and starts no work (a-chief-at-rest-with-background-work…, idle-compaction…, A5).
        scenario(
            "field-resume-at-rest",
            vec![
                Step {
                    human: None,
                    restarts: false,
                    name: "the chief leaves a job running",
                    asks: |_| "Start `sleep 300` in the background in your shell, and tell me in one line when it runs.".to_owned(),
                    stand_in: |_| "BACKGROUND 300 the chief left a job".to_owned(),
                    reached: |snapshot, _| crate::matrix::run::settled(snapshot),
                },
                human(
                    "the human compacts the chief's conversation",
                    |members| members.rig().types(&pane_of(members, "chief")?, b"/compact\r"),
                    |snapshot, _| crate::matrix::run::settled(snapshot),
                ),
                human(
                    "the human closes the project",
                    |members| members.page("project.close", json!({})).map(|_| ()),
                    |snapshot, _| chief_lane(snapshot).is_none_or(|lane| lane["pane"].is_null()),
                ),
                human(
                    "the human resumes it",
                    |members| members.resume().map(|_| ()),
                    |snapshot, _| {
                        chief_lane(snapshot).is_some_and(|lane| !lane["pane"].is_null() && lane["activity"]["state"] == "idle")
                            && crate::matrix::run::settled(snapshot)
                    },
                ),
                human("a moment goes by", |_| Ok(()), |snapshot, _| {
                    snapshot.events.iter().rev().find(|event| event["kind"] == "project.state").and_then(|event| event["at"].as_str()).is_some_and(|at| {
                        jiff::Timestamp::now().as_millisecond() - at.parse::<jiff::Timestamp>().map_or(0, |at| at.as_millisecond()) >= 15_000
                    }) && crate::matrix::run::settled(snapshot)
                }),
            ],
            &[],
            |judged| {
                vec![
                    told_none_of(judged, "chief", "it was told of no turn cut", &["in the middle of your turn", "was cut"]),
                    Check::new("chief", "no turn of its was cut in its window", chief_given(judged, "in the middle of your turn") == 0, "its record"),
                ]
            },
            &[],
            &[],
        )
        .waiting(Duration::from_secs(60)),
        // An app quit mid-turn and started again; the human resumes: the window comes back on its
        // conversation, the task is told once that it was cut, and its result comes once
        // (restarts-lose-worker-windows…, the 2026-10-18 task stuck working, paste-restart).
        scenario(
            "field-restart-mid-turn",
            vec![
                run!(
                    "X and A",
                    "task add --advice --tier standard :: SLEEP 30 Reply with exactly: X_RESTART_OK\ntask add --tier standard :: SLEEP 30 Reply with exactly: RESTART_OK",
                    |snapshot, bindings| {
                        since(snapshot, bindings, "A", "working").is_some_and(|seconds| seconds >= 5)
                            && is(snapshot, bindings, "X", 0, &["working"])
                    }
                ),
                Step {
                    human: Some(|_| Ok(())),
                    restarts: true,
                    name: "the app is quit mid-turn and started again",
                    asks: |_| String::new(),
                    stand_in: |_| String::new(),
                    reached: |snapshot, bindings| {
                        answered(snapshot, bindings, "A", 0) && answered(snapshot, bindings, "X", 0) && crate::matrix::run::settled(snapshot)
                    },
                },
            ],
            &[
                Bind { name: "X", is: Is::Pool("advisor"), requester: "chief" },
                Bind { name: "A", is: Is::Pool("worker"), requester: "chief" },
            ],
            |judged| {
                let told: Vec<&str> = judged
                    .snapshot
                    .chief_inbox
                    .iter()
                    .filter(|message| message["kind"] == "note" && message.get("sender").is_none_or(Value::is_null))
                    .filter_map(|message| message["body"].as_str())
                    .filter(|body| body.contains("paused") || body.contains(" cut"))
                    .collect();
                vec![
                    oracle::state(judged, "X", 0, &["done"]),
                    oracle::state(judged, "A", 0, &["done"]),
                    Check::new("X", "its result came once", about(judged, "X", "result").len() == 1, format!("{} result(s)", about(judged, "X", "result").len())),
                    Check::new("A", "its result came once", about(judged, "A", "result").len() == 1, format!("{} result(s)", about(judged, "A", "result").len())),
                    stopped_once(judged, "X"),
                    stopped_once(judged, "A"),
                    Check::new(
                        "chief",
                        "it was told of the two tasks the quit stopped in one note at the most",
                        told.len() <= 1,
                        crate::live::cut(&told.join(" | "), 300),
                    ),
                ]
            },
            &[],
            &[],
        )
        .waiting(Duration::from_secs(90)),
        // dip-agentos #3 and the quota hold: the worker's account runs out mid-task; the task
        // waits, it is not failed, and once the account is back its work resumes and its result
        // comes (quota-hold-drops-messages…).
        scenario(
            "field-quota-out",
            vec![
                human(
                    "the worker's account is out",
                    |members| crate::files::write(&members.folder().join(SPENT), ""),
                    |_, _| true,
                ),
                run!(
                    "A",
                    "task add --tier standard :: SLEEP 3 QUOTA-OUT Reply with exactly: WORKER_OK",
                    |snapshot, bindings| is(snapshot, bindings, "A", 0, &["paused", "failed"])
                ),
                human(
                    "the human logs the worker into another account, and tries it again",
                    |members| {
                        let spent = members.folder().join(SPENT);
                        std::fs::remove_file(&spent).map_err(crate::Error::file("remove", &spent))?;
                        members.page("member.back", json!({ "participant": "matrix-worker" })).map(|_| ())
                    },
                    |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot),
                ),
            ],
            A,
            |judged| {
                let waits: Vec<&str> = chief_messages_with(judged, " waits with @");
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    Check::new("A", "the chief was told once that it waits for the account", waits.len() == 1, format!("{} note(s)", waits.len())),
                    told_none_of(judged, "A", "nothing said it failed", &["failed"]),
                ]
            },
            &[],
            &[],
        )
        .with_vars(&[("CF_TEST_QUOTA_OUT", "matrix-worker"), ("CF_TEST_QUOTA_FILE", SPENT)], &[]),
        // results-stay-queued-after-a-quota-pause, quota-other-windows…: the chief and its worker
        // share an account that runs out while the worker works; the human switches the account
        // and tries the chief again: the worker's result goes into the chief's window.
        scenario(
            "field-quota-account",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: SLEEP 20 Reply with exactly: WORKER_OK",
                    |snapshot, bindings| is(snapshot, bindings, "A", 0, &["working"])
                ),
                human(
                    "the account the chief and the worker share runs out",
                    |members| crate::files::write(&members.folder().join(SPENT), ""),
                    |snapshot, bindings| is(snapshot, bindings, "A", 0, &["paused"]),
                ),
                human(
                    "the human switches the account and tries the chief again",
                    |members| {
                        let spent = members.folder().join(SPENT);
                        std::fs::remove_file(&spent).map_err(crate::Error::file("remove", &spent))?;
                        members.page("member.back", json!({ "participant": "chief" })).map(|_| ())?;
                        members.page("member.back", json!({ "participant": "matrix-worker" })).map(|_| ())
                    },
                    |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot),
                ),
            ],
            A,
            |judged| {
                let result = about(judged, "A", "result").first().map(|message| message["id"].clone());
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    Check::new(
                        "A",
                        "its result went into the chief's window",
                        result.as_ref().is_some_and(|id| chief_given(judged, &format!("[ConsensFlow m-{id} ")) == 1),
                        format!("{result:?}"),
                    ),
                ]
            },
            &[],
            &[],
        )
        .with_vars(&[("CF_TEST_QUOTA_SPENT", "chief,matrix-worker"), ("CF_TEST_QUOTA_FILE", SPENT)], &[]),
        // dip-agentos #2: the project's folder was just `git init`ed; Claude Code asks whether to
        // trust it at the worker's launch; ConsensFlow answers yes in the project's own folder.
        scenario(
            "field-trust-git-init",
            vec![
                human(
                    "the project's folder is made a git repository",
                    |members| {
                        crate::process::Run::new("git").args(["init", "-q"]).cwd(members.folder()).run().map(|_| ())
                    },
                    |_, _| true,
                ),
                run!(
                    "A",
                    "task add --tier standard :: Reply with exactly: TRUSTED_OK",
                    |snapshot, bindings| answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot)
                ),
            ],
            A,
            |judged| {
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    oracle::result_kept(judged, "A"),
                ]
            },
            &[],
            &[],
        )
        .with_vars(&[("CF_TEST_TRUST", "matrix-worker")], &[("CONSENSFLOW_LAUNCH_TIMEOUT_MS", "8000")]),
        // B2: the worker's window closes by itself mid-task; the task is paused and the chief
        // told, and resumed it goes on to its result.
        scenario(
            "field-window-closed",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: SLEEP 60 Reply with exactly: NEVER_SAID",
                    |snapshot, bindings| since(snapshot, bindings, "A", "working").is_some_and(|seconds| seconds >= 4)
                ),
                human(
                    "the worker's window ends",
                    |members| members.cut(&pane_of(members, &a_window(members)?)?).map(|_| ()),
                    |snapshot, bindings| !is(snapshot, bindings, "A", 0, &["working", "queued"]) && crate::matrix::run::settled(snapshot),
                ),
                run!("resume A", "task resume {A} :: Reply with exactly: RESUMED_OK", |snapshot, bindings| {
                    answered(snapshot, bindings, "A", 0) && crate::matrix::run::settled(snapshot)
                }),
            ],
            A,
            |judged| {
                let closed = about(judged, "A", "note");
                vec![
                    oracle::state(judged, "A", 0, &["done"]),
                    Check::new(
                        "A",
                        "the chief was told once that its window closed, how it ended and what it showed last",
                        closed
                            .iter()
                            .filter(|note| {
                                note["body"].as_str().is_some_and(|body| {
                                    body.contains("window closed (exit code") && body.contains("its screen ended with:")
                                })
                            })
                            .count()
                            == 1,
                        crate::live::cut(&closed.iter().filter_map(|note| note["body"].as_str()).collect::<Vec<_>>().join(" | "), 240),
                    ),
                    Check::new(
                        "A",
                        "its result is the resumed turn's",
                        about(judged, "A", "result").iter().any(|message| message["body"] == "RESUMED_OK"),
                        format!("{:?}", about(judged, "A", "result").iter().map(|message| message["body"].clone()).collect::<Vec<_>>()),
                    ),
                ]
            },
            &[],
            &[],
        ),
        // B3: `cf tell` to a task still queued, its window not up yet: taken, and it goes in
        // after the brief. A is T-1 on the board this scenario starts: the tell, in the same
        // turn as the task, names it so.
        scenario(
            "field-tell-queued",
            vec![run!(
                "A, and a tell while it is queued",
                "task add --tier standard :: Reply with exactly: BRIEF\ntell T-1 :: Reply with exactly: TOLD",
                |snapshot, bindings| {
                    bindings.nth("A", 0).is_some()
                        && about_results(snapshot, bindings.nth("A", 0)).iter().any(|body| body == "TOLD")
                        && crate::matrix::run::settled(snapshot)
                }
            )],
            A,
            |judged| {
                let results: Vec<Value> = about(judged, "A", "result").iter().map(|message| message["body"].clone()).collect();
                let tells: Vec<_> = crate::matrix::observe::calls(&judged.snapshot.events)
                    .into_iter()
                    .filter(|call| call.participant == "chief" && call.path.ends_with("/tell"))
                    .collect();
                let refused = tells.iter().filter(|call| call.error.is_some()).count();
                vec![
                    Check::new("chief", "its tell of the queued task was taken", !tells.is_empty() && refused == 0, format!("{} tell(s), {refused} refused", tells.len())),
                    Check::new("A", "the words told went in after the brief, and answered", results.last().is_some_and(|body| body == "TOLD"), format!("{results:?}")),
                ]
            },
            &[],
            &[],
        )
        .with_vars(&[("CF_TEST_SLOW_START", "matrix-worker 8000")], &[]),
        // dip-agentos #4: a failure says the harness's reason, the words its provider gave.
        scenario(
            "field-failure-reason",
            vec![run!(
                "A",
                "task add --tier standard :: MODEL-GONE",
                |snapshot, bindings| is(snapshot, bindings, "A", 0, &["failed"]) && crate::matrix::run::settled(snapshot)
            )],
            A,
            |judged| {
                let failed: Vec<&str> = chief_messages_with(judged, " failed: ");
                vec![
                    oracle::state(judged, "A", 0, &["failed"]),
                    Check::new(
                        "A",
                        "the chief was told why it failed, in the harness's own words",
                        failed.len() == 1 && failed[0].contains("it said: "),
                        crate::live::cut(&failed.join(" | "), 240),
                    ),
                ]
            },
            &[],
            &[],
        )
        .ending(&["A"]),
        // a-stop-closed-a-window-whose-turn-was-over: a worker's turn leaves agents and a job
        // running; the chief tells it something; the stop takes one Escape, the window stays,
        // its work is not ended, and its answer reaches the chief.
        scenario(
            "field-tell-keeps-work",
            vec![
                run!(
                    "A",
                    "task add --tier standard :: JOB 300 AGENTS --then 2 60 SLEEP 300 Reply with exactly: NEVER_OK",
                    |snapshot, bindings| since(snapshot, bindings, "A", "working").is_some_and(|seconds| seconds >= 5)
                ),
                run!("tell A", "tell {A} :: Drop the cap. REPLY dropped", |snapshot, bindings| {
                    bindings.nth("A", 0).is_some_and(|number| {
                        snapshot.chief_inbox.iter().any(|message| message["kind"] == "answer" && message["taskNumber"] == number)
                    }) && crate::matrix::run::settled(snapshot)
                }),
                run!("cancel A", "task cancel {A}", |snapshot, bindings| is(snapshot, bindings, "A", 0, &["cancelled"])),
            ],
            A,
            |judged| {
                vec![
                    Check::new(
                        "A",
                        "its answer to the tell reached the chief",
                        about(judged, "A", "answer").len() == 1,
                        format!("{} answer(s)", about(judged, "A", "answer").len()),
                    ),
                    told_none_of(judged, "A", "nothing said its window did not stop or that a close ended its work", &["did not stop", "its close ended", "ignored the interrupt"]),
                ]
            },
            &[],
            &[],
        )
        .ending(&["A"]),
        // a-switch-chief-note-lists-a-question-on-a-cancelled-task: a worker asks, the chief
        // answers and cancels the task, and the human switches the chief: the new chief's note
        // lists no question to answer.
        scenario(
            "field-switch-chief",
            vec![
                run!(
                    "A asks",
                    "task add --tier standard :: CF ask :: Which grammar, the old one or the new?",
                    |snapshot, bindings| bindings.nth("A", 0).is_some_and(|number| {
                        snapshot.chief_inbox.iter().any(|message| message["kind"] == "question" && message["taskNumber"] == number)
                    }) && crate::matrix::run::settled(snapshot)
                ),
                run!("answer and cancel", "answer m-2 :: Use the new grammar\ntask cancel {A}", |snapshot, bindings| {
                    is(snapshot, bindings, "A", 0, &["cancelled"]) && crate::matrix::run::settled(snapshot)
                }),
                human(
                    "the human makes the advisor the chief",
                    |members| members.page("chief.switch", json!({ "agent": "matrix-advisor" })).map(|_| ()),
                    |snapshot, _| {
                        chief_lane(snapshot).is_some_and(|lane| lane["activity"]["state"] == "idle")
                            && crate::matrix::run::settled(snapshot)
                            && snapshot.events.iter().any(|event| event["kind"] == "chief.switched" || event["kind"] == "project.chief")
                    },
                ),
            ],
            A,
            |judged| {
                vec![
                    Check::new("chief", "the new chief was told it is the chief", chief_given(judged, "You are the chief now") >= 1, "its record"),
                    Check::new("chief", "it was sent to answer no question of a cancelled task", chief_given(judged, "cf answer m-2") == 0, "its record"),
                ]
            },
            &[],
            &[],
        )
        .ending(&["A"]),
    ]
}

/// The bodies of the results of task `number`.
fn about_results(snapshot: &Snapshot, number: Option<i64>) -> Vec<String> {
    snapshot
        .chief_inbox
        .iter()
        .filter(|message| {
            message["kind"] == "result" && number.is_some_and(|n| message["taskNumber"] == n)
        })
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .collect()
}

/// The check that the window of the task bound to `name` was told once, when it came back after
/// the app's restart, that ConsensFlow stopped while it worked.
fn stopped_once(judged: &Judged, name: &str) -> Check {
    let told = judged
        .nth(name, 0)
        .and_then(|number| judged.snapshot.task(number))
        .and_then(|task| task["messages"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter(|message| {
            message["body"].as_str().is_some_and(|body| {
                body.starts_with("ConsensFlow stopped while you were working on T-")
            })
        })
        .count();
    Check::new(
        name,
        "its window came back and was told once that ConsensFlow stopped while it worked",
        told == 1,
        format!("told {told} time(s)"),
    )
}
