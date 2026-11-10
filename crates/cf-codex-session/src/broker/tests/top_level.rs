//! Which of the TUI's starts, resumes and forks reach Codex with the window's role: one
//! table, each row played in one life of the broker and across a close and a reopen of the
//! window, over the threads Codex 0.167.0 described (`codex_0_167_threads.json`).
//!
//! The role goes to the window's main start, and to a resume or a fork only of a thread
//! Codex's own metadata names top-level: no parent, a user's source, and every thread it was
//! forked from the same. A sub-agent's thread given it takes it at its next compaction and
//! acts on the board as the member (T-1402, T-1412); a top-level one not given it loses it
//! there (T-1388). Where Codex does not say, the thread goes without it.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::Duration;

use serde_json::{json, Value};

use super::fixture::{run, Fixture, Options, Reads, Tui};

const ROLE: &str = "Codex's own instructions.\n\nYour ConsensFlow role is worker.";

/// What Codex 0.167.0 answered to `thread/read` of each thread, recorded by T-1414.
const RECORDED: &str = include_str!("codex_0_167_threads.json");

/// Every thread Codex has, by the name the rows use: the ones it described, and a few
/// more of the same shapes under other ids.
fn threads() -> HashMap<&'static str, Value> {
    let recorded: Value = serde_json::from_str(RECORDED).expect("the recording");
    let read = |name: &str| recorded["reads"][name].clone();
    let main = read("main");
    let like = |base: &Value, id: &str, changes: Value| {
        let mut thread = base.clone();
        thread["id"] = id.into();
        thread["sessionId"] = id.into();
        for (key, value) in changes.as_object().expect("changes") {
            thread[key] = value.clone();
        }
        thread
    };
    let mut threads = HashMap::from([
        ("main", main.clone()),
        ("child", read("child")),
        ("forkContextChild", read("forkContextChild")),
        ("forkOfMain", read("forkOfMain")),
        ("forkOfChild", read("forkOfChild")),
    ]);
    // Another conversation of the user's: the probe's other main thread, as `main` reads.
    threads.insert(
        "other",
        like(&main, "01a17a02-6a4f-74a0-a98c-39d3d91c56a3", json!({})),
    );
    // Threads the window's `/new` began: top-level, as `main` reads.
    threads.insert(
        "new",
        like(&main, "01a1c7a0-0000-7000-8000-000000000001", json!({})),
    );
    // A fork of the user's fork: as `forkOfMain` reads, forked from it.
    let fork_of_main = read("forkOfMain");
    threads.insert(
        "forkOfFork",
        like(
            &fork_of_main,
            "01a1c7a0-0000-7000-8000-000000000002",
            json!({ "forkedFromId": fork_of_main["id"] }),
        ),
    );
    // Codex's memory job: an internal session, which the protocol names `unknown`
    // (codex-rs 0.167.0 `thread_data.rs`), of the memory job's source.
    threads.insert(
        "memoryJob",
        like(
            &main,
            "01a1c7a0-0000-7000-8000-000000000003",
            json!({ "source": "unknown", "threadSource": "memory_consolidation" }),
        ),
    );
    // A review's sub-agent: `subAgent` of kind `review`, under the main thread.
    threads.insert(
        "review",
        like(
            &main,
            "01a1c7a0-0000-7000-8000-000000000004",
            json!({
                "source": { "subAgent": "review" },
                "parentThreadId": main["id"],
                "threadSource": "guardian_review",
            }),
        ),
    );
    // A top-level fork of a thread Codex no longer has.
    threads.insert(
        "orphan",
        like(
            &fork_of_main,
            "01a1c7a0-0000-7000-8000-000000000005",
            json!({ "forkedFromId": "01a1c7a0-0000-7000-8000-0000000000ff" }),
        ),
    );
    threads
}

