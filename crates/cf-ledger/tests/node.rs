//! The ledger file between the two implementations, while both exist: each
//! is refused (`ledger-locked`) while the other holds the file, and Node
//! opens a ledger this crate wrote and reads it as this crate does. Node is
//! `CONSENSFLOW_NODE`, or `node` on the PATH; its ledger is `src/ledger/`.

// The tests start Node themselves; their helpers expect, as the tests do.
#![allow(clippy::disallowed_methods, clippy::expect_used)]

mod node_ledger;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewNote, NewProject, NewQuestion, NewTask, Options,
    ProjectView,
};
use node_ledger::{ledger_module, node, printed};

/// Node's daemon on the ledger at `file`, for a window of `participant`
/// (handle `handle`): what is next for it is delivered, and Node's own
/// collector then reads the turn after it, which wrote `words` (nothing,
/// for a turn that wrote none yet) and is over or not (`settled`). It
/// prints what it did, as `tests/fixtures/node-collector.mjs` says.
fn node_collects(
    file: &Path,
    project: i64,
    participant: (i64, &str),
    turn: Option<(&str, bool)>,
) -> serde_json::Value {
    let program = std::env::var_os("CONSENSFLOW_NODE").unwrap_or_else(|| "node".into());
    let driver = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("node-collector.mjs");
    let plan = serde_json::json!({
        "project": project,
        "participant": participant.0,
        "handle": participant.1,
        "words": turn.map(|(words, _)| words),
        "settled": turn.is_some_and(|(_, settled)| settled),
    });
    let mut command = Command::new(program);
    command
        .arg(driver)
        .arg(ledger_module())
        .arg(file)
        .arg(plan.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    serde_json::from_str(printed(&mut command).trim()).expect("Node prints a line of JSON")
}

/// A project at /work/app: apollo its chief and zeus its one standard worker,
/// both on Claude Code, with no gate.
fn project_with_zeus(ledger: &mut Ledger) -> ProjectView {
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                agent: Some("apollo".into()),
            },
            staff: vec![NewMember {
                agent: "zeus".into(),
                harness: "claude-code".into(),
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            }],
            gate: false,
        })
        .expect("a project")
}

/// The chief gives zeus the task T-1, "Parser", by name, and its brief is
/// delivered and confirmed.
fn given_and_received(ledger: &mut Ledger, project: i64) {
    let brief = ledger
        .create_task(
            project,
            &NewTask {
                from: "chief".into(),
                to: Some("zeus".into()),
                body: "Parser".into(),
                ..NewTask::default()
            },
        )
        .expect("a task")
        .message
        .expect("its brief")
        .id;
    ledger.begin_delivery(brief).expect("its delivery begins");
    ledger
        .confirm_delivery(brief, Some(&serde_json::json!({ "item": "i-1" })))
        .expect("its delivery is confirmed");
}

/// The id of `handle` in the project.
fn id_of(project: &ProjectView, handle: &str) -> i64 {
    project
        .participants
        .iter()
        .find(|participant| participant.handle == handle)
        .expect("a participant")
        .id
}

/// Node holding the ledger at `file` until killed, once it says so.
fn node_holding(file: &Path) -> Child {
    let mut holder = node(
        "openLedger(file);\nconsole.log('ready');\nsetInterval(() => {}, 1000);",
        file,
    )
    .spawn()
    .expect("Node starts");
    let said = BufReader::new(holder.stdout.take().expect("its output"))
        .lines()
        .map_while(Result::ok)
        .next();
    assert_eq!(said.as_deref(), Some("ready"));
    holder
}

#[test]
fn a_ledger_node_holds_is_refused_here_and_one_held_here_is_refused_to_node() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");

    let mut holder = node_holding(&file);
    let refused = open_ledger(&file, Options::default()).err().unwrap();
    assert_eq!(refused.code(), Some("ledger-locked"));
    holder.kill().unwrap();
    holder.wait().unwrap();

    let held = open_ledger(&file, Options::default()).unwrap();
    let said = printed(&mut node(
        "try { openLedger(file); console.log('opened') } catch (cause) { console.log(cause.code) }",
        &file,
    ));
    assert_eq!(said.trim(), "ledger-locked");
    held.close().unwrap();
}

#[test]
fn node_opens_a_ledger_written_here_and_reads_it_as_this_crate_does() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    let mut ledger = open_ledger(&file, Options::default()).unwrap();
    let project = ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "codex".into(),
                agent: Some("astraeus".into()),
            },
            staff: vec![NewMember {
                agent: "zeus".into(),
                harness: "pi".into(),
                designer: false,
                roles: vec!["worker".into(), "reviewer".into()],
                tier: "standard".into(),
            }],
            gate: true,
        })
        .unwrap();
    let here = serde_json::to_string(&ledger.projects().unwrap()).unwrap();
    let events = serde_json::to_string(&ledger.events(project.id, 0, 500).unwrap()).unwrap();
    ledger.close().unwrap();

    let read = printed(&mut node(
        "const ledger = openLedger(file);\n\
         console.log(JSON.stringify(ledger.projects()));\n\
         console.log(JSON.stringify(ledger.events(1)));\n\
         ledger.close();",
        &file,
    ));
    let mut lines = read.lines();
    assert_eq!(lines.next(), Some(here.as_str()), "the projects");
    assert_eq!(lines.next(), Some(events.as_str()), "their events");
}

