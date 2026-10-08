//! What a cancel leaves behind: the chief's re-plan of 2026-10-07 (the chief
//! eval `six-decisions`, run on the native daemon) cancelled tasks that no
//! member had yet and a task whose window it had told, and the eval counted
//! fewer tasks on the board and fewer tells answered than it had made. The
//! board then left such a task off both its lists, as it did a paused backlog
//! task and a removed member's tasks; the owner decided (2026-10-07) that all
//! three stay in view, and the board places them so. Each test writes the
//! states here and holds the ledger to what Node's ledger answered on the same
//! file while Node's ledger existed (`21297242`, the last commit that held
//! both, where the test compared the two and they agreed): the threads below
//! are those answers, written down as this ledger gave them there, and fixed
//! since Node went.

#![allow(clippy::expect_used)]

use std::path::Path;

use cf_ledger::{
    open_ledger, Board, Ledger, NewChief, NewMember, NewProject, NewQuestion, NewTask, Options,
    ProjectView, TaskCard,
};
use serde_json::{json, Value};

/// Session names in a fixed order, so a test can say `zeus-amber-pine`.
fn options() -> Options {
    let mut names = ["amber-pine", "brisk-birch", "calm-brook"]
        .into_iter()
        .cycle();
    Options {
        names: Box::new(move || names.next().unwrap_or_default().to_owned()),
        ..Options::default()
    }
}

/// A project at /work/app: apollo its chief, zeus, hera and athena its standard workers.
fn project(ledger: &mut Ledger) -> ProjectView {
    let worker = |agent: &str| NewMember {
        agent: agent.into(),
        harness: "claude-code".into(),
        designer: false,
        roles: vec!["worker".into()],
        tier: "standard".into(),
    };
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                agent: Some("apollo".into()),
            },
            staff: vec![worker("zeus"), worker("hera"), worker("athena")],
            gate: false,
        })
        .expect("a project")
}

fn id_of(project: &ProjectView, handle: &str) -> i64 {
    project
        .participants
        .iter()
        .find(|participant| participant.handle == handle)
        .expect("a participant")
        .id
}

/// The chief gives a task to `to` by name, waiting for `needs`.
fn by_name(ledger: &mut Ledger, project: i64, to: &str, body: &str, needs: &[u64]) {
    ledger
        .create_task(
            project,
            &NewTask {
                from: "chief".into(),
                to: Some(to.into()),
                body: body.into(),
                needs: needs.to_vec(),
                ..NewTask::default()
            },
        )
        .expect("a task given by name");
}

/// The chief opens a task for a standard worker, waiting for `needs`.
fn open(ledger: &mut Ledger, project: i64, body: &str, needs: &[u64]) {
    ledger
        .create_task(
            project,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: body.into(),
                needs: needs.to_vec(),
                ..NewTask::default()
            },
        )
        .expect("a task");
}

/// The daemon gives the open task `number` to a new session of `member`, whose
/// window takes the brief in; if `result`, its turn ends with that result.
fn given(ledger: &mut Ledger, project: i64, number: i64, member: i64, result: Option<&str>) {
    let brief = ledger
        .assign_task(project, number, member)
        .expect("assigned")
        .message
        .expect("its brief")
        .id;
    ledger.begin_delivery(brief).expect("its delivery begins");
    ledger
        .confirm_delivery(brief, Some(&json!({ "item": "i-1" })))
        .expect("its delivery is confirmed");
    if let Some(result) = result {
        ledger
            .record_result(project, number, result)
            .expect("its result");
    }
}

/// Where each task the board lists is, as `place T-n state`: those no lane
/// has under `open`, the rest under their lane's handle.
fn listed(board: &Board) -> Vec<String> {
    let card = |place: &str, task: &TaskCard| format!("{place} T-{} {}", task.number, task.state);
    board
        .open
        .iter()
        .map(|task| card("open", task))
        .chain(board.lanes.iter().flat_map(|lane| {
            lane.tasks
                .iter()
                .map(|task| card(&lane.participant.handle, task))
        }))
        .collect()
}

