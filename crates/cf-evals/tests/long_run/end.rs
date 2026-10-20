//! 9: the project reaches its end within the budget, every task in an end state and
//! the chief's report to the owner after the last of them, both by the deadline.

use cf_evals::long_run::invariants::end;

use crate::scene::{ms, t, Scene, CHIEF, HUMAN};

/// Three tasks of the worker (3): two accepted at 10:30 and 10:40, one `third`.
fn board(third: &str) -> Scene {
    let scene = Scene::new(&[("long-worker", "codex")]);
    scene.task(1, 3, "accepted", &t(30, 0));
    scene.task(2, 3, "accepted", &t(40, 0));
    scene.task(3, 3, third, &t(45, 0));
    scene
}

/// A budget of two hours from 10:00: its deadline.
fn two_hours() -> i64 {
    ms(120, 0)
}

#[test]
fn a_task_still_open_or_no_report_to_the_owner_breaks_the_ninth_and_says_where_it_stood() {
    let scene = board("working");
    scene.message(
        300,
        (HUMAN, Some(CHIEF)),
        "note",
        None,
        "Phase 1 and 2 done.",
        &t(41, 0),
    );
    let found = end(&scene.look(ms(120, 0)), "time ran out", two_hours()).unwrap();
    assert_eq!(found.key, "9:end");
    assert_eq!(
        found.said,
        "the project did not reach its end within the budget (time ran out): 1 of 3 tasks open, no report"
    );
    assert_eq!(found.evidence.rows[0], "T-3 working @long-worker: Task 3");
    assert!(
        found.evidence.rows[1].starts_with("no report to the owner between 2026-10-15T10:45:00")
    );
    // A report after the last change is no end while a task is still open.
    scene.message(
        302,
        (HUMAN, Some(CHIEF)),
        "note",
        None,
        "Phase 3 is on its way.",
        &t(50, 0),
    );
    let found = end(&scene.look(ms(120, 0)), "time ran out", two_hours()).unwrap();
    assert_eq!(
        found.said,
        "the project did not reach its end within the budget (time ran out): 1 of 3 tasks open"
    );
    // Every task ended, and the chief said nothing to the owner after the last.
    let scene = board("cancelled");
    let found = end(&scene.look(ms(60, 0)), "it went quiet", two_hours()).unwrap();
    assert_eq!(
        found.said,
        "the project did not reach its end within the budget (it went quiet): 0 of 3 tasks open, no report"
    );
    // Nothing was put on the board.
    let scene = Scene::new(&[]);
    assert!(end(&scene.look(ms(60, 0)), "it went quiet", two_hours())
        .unwrap()
        .evidence
        .rows
        .contains(&"nothing was put on the board".to_owned()));
}

#[test]
fn every_task_ended_and_the_chiefs_report_after_the_last_holds_the_ninth() {
    let scene = board("failed");
    scene.message(
        301,
        (HUMAN, Some(CHIEF)),
        "note",
        None,
        "All three phases are done: 61 tests, 36 pages.",
        &t(46, 0),
    );
    assert!(end(&scene.look(ms(60, 0)), "it went quiet", two_hours()).is_none());
    // Its words in its terminal are a report too.
    let scene = board("accepted");
    let conversation = scene.conversation(CHIEF, "claude-code", "s-chief");
    scene.item(
        conversation,
        0,
        "assistant",
        "Done: all three phases.",
        true,
        &t(50, 0),
    );
    assert!(end(&scene.look(ms(60, 0)), "it went quiet", two_hours()).is_none());
}

#[test]
fn a_project_that_ends_after_its_budget_breaks_the_ninth_however_soon_after() {
    // A budget of 45 minutes: T-3 accepted at 10:45:05 and the report at 10:46, both
    // seen by the look at 10:46:10, which a disruption that blocked made late.
    let scene = Scene::new(&[("long-worker", "codex")]);
    scene.task(1, 3, "accepted", &t(30, 0));
    scene.task(3, 3, "accepted", &t(45, 5));
    scene.message(
        301,
        (HUMAN, Some(CHIEF)),
        "note",
        None,
        "All done.",
        &t(46, 0),
    );
    let deadline = ms(45, 0);
    let found = end(&scene.look(ms(46, 10)), "time ran out", deadline).unwrap();
    assert_eq!(
        found.said,
        "the project did not reach its end within the budget (time ran out): 1 of 2 tasks open, no report"
    );
    assert_eq!(
        found.evidence.rows[0],
        "T-3 accepted at 2026-10-15T10:45:05.000Z, after the deadline @long-worker: Task 3"
    );
    // Every task in time, and the report after the deadline: late too.
    let scene = Scene::new(&[("long-worker", "codex")]);
    scene.task(1, 3, "accepted", &t(30, 0));
    scene.message(
        301,
        (HUMAN, Some(CHIEF)),
        "note",
        None,
        "All done.",
        &t(46, 0),
    );
    let found = end(&scene.look(ms(46, 10)), "time ran out", deadline).unwrap();
    assert!(
        found.said.ends_with("0 of 1 tasks open, no report"),
        "{}",
        found.said
    );
    // Both before it: the end, though the run said its time ran out after.
    assert!(end(&scene.look(ms(50, 0)), "time ran out", ms(47, 0)).is_none());
}