/// Node knows nothing of carriers, claims, doors or stops: a ledger this
/// crate wrote with all of them is, to its ledger, rows, each a message of
/// its own. It delivers the rows a carrier carried and the carrier as
/// separate messages in the order of their ids, loses none, and its task goes
/// on: the way back to Node's daemon is a way back.
#[test]
fn node_delivers_the_rows_a_carrier_carried_and_the_carrier_as_separate_messages_losing_none() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    let mut ledger = open_ledger(&file, Options::default()).unwrap();
    let project = project_with_zeus(&mut ledger);
    let (chief, zeus) = (id_of(&project, "chief"), id_of(&project, "zeus"));
    given_and_received(&mut ledger, project.id);
    // A question with a door, answered and claimed; a note; a hold and its resume.
    let asked = ledger
        .ask(
            project.id,
            &NewQuestion {
                from: Some("zeus".into()),
                to: "chief".into(),
                task: Some(1),
                questions: Some(serde_json::json!([{
                    "question": "Which?", "header": "H",
                    "options": [{ "label": "red" }], "multiple": false,
                }])),
                ..NewQuestion::default()
            },
        )
        .unwrap();
    let answer = ledger
        .answer(asked.id, chief, None, Some(&serde_json::json!([["red"]])))
        .unwrap();
    assert!(matches!(
        ledger.claim_answer(asked.id, zeus).unwrap(),
        cf_ledger::Claim::Answered(_)
    ));
    let note = ledger
        .note(
            project.id,
            &NewNote {
                from: Some("chief".into()),
                to: "zeus".into(),
                body: "Mind the tests".into(),
                task: Some(1),
            },
        )
        .unwrap();
    ledger
        .hold_task(project.id, 1, "2026-10-10T15:00:00.000Z", "out of quota")
        .unwrap();
    let carrier = ledger
        .resume_task(project.id, 1, None, cf_ledger::RESUME_WORDS)
        .unwrap()
        .message
        .unwrap()
        .id;
    ledger.close().unwrap();

    let delivered = printed(&mut node(
        &format!(
            "const ledger = openLedger(file);
             const delivered = [];
             for (;;) {{
               const next = ledger.nextDelivery({zeus});
               if (!next) break;
               ledger.beginDelivery(next.id);
               ledger.confirmDelivery(next.id, {{ item: 'i' }});
               delivered.push(next.id);
             }}
             console.log(JSON.stringify(delivered));
             console.log(ledger.task({project}, 1).state);
             ledger.close();",
            project = project.id
        ),
        &file,
    ));
    let mut lines = delivered.lines();
    assert_eq!(
        lines.next(),
        Some(format!("[{},{},{carrier}]", answer.id, note.id).as_str()),
        "each row its own message, the carrier last, none lost, the claim no matter to it"
    );
    assert_eq!(lines.next(), Some("working"), "and its task goes on");
}

/// What the way back to Node and forward again starts from: zeus given a
/// task, whose question is open when the hold begins, the daemon's words that
/// resume it (the carrier), and then the chief's answer, which comes after
/// them and rides in them. The ledger is closed, as a daemon that stopped
/// leaves it.
struct Resumed {
    file: PathBuf,
    project: i64,
    zeus: i64,
    carrier: i64,
    answer: i64,
}

fn resumed_then_answered(dir: &Path) -> Resumed {
    let file = dir.join("consensflow.db");
    let mut ledger = open_ledger(&file, Options::default()).expect("a ledger");
    let project = project_with_zeus(&mut ledger);
    let (chief, zeus) = (id_of(&project, "chief"), id_of(&project, "zeus"));
    given_and_received(&mut ledger, project.id);
    let asked = ledger
        .ask(
            project.id,
            &NewQuestion {
                from: Some("zeus".into()),
                to: "chief".into(),
                body: Some("Which format?".into()),
                task: Some(1),
                ..NewQuestion::default()
            },
        )
        .expect("a question");
    ledger
        .hold_task(project.id, 1, "2026-10-10T15:00:00.000Z", "out of quota")
        .expect("held");
    let carrier = ledger
        .resume_task(project.id, 1, None, cf_ledger::RESUME_WORDS)
        .expect("resumed")
        .message
        .expect("its words")
        .id;
    let answer = ledger
        .answer(asked.id, chief, Some(&serde_json::json!("JSON")), None)
        .expect("an answer")
        .id;
    assert!(
        carrier < answer,
        "the answer came after the words it rides in"
    );
    ledger.close().expect("the file is given up");
    Resumed {
        file,
        project: project.id,
        zeus,
        carrier,
        answer,
    }
}