/// A ledger in every state a task may be placed from, the way the chief's
/// re-plan, the human and the daemon's own moves leave them:
///
/// - T-1 given to zeus, done, and not accepted, so what needs it waits;
/// - T-2 cancelled, T-3 paused and T-4 failed, each still open, no member
///   having had it (a task for a tier has no assignee until the daemon gives
///   it one): the three of them wait for T-1;
/// - T-5 open, waiting for a member;
/// - T-6 given to zeus by name and cancelled: it has its assignee;
/// - T-7 done in a session of hera that the human then ended;
/// - T-8 done in a session of zeus, accepted, and deleted by the human;
/// - athena's, before the human removed her from the staff: T-9 given to her
///   by name, T-10 given to her by name and paused, T-11 done in a session
///   of hers, T-12 done in another and accepted, T-13 given to her by name
///   and waiting for T-1. Her removal cancelled T-9, the one task in her
///   hands (queued); the others stay as they were, and her lane and her
///   sessions' went.
fn replanned(file: &Path) -> Board {
    let mut ledger = open_ledger(file, options()).expect("a ledger");
    let project = project(&mut ledger);
    let (zeus, hera, athena) = (
        id_of(&project, "zeus"),
        id_of(&project, "hera"),
        id_of(&project, "athena"),
    );
    for (body, needs) in [
        ("Parser", &[][..]),
        ("Docs", &[1]),
        ("Tests", &[1]),
        ("Lexer", &[1]),
        ("Release", &[]),
    ] {
        open(&mut ledger, project.id, body, needs);
    }
    given(&mut ledger, project.id, 1, zeus, Some("Parsed"));
    ledger.cancel_task(project.id, 2, "chief").expect("T-2");
    ledger
        .pause_task(project.id, 3, Some("chief"), None)
        .expect("T-3");
    ledger
        .fail_task(project.id, 4, "its launch never came up")
        .expect("T-4");
    by_name(&mut ledger, project.id, "zeus", "By name", &[]);
    ledger.cancel_task(project.id, 6, "chief").expect("T-6");
    open(&mut ledger, project.id, "Ended", &[]);
    given(&mut ledger, project.id, 7, hera, Some("Done"));
    ledger
        .end_session(project.id, "hera-brisk-birch", "human")
        .expect("the session ends");
    open(&mut ledger, project.id, "Deleted", &[]);
    given(&mut ledger, project.id, 8, zeus, Some("Done"));
    ledger.accept_task(project.id, 8, "chief").expect("T-8");
    ledger.delete_tasks(project.id, &[8]).expect("T-8 goes");
    by_name(&mut ledger, project.id, "athena", "Gone", &[]);
    by_name(&mut ledger, project.id, "athena", "Held", &[]);
    ledger
        .pause_task(project.id, 10, Some("chief"), None)
        .expect("T-10");
    open(&mut ledger, project.id, "Finished", &[]);
    given(&mut ledger, project.id, 11, athena, Some("Done"));
    open(&mut ledger, project.id, "Accepted", &[]);
    given(&mut ledger, project.id, 12, athena, Some("Done"));
    ledger.accept_task(project.id, 12, "chief").expect("T-12");
    by_name(&mut ledger, project.id, "athena", "Waits", &[1]);
    ledger
        .remove_member(project.id, "athena")
        .expect("athena leaves");
    let board = ledger.board(project.id).expect("a board");
    ledger.close().expect("the file is given up");
    board
}

#[test]
fn a_task_no_lane_has_is_among_the_open_ones_whatever_its_state_as_node_draws_the_board() {
    let dir = tempfile::tempdir().unwrap();
    let board = replanned(&dir.path().join("consensflow.db"));

    assert_eq!(
        listed(&board),
        [
            "open T-2 cancelled",
            "open T-3 paused",
            "open T-4 failed",
            "open T-5 open",
            "open T-9 cancelled",
            "open T-10 paused",
            "open T-11 done",
            "open T-12 accepted",
            "open T-13 open",
            "zeus T-6 cancelled",
            "hera T-7 done",
            "zeus-amber-pine T-1 done",
        ],
        "T-2 (cancelled), T-3 (paused) and T-4 (failed) were never given to a member, and athena's \
         tasks (T-9 to T-13, whatever their state) are of a member who left: the board lists a \
         task by its lane, and these have none, so they are among the open ones, each in the \
         order of its number. T-8 was deleted by the human: it is on neither"
    );
}

/// What the cancel of T-1 (`by` the chief) left of its thread, as `[task,
/// state]` and each message as `[id, kind, state, reason]`.
fn thread(task: &Value) -> Value {
    let messages = task["messages"].as_array().expect("its thread");
    json!([
        [task["number"], task["state"]],
        messages
            .iter()
            .map(|message| json!([
                message["id"],
                message["kind"],
                message["state"],
                message["reason"]
            ]))
            .collect::<Vec<_>>(),
    ])
}