/// One step of a row: what the TUI says, or what happens to the window or to Codex.
enum Step {
    /// The TUI's main start, which Codex answers with this thread; and whether it must reach
    /// Codex with the role (none: either).
    Start(&'static str, Option<bool>),
    /// The TUI resumes this thread, naming these workspace roots.
    Resume(&'static str, Value, Option<bool>),
    /// The user forks the first thread, and Codex answers with the second.
    Fork(&'static str, &'static str, Option<bool>),
    /// The TUI says `method` with these params, and Codex answers with this result.
    Other(&'static str, Value, Value, Option<bool>),
    /// The window closes and opens again, launched to resume this thread (none: to start).
    Reopen(Option<&'static str>),
    /// Codex answers a `thread/read` as this says from now on.
    Reads(Reads),
    /// Codex answers a read of the first thread with what it keeps of the second.
    Misread(&'static str, &'static str),
}

use Step::{Fork, Misread, Other, Reads as ReadsAs, Reopen, Resume, Start};

/// How a row's steps are made, each time they are played.
type Steps = fn() -> Vec<Step>;

/// A row: what it is, played in one life of the broker, and across a close and a reopen.
struct Row {
    name: &'static str,
    one_life: Vec<Step>,
    reopened: Vec<Step>,
}

const YES: Option<bool> = Some(true);
const NO: Option<bool> = Some(false);

fn list() -> Value {
    json!(["/work"])
}

fn rows() -> Vec<Row> {
    let mut rows = vec![
        Row {
            name: "the first main start",
            one_life: vec![Start("main", YES)],
            reopened: vec![Start("main", None), Reopen(None), Start("new", YES)],
        },
        Row {
            name: "a resume of the launch's thread, top-level, first and second",
            one_life: vec![
                Reopen(Some("main")),
                Resume("main", list(), YES),
                Resume("main", list(), YES),
            ],
            reopened: vec![
                Start("main", None),
                Reopen(Some("main")),
                Resume("main", list(), YES),
                Reopen(Some("main")),
                Resume("main", list(), YES),
            ],
        },
        Row {
            name: "a resume of the launch's thread when the ledger holds a child's",
            one_life: vec![Reopen(Some("child")), Resume("child", list(), NO)],
            reopened: vec![
                Start("main", None),
                Resume("child", Value::Null, None),
                Reopen(Some("child")),
                Resume("child", list(), NO),
            ],
        },
        Row {
            name: "/new, then its resume",
            one_life: vec![
                Start("main", None),
                Start("new", YES),
                Resume("new", list(), YES),
            ],
            reopened: vec![
                Start("main", None),
                Start("new", None),
                Reopen(Some("new")),
                Resume("new", list(), YES),
            ],
        },
        Row {
            name: "a user's fork of a top-level thread, and a fork of that fork",
            one_life: vec![
                Start("main", None),
                Fork("main", "forkOfMain", YES),
                Fork("forkOfMain", "forkOfFork", YES),
            ],
            reopened: vec![
                Start("main", None),
                Fork("main", "forkOfMain", None),
                Reopen(Some("forkOfMain")),
                Resume("forkOfMain", list(), YES),
                Fork("forkOfMain", "forkOfFork", YES),
                Reopen(Some("forkOfFork")),
                Resume("forkOfFork", list(), YES),
            ],
        },
        Row {
            name: "a fork of a child, and its resume",
            one_life: vec![
                Start("main", None),
                Resume("child", Value::Null, None),
                Fork("child", "forkOfChild", NO),
                Resume("forkOfChild", list(), NO),
            ],
            reopened: vec![
                Start("main", None),
                Resume("child", Value::Null, None),
                Fork("child", "forkOfChild", None),
                Reopen(Some("forkOfChild")),
                Resume("forkOfChild", list(), NO),
            ],
        },
        Row {
            name: "a user's /resume of another top-level conversation",
            one_life: vec![Start("main", None), Resume("other", list(), YES)],
            reopened: vec![
                Start("main", None),
                Reopen(Some("main")),
                Resume("main", list(), None),
                Resume("other", list(), YES),
            ],
        },
        Row {
            name: "a live child selected, its roots null and a list",
            one_life: vec![
                Start("main", None),
                Resume("child", Value::Null, NO),
                Resume("child", list(), NO),
                Resume("forkContextChild", Value::Null, NO),
            ],
            reopened: vec![
                Start("main", None),
                Reopen(Some("main")),
                Resume("main", list(), None),
                Resume("child", Value::Null, NO),
                Resume("forkContextChild", list(), NO),
            ],
        },
    ];
    // A child the TUI resumes cold, its live attach refused for a config mismatch: the TUI's
    // resume with the thread's own settings, which Codex answers from the rollout.
    let cold = |want| {
        Other(
            "thread/resume",
            json!({
                "threadId": threads()["child"]["id"],
                "runtimeWorkspaceRoots": null,
                "model": "gpt-5.5",
                "config": { "model_reasoning_effort": "low" },
            }),
            json!({ "thread": threads()["child"] }),
            want,
        )
    };
    rows.push(Row {
        name: "a child resumed cold after a config mismatch",
        one_life: vec![Start("main", None), cold(NO)],
        reopened: vec![
            Start("main", None),
            Reopen(Some("main")),
            Resume("main", list(), None),
            cold(NO),
        ],
    });
    let main = || threads()["main"]["id"].clone();
    let side = || {
        vec![
            // The memory job's start, a review, a side start, an ephemeral fork, a turn.
            Other(
                "thread/start",
                json!({ "ephemeral": true, "threadSource": "memory_consolidation", "developerInstructions": null }),
                json!({ "thread": threads()["memoryJob"] }),
                NO,
            ),
            Other(
                "review/start",
                json!({ "threadId": main(), "target": { "type": "uncommittedChanges" } }),
                json!({ "turn": { "id": "t-1" } }),
                NO,
            ),
            Other(
                "thread/start",
                json!({ "ephemeral": true, "threadSource": "user", "developerInstructions": null }),
                json!({ "thread": { "id": "01a1c7a0-0000-7000-8000-000000000006" } }),
                NO,
            ),
            Other(
                "thread/fork",
                json!({ "threadId": main(), "ephemeral": true, "threadSource": "user", "runtimeWorkspaceRoots": [] }),
                json!({ "thread": { "id": "01a1c7a0-0000-7000-8000-000000000007" } }),
                NO,
            ),
            Other(
                "turn/start",
                json!({ "threadId": main(), "input": [] }),
                json!({ "turn": { "id": "t-2" } }),
                NO,
            ),
            // The memory job's thread and a review's, resumed.
            Resume("memoryJob", list(), NO),
            Resume("review", Value::Null, NO),
        ]
    };
    let mut one_life = vec![Start("main", None)];
    one_life.extend(side());
    let mut reopened = vec![
        Start("main", None),
        Reopen(Some("main")),
        Resume("main", list(), None),
    ];
    reopened.extend(side());
    rows.push(Row {
        name: "the memory job, a review, a side start, an ephemeral fork, a turn",
        one_life,
        reopened,
    });
    let unread: [(&'static str, Steps); 5] = [
        ("a Codex with no thread/read", || {
            vec![ReadsAs(Reads::Refused(
                json!({ "code": -32601, "message": "Method not found" }),
            ))]
        }),
        ("a read that failed", || {
            vec![ReadsAs(Reads::Refused(
                json!({ "code": -32603, "message": "failed to read thread" }),
            ))]
        }),
        ("a read never answered", || vec![ReadsAs(Reads::Silent)]),
        ("a read answered late", || {
            vec![ReadsAs(Reads::Late(Duration::from_secs(1)))]
        }),
        ("a read answered with another thread", || {
            vec![Misread("main", "other")]
        }),
    ];
    for (name, how) in unread {
        let mut one_life = vec![Start("main", None)];
        one_life.extend(how());
        one_life.push(Resume("main", list(), NO));
        let mut reopened = vec![Start("main", None), Reopen(Some("main"))];
        reopened.extend(how());
        reopened.push(Resume("main", list(), NO));
        rows.push(Row {
            name,
            one_life,
            reopened,
        });
    }
    // A top-level fork whose thread forked from is gone: nothing says what that was.
    rows.push(Row {
        name: "a fork of a thread Codex no longer has",
        one_life: vec![Start("main", None), Resume("orphan", list(), NO)],
        reopened: vec![
            Start("main", None),
            Reopen(Some("orphan")),
            Resume("orphan", list(), NO),
        ],
    });
    rows
}

/// What a window is launched with: its role. The thread it resumes is the TUI's to name
/// (`codex resume <id>`), and its resume is all the broker hears of it.
fn launched(_thread: Option<&str>) -> Options {
    Options {
        role: Some(ROLE.to_owned()),
        ..Options::default()
    }
}

/// Plays `steps` in a window whose Codex has `threads`: each step that must or must not
/// reach Codex with the role and did otherwise, said.
async fn play(steps: Vec<Step>, threads: &HashMap<&'static str, Value>) -> Vec<String> {
    let thread = |name: &str| threads[name].clone();
    let id = |name: &str| threads[name]["id"].clone();
    let mut f = Fixture::with(launched(None)).await;
    for known in threads.values() {
        f.codex.knows(known);
    }
    let mut tui: Tui = f.connect().await;
    let mut missed = Vec::new();
    for (at, step) in steps.into_iter().enumerate() {
        let n = at as u64 + 1;
        let (what, sent, want) = match step {
            Start(answer, want) => (
                format!("start answered with {answer}"),
                f.call(
                    &tui,
                    n,
                    "thread/start",
                    json!({ "ephemeral": false, "threadSource": "user", "developerInstructions": null }),
                    json!({ "thread": thread(answer) }),
                )
                .await,
                want,
            ),
            Resume(name, roots, want) => (
                format!("resume of {name}, roots {roots}"),
                f.call(
                    &tui,
                    n,
                    "thread/resume",
                    json!({ "threadId": id(name), "runtimeWorkspaceRoots": roots }),
                    json!({ "thread": thread(name) }),
                )
                .await,
                want,
            ),
            Fork(from, to, want) => (
                format!("fork of {from}"),
                f.call(
                    &tui,
                    n,
                    "thread/fork",
                    json!({ "threadId": id(from), "threadSource": "user", "runtimeWorkspaceRoots": [] }),
                    json!({ "thread": thread(to) }),
                )
                .await,
                want,
            ),
            Other(method, params, answer, want) => (
                format!("{method} {params}"),
                f.call(&tui, n, method, params, answer).await,
                want,
            ),
            Reopen(launch) => {
                drop(tui);
                f = f.reopen(launched(launch)).await;
                tui = f.connect().await;
                continue;
            }
            ReadsAs(reads) => {
                f.codex.reads(reads);
                continue;
            }
            Misread(asked, answered) => {
                f.codex
                    .knows_as(id(asked).as_str().expect("an id"), &thread(answered));
                continue;
            }
        };
        let given = sent["developerInstructions"] == ROLE;
        if want.is_some_and(|want| want != given) {
            missed.push(format!(
                "step {n}, {what}: {}",
                if given {
                    "given the role"
                } else {
                    "not given the role"
                }
            ));
        }
    }
    f.broker.close().await;
    missed
}

#[test]
fn a_resume_or_fork_is_given_the_windows_role_only_when_codex_says_its_thread_is_top_level() {
    run(async {
        let threads = threads();
        let mut report = String::new();
        let mut red = 0;
        for row in rows() {
            for (life, steps) in [("one life", row.one_life), ("reopened", row.reopened)] {
                let missed = play(steps, &threads).await;
                let outcome = if missed.is_empty() {
                    "ok".to_owned()
                } else {
                    red += 1;
                    format!("RED: {}", missed.join("; "))
                };
                let _ = writeln!(report, "{} | {life} | {outcome}", row.name);
            }
        }
        println!("{report}");
        assert_eq!(red, 0, "rows that missed:\n{report}");
    });
}
