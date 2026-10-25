//! Where the Resume's Enter meets main's field 3.0.6 (the merge of T-392): the
//! Enter pressed once more for a paste its window did not send goes only as the
//! ledger admits it now, and a paste whose key was withheld stays in the input box,
//! cleared before the next paste or taken for arrived once the human sends it. The
//! chief's own step finished while its brief sat in the chief's box is no brief to
//! send (B4); words told to a task's window wait for it while their paste waits in
//! its box, and its answer is the task's result once they are in, the human's Enter
//! on them included (B3).

use cf_engine::testing::Context;
use cf_harness::records::Role;
use cf_ledger::NewTask;
use serde_json::json;

use crate::deliveries::enters;
use crate::queued_tells::told_on_its_way;
use crate::stale_box::{human_sends, pasted};
use crate::withheld_enter::{clears, enter_again_with, times_handed};

const SEAM: &str = "adapter:claude-code";

/// The brief of the chief's own step (`cf task add --self`) sat unsent in the
/// chief's input box, and the chief finished the step while the Enter for it looked
/// at the screen. No key goes: the brief is withdrawn in the finish's words, as a
/// paste of it refused or tried again is, and never pasted again. The human may
/// send what the box holds: it arrived, and the step stays accepted; or the box is
/// emptied before the next paste.
#[test]
fn a_brief_of_the_chiefs_own_step_finished_while_its_enter_looked_is_withheld_and_never_sent_again()
{
    for sent_by_hand in [false, true] {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]).id;
        context.pass().unwrap();
        context.adapter.with("chief", |agent| {
            agent.settled = true;
            agent.arrive = false;
        });
        context.create_task(
            project,
            NewTask {
                from: "chief".to_owned(),
                to: Some("chief".to_owned()),
                body: "Check the parser".to_owned(),
                ..NewTask::default()
            },
        );
        context.pass().unwrap();
        let brief = context.task(project, 1).messages[0].id;
        assert_eq!(context.message(brief).state, "delivering");
        let chief = context.host.last("chief").expect("its window").pane.id;

        enter_again_with(&context, &chief, || {
            context
                .ledger
                .borrow_mut()
                .record_result(project, 1, "Checked: it holds")
                .unwrap();
        });
        assert_eq!(enters(&context), 0, "no key that sends");
        let withdrawn = context.message(brief);
        assert_eq!(
            (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
            (
                "cancelled",
                Some("T-1 was done before it came back to @chief")
            ),
            "withdrawn in the finish's words"
        );

        if sent_by_hand {
            human_sends(&context, "chief", &pasted(&context, &chief, brief));
        }
        // The next paste into the chief's window: what the box holds is looked at first.
        context.adapter.with("chief", |agent| agent.arrive = true);
        let note = context.note(project, "zeus", "chief", "Lexer ready");
        for _ in 0..3 {
            context.pass().unwrap();
        }
        assert_eq!(
            times_handed(&context, SEAM, brief),
            1,
            "never pasted again ({sent_by_hand})"
        );
        assert_eq!(times_handed(&context, SEAM, note.id), 1, "the note goes");
        assert_eq!(context.task(project, 1).task.state, "accepted");
        let human = context.id(project, "human");
        assert!(
            context.inbox(human).is_empty(),
            "nobody hears of a failure: {:?}",
            context.inbox(human)
        );
        if sent_by_hand {
            assert_eq!(context.message(brief).state, "delivered", "it arrived");
            assert_eq!(clears(&context), 0, "nothing pressed into an empty box");
        } else {
            assert_eq!(clears(&context), 1, "the box emptied before the note");
        }
    }
}