/// The chief's tell of T-1 as the ledger holds it once its window has taken it
/// in (`received`), answered (`answered`) and the answer has reached the chief
/// (`heard`): T-1 is paused, and zeus-amber-pine's window is on it. The ids of
/// the tell and of the session.
fn told(file: &Path, received: bool, answered: bool, heard: bool) -> (i64, i64) {
    let mut ledger = open_ledger(file, options()).expect("a ledger");
    let project = project(&mut ledger);
    open(&mut ledger, project.id, "Parser", &[]);
    given(&mut ledger, project.id, 1, id_of(&project, "zeus"), None);
    let tell = ledger
        .ask(
            project.id,
            &NewQuestion {
                from: Some("chief".into()),
                to: "zeus-amber-pine".into(),
                body: Some("Stop: the grammar changed".into()),
                task: Some(1),
                urgent: true,
                ..NewQuestion::default()
            },
        )
        .expect("a tell")
        .id;
    let session = ledger
        .project(project.id)
        .expect("a project")
        .and_then(|found| {
            found
                .participants
                .into_iter()
                .find(|participant| participant.handle == "zeus-amber-pine")
        })
        .expect("the session")
        .id;
    if received {
        ledger.begin_delivery(tell).expect("the tell goes in");
        ledger
            .confirm_delivery(tell, Some(&json!({ "item": "i-2" })))
            .expect("the tell is confirmed");
    }
    if answered {
        let chief = id_of(&project, "chief");
        let answer = ledger
            .answer(tell, session, Some(&json!("Stopped")), None)
            .expect("the window answers its tell");
        assert_eq!(
            (answer.recipient_id, answer.state.as_str()),
            (chief, "queued")
        );
        if heard {
            ledger
                .begin_delivery(answer.id)
                .expect("the answer goes in");
            ledger
                .confirm_delivery(answer.id, Some(&json!({ "item": "i-3" })))
                .expect("the answer is confirmed");
        }
    }
    ledger.close().expect("the file is given up");
    (tell, session)
}

/// The chief cancels T-1: the thread it leaves, and what a window's answer to
/// the tell, given after it, is told.
fn cancelled(file: &Path, tell: i64, session: i64) -> (Value, Option<&'static str>) {
    let mut ledger = open_ledger(file, options()).expect("a ledger");
    ledger.cancel_task(1, 1, "chief").expect("cancelled");
    let found =
        serde_json::to_value(ledger.task(1, 1).expect("a task").expect("T-1")).expect("JSON");
    let late = ledger
        .answer(tell, session, Some(&json!("Late")), None)
        .err()
        .and_then(|refused| refused.code());
    ledger.close().expect("the file is given up");
    (thread(&found), late)
}

/// Whether the thread's one tell counts as answered the way the chief eval
/// counts it (`plumbing` in `evals/measure.mjs`): an answer to it that is not
/// cancelled.
fn counted_answered(thread: &Value) -> bool {
    thread[1]
        .as_array()
        .expect("messages")
        .iter()
        .any(|message| message[1] == "answer" && message[2] != "cancelled")
}

#[test]
fn a_cancel_withdraws_the_tell_not_yet_in_its_window_and_the_answer_not_yet_with_the_chief_as_node_does(
) {
    for (received, answered, heard, tell_state, counted, thread) in [
        // The window was still busy with its turn: the tell never went in.
        (
            false,
            false,
            false,
            "cancelled",
            false,
            json!([
                [1, "cancelled"],
                [
                    [1, "task", "delivered", null],
                    [2, "question", "cancelled", "cancelled by @chief"],
                ]
            ]),
        ),
        // It went in, and the window never answered it.
        (
            true,
            false,
            false,
            "delivered",
            false,
            json!([
                [1, "cancelled"],
                [
                    [1, "task", "delivered", null],
                    [2, "question", "delivered", null],
                ]
            ]),
        ),
        // It answered, but the chief was in a turn of its own: the answer
        // waited in its queue, and the cancel took it back.
        (
            true,
            true,
            false,
            "delivered",
            false,
            json!([
                [1, "cancelled"],
                [
                    [1, "task", "delivered", null],
                    [2, "question", "delivered", null],
                    [3, "answer", "cancelled", "cancelled by @chief"],
                ]
            ]),
        ),
        // The chief heard the answer before it cancelled.
        (
            true,
            true,
            true,
            "delivered",
            true,
            json!([
                [1, "cancelled"],
                [
                    [1, "task", "delivered", null],
                    [2, "question", "delivered", null],
                    [3, "answer", "delivered", null],
                ]
            ]),
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("consensflow.db");
        let (tell, session) = told(&file, received, answered, heard);

        let (here, refused) = cancelled(&file, tell, session);
        let case = format!("received {received}, answered {answered}, heard {heard}");
        assert_eq!(here, thread, "{case}: the thread the cancel leaves");
        assert_eq!(
            refused,
            Some("task-cancelled"),
            "{case}: nobody waits for it"
        );
        let tell_row = here[1]
            .as_array()
            .expect("messages")
            .iter()
            .find(|message| message[0].as_i64() == Some(tell))
            .expect("the tell");
        assert_eq!(tell_row[2], tell_state, "{case}: the tell");
        assert_eq!(
            counted_answered(&here),
            counted,
            "{case}: counted as answered"
        );
    }
}