/// Node delivers the carrier of Rust's folds, and with it only its own words:
/// a row that came after the carrier and rode in it is, to Node, a message of
/// its own, still queued. A turn of the window that is not over yet leaves the
/// task going on: when Rust starts again the answer is an ordinary queued
/// message, the next the window is given, and nothing stays queued.
#[test]
fn a_carrier_node_delivered_leaves_a_late_answer_that_this_ledger_delivers_when_it_starts_again() {
    let dir = tempfile::tempdir().unwrap();
    let start = resumed_then_answered(dir.path());

    let node = node_collects(&start.file, start.project, (start.zeus, "zeus"), None);
    assert_eq!(
        node["delivered"], start.carrier,
        "the words, which are first"
    );
    assert_eq!(
        node["text"],
        format!(
            "[ConsensFlow m-{} · T-1 · task from ConsensFlow]\nResumed: Go on where you stopped.",
            start.carrier
        ),
        "the carrier's row stands on its own: its words, and nothing of what it carried"
    );
    assert_eq!(
        node["task"], "working",
        "Node counts the queued answer as received"
    );

    let mut ledger = open_ledger(&start.file, Options::default()).unwrap();
    assert_eq!(
        ledger.task(start.project, 1).unwrap().unwrap().task.state,
        "waiting",
        "the question has no answer received, whatever Node counted: the start says so"
    );
    let next = ledger
        .next_delivery(start.zeus)
        .unwrap()
        .expect("a message");
    assert_eq!(
        next.id, start.answer,
        "the answer is the next the window is given"
    );
    let begun = ledger.begin_delivery(next.id).unwrap();
    assert!(
        begun.carried.is_empty(),
        "pasted by itself: it rides in nothing"
    );
    ledger
        .confirm_delivery(next.id, Some(&serde_json::json!({ "item": "i-3" })))
        .unwrap();
    assert!(
        ledger.pending(start.zeus).unwrap().is_empty(),
        "nothing is left queued"
    );
    assert_eq!(
        ledger.task(start.project, 1).unwrap().unwrap().task.state,
        "working"
    );
    assert!(
        ledger
            .events(start.project, 0, 500)
            .unwrap()
            .iter()
            .any(|event| event.kind == "message.uncarried"
                && event.data
                    == serde_json::json!({ "carrier": start.carrier, "released": [start.answer] })),
        "the ledger says what it let go of"
    );
    ledger.close().unwrap();
}

/// Astraeus's probe: Node's collector reads the turn the carrier began, which
/// says it still needs the format, and finishes the task before the answer
/// arrives, as Node always could; that is Node's rule, kept. What this ledger
/// owes is that the answer is not lost to it: after the start it is a queued
/// message of its own, and the reopening that sends the task back carries it
/// (the reopening's fold took a row under a settled carrier before the start
/// let it go; what the start's release alone holds is the test above).
#[test]
fn an_answer_node_finished_the_task_before_delivering_is_carried_by_the_reopening_after_the_start()
{
    let dir = tempfile::tempdir().unwrap();
    let start = resumed_then_answered(dir.path());

    let node = node_collects(
        &start.file,
        start.project,
        (start.zeus, "zeus"),
        Some(("I still need the format.", true)),
    );
    assert_eq!(node["delivered"], start.carrier);
    assert_eq!(
        node["task"], "done",
        "Node's collector took the turn for the result"
    );
    assert_eq!(node["result"], "I still need the format.");

    let mut ledger = open_ledger(&start.file, Options::default()).unwrap();
    assert_eq!(
        ledger
            .pending(start.zeus)
            .unwrap()
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        [start.answer],
        "the answer is queued, as a message of its own"
    );
    assert_eq!(
        ledger.next_delivery(start.zeus).unwrap(),
        None,
        "and there is no task for it to go in: that is for the reopening"
    );

    let words = ledger
        .reopen_task(start.project, 1, "chief", "Use JSON")
        .unwrap()
        .message
        .unwrap();
    let begun = ledger.begin_delivery(words.id).unwrap();
    assert_eq!(
        begun
            .carried
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        [start.answer],
        "the reopening carries it: it came to the window after all"
    );
    ledger
        .confirm_delivery(words.id, Some(&serde_json::json!({ "item": "i-4" })))
        .unwrap();
    assert!(
        ledger.pending(start.zeus).unwrap().is_empty(),
        "nothing is left queued"
    );
    assert_eq!(
        ledger.message(start.answer).unwrap().unwrap().state,
        "delivered"
    );
    assert_eq!(
        ledger.task(start.project, 1).unwrap().unwrap().task.state,
        "working"
    );
    ledger.close().unwrap();
}