/// Words told to T-1's window while its brief was on its way were pasted next, and
/// sat unsent in the box while the human closed the project (the window outlived
/// the Close, and its Enter came after it): the key was withheld, and they wait for
/// the Resume. The window's
/// answer is no result until they are in (`cf task done` refused, 409): after the
/// Resume they are pasted once into an emptied box; or the human sent them from the
/// box while the project was closed, and they arrived. Either way the window's
/// answer after them is the task's result; and where the human sent them, its
/// `cf task done` is taken once they are in (Calliope's review of the merge, T-408).
#[test]
fn words_told_whose_enter_was_withheld_by_a_close_wait_and_the_answer_is_the_result_once_they_are_in(
) {
    for (sent_by_hand, done_by_hand) in [(false, false), (true, false), (true, true)] {
        let context = Context::new();
        let (tiers, session, told) = told_on_its_way(&context);
        let project = tiers.project.id;
        context.adapter.with("zeus", |agent| {
            agent.settled = true;
            agent.arrive = false;
        });
        context.pass().unwrap();
        assert_eq!(context.message(told).state, "delivering");
        let pane = context.host.last(&session).expect("its window").pane.id;

        context.host.refuse_kills.set(true);
        assert!(context.close_project(project).is_err(), "its window stays");
        context.host.refuse_kills.set(false);
        context.advance(11_000);
        context.host.set_snapshot(json!({ "outputQuietMs": 5_000 }));
        context.pass().unwrap();
        assert_eq!(enters(&context), 0, "no key that sends");
        assert_eq!(context.message(told).state, "queued", "given back");
        if done_by_hand {
            // The human sends them and the window has not answered yet: only its
            // `cf task done` gives the result.
            let given = context
                .adapter
                .item(Role::User, &pasted(&context, &pane, told));
            context
                .adapter
                .with("zeus", |agent| agent.items.push(given));
            context.pass().unwrap();
        } else if sent_by_hand {
            human_sends(&context, "zeus", &pasted(&context, &pane, told));
            context.pass().unwrap();
        }

        context.adapter.with("zeus", |agent| agent.arrive = true);
        context.resume_project(project).unwrap();
        if !sent_by_hand {
            let refused = context
                .ledger
                .borrow_mut()
                .record_result(project, 1, "Parser done")
                .expect_err("words on their way");
            assert!(
                format!("{refused:?}").contains("words-on-their-way"),
                "{refused:?}"
            );
        }
        if done_by_hand {
            // Refused while the words the human sent are not yet read as in.
            let refused = context
                .ledger
                .borrow_mut()
                .record_result(project, 1, "Parser done, on the v2 grammar")
                .expect_err("words on their way");
            assert!(
                format!("{refused:?}").contains("words-on-their-way"),
                "{refused:?}"
            );
        }
        for _ in 0..6 {
            context.pass().unwrap();
        }
        if done_by_hand {
            assert_eq!(context.message(told).state, "delivered");
            let taken = context
                .ledger
                .borrow_mut()
                .record_result(project, 1, "Parser done, on the v2 grammar")
                .expect("taken once the words the human sent are in");
            assert_eq!(taken.task.state, "done");
        }
        let pasted_words = context
            .recorder
            .calls(SEAM, &["deliver"])
            .iter()
            .filter(|(_, args)| {
                args[0]["text"]
                    .as_str()
                    .is_some_and(|text| text.contains("Use the v2 grammar"))
            })
            .count();
        assert_eq!(
            pasted_words,
            if sent_by_hand { 1 } else { 2 },
            "pasted once more only where nobody sent them ({sent_by_hand})"
        );
        assert_eq!(context.message(told).state, "delivered");
        assert_eq!(
            clears(&context),
            usize::from(!sent_by_hand),
            "the box emptied before the paste, and only then"
        );
        // The window's answer after them is the task's result: the human's Enter
        // was answered already, and the window answers the words pasted now.
        if !sent_by_hand {
            context
                .adapter
                .answer("zeus", "Parser done, on the v2 grammar");
            for _ in 0..3 {
                context.pass().unwrap();
            }
        }
        let thread = context.task(project, 1);
        assert_eq!(thread.task.state, "done", "{sent_by_hand} {done_by_hand}");
        let result = thread
            .messages
            .iter()
            .find(|message| message.kind == "result")
            .expect("its result");
        assert!(result.id > told, "after the words");
    }
}
