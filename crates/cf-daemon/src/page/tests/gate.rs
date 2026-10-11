//! The human's approve and decline on the page, and the setting that holds
//! what agents give one another: nothing is passed on in a project that is
//! closed; what is not at the gate can be neither passed on nor declined; what
//! can only be passed on says so when it is declined; a decision wakes the
//! dispatcher; and approval switched off leaves what waits where it is. Not
//! held to a Node recording. The real dispatcher stands behind the page.

use super::reading::home_with;
use super::*;

/// The page over a project whose human approves every hand-off, with a task
/// given to zeus whose brief waits at the gate.
struct Held {
    _home: tempfile::TempDir,
    _daemon: Bridge,
    app: Bridge,
    rig: Rig,
    project: i64,
    brief: i64,
}

fn held() -> Held {
    let (home, env) = home_with("[]");
    let rig = rig_over(env, None);
    let (daemon, app) = bridge_pair_over(&rig.spawn);
    register(&daemon, &rig.page, &rig.spawn);
    let context = &rig.kit;
    let project = context.with_staff(&["zeus"]).id;
    context.ledger.borrow_mut().set_gate(project, true).unwrap();
    let brief = context.give(project, "zeus", "Parser").message.unwrap().id;
    assert_eq!(context.message(brief).state, "gated");
    Held {
        _home: home,
        _daemon: daemon,
        app,
        rig,
        project,
        brief,
    }
}

#[tokio::test]
async fn nothing_is_passed_on_in_a_closed_project_until_it_is_resumed() {
    LocalSet::new()
        .run_until(async {
            let held = held();
            let context = &held.rig.kit;
            context.close_project(held.project).unwrap();
            let refused = ask(
                &held.app,
                "message.approve",
                json!({ "message": held.brief }),
            )
            .await;
            assert_eq!(
                refused,
                json!({ "ok": false, "error": "app is closed: resume it first" })
            );
            assert_eq!(context.message(held.brief).state, "gated");
            assert_eq!(
                held.rig.kicks.get(),
                0,
                "nothing succeeded, nothing is woken"
            );

            context.resume_project(held.project).unwrap();
            let passed = ask(
                &held.app,
                "message.approve",
                json!({ "message": held.brief }),
            )
            .await;
            assert_eq!(passed["ok"], true, "{passed}");
            assert_eq!(passed["message"]["state"], "queued");
            assert_eq!(held.rig.kicks.get(), 1, "the dispatcher is woken once");
        })
        .await;
}

#[tokio::test]
async fn a_message_that_is_not_at_the_gate_is_neither_passed_on_nor_declined() {
    LocalSet::new()
        .run_until(async {
            let held = held();
            let id = held.brief;
            let approve = |message: i64| json!({ "message": message });
            let passed = ask(&held.app, "message.approve", approve(id)).await;
            assert_eq!(passed["ok"], true, "{passed}");
            // Passed on once: it is the recipient's now.
            for operation in ["message.approve", "message.decline"] {
                assert_eq!(
                    ask(&held.app, operation, approve(id)).await,
                    json!({ "ok": false, "error": format!("message {id} is queued, not gated") }),
                    "{operation}"
                );
            }
            for operation in ["message.approve", "message.decline"] {
                assert_eq!(
                    ask(&held.app, operation, approve(9_999)).await,
                    json!({ "ok": false, "error": "no message 9999" }),
                    "{operation}"
                );
            }
            assert_eq!(held.rig.kicks.get(), 1, "only the first one succeeded");
        })
        .await;
}

#[tokio::test]
async fn a_question_a_note_and_a_result_can_only_be_passed_on_and_the_human_is_told_so() {
    LocalSet::new()
        .run_until(async {
            let held = held();
            let context = &held.rig.kit;
            // The brief is passed on and received: the task is at work.
            {
                let mut ledger = context.ledger.borrow_mut();
                ledger.approve_message(held.brief, "human").unwrap();
                ledger.begin_delivery(held.brief).unwrap();
                ledger
                    .confirm_delivery(held.brief, Some(&json!({ "item": "i-1" })))
                    .unwrap();
            }
            let question = context.ask(held.project, "zeus", "chief", 1, "Which format?");
            let note = context.note(held.project, "zeus", "chief", "Mind the tests");
            let result = context
                .ledger
                .borrow_mut()
                .record_result(held.project, 1, "Parser done")
                .unwrap()
                .message
                .unwrap();
            let held_for_the_human = [
                (question.id, "question"),
                (note.id, "note"),
                (result.id, "result"),
            ];
            for (id, kind) in held_for_the_human {
                assert_eq!(context.message(id).state, "gated", "{kind}");
                assert_eq!(
                    ask(&held.app, "message.decline", json!({ "message": id })).await,
                    json!({ "ok": false, "error": format!("a {kind} is passed on, not declined") }),
                    "{kind}"
                );
                assert_eq!(context.message(id).state, "gated", "{kind}");
            }
            assert_eq!(held.rig.kicks.get(), 0);
            // Each is the human's to pass on.
            for (id, kind) in held_for_the_human {
                let passed = ask(&held.app, "message.approve", json!({ "message": id })).await;
                assert_eq!(passed["message"]["state"], "queued", "{kind}: {passed}");
            }
        })
        .await;
}

#[tokio::test]
async fn a_declined_brief_is_cancelled_with_its_task_and_the_dispatcher_is_woken() {
    LocalSet::new()
        .run_until(async {
            let held = held();
            let declined = ask(
                &held.app,
                "message.decline",
                json!({ "message": held.brief }),
            )
            .await;
            assert_eq!(declined["ok"], true, "{declined}");
            assert_eq!(declined["message"]["state"], "cancelled");
            let context = &held.rig.kit;
            assert_eq!(context.task(held.project, 1).task.state, "cancelled");
            assert_eq!(held.rig.kicks.get(), 1);
        })
        .await;
}

#[tokio::test]
async fn approval_switched_off_on_the_page_wakes_the_dispatcher_and_leaves_what_waits_at_the_gate()
{
    LocalSet::new()
        .run_until(async {
            let held = held();
            let off = ask(
                &held.app,
                "project.gate",
                json!({ "project": held.project, "gate": false }),
            )
            .await;
            assert_eq!(off["ok"], true, "{off}");
            assert_eq!(off["project"]["gate"], false);
            assert_eq!(held.rig.kicks.get(), 1);
            let context = &held.rig.kit;
            assert_eq!(
                context.message(held.brief).state,
                "gated",
                "what was held is still the human's to decide"
            );
            let board = ask(&held.app, "board.get", json!({ "project": held.project })).await;
            assert_eq!(board["board"]["project"]["gate"], false);
            assert_eq!(board["board"]["gated"][0]["id"], held.brief);
        })
        .await;
}
