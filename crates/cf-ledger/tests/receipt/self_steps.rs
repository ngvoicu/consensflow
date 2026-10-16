//! The chief's own later step (`cf task add --self --needs T-n`), finished by its
//! word (the field report of 2026-10-15, B4, and the owner's decision of
//! 2026-10-16: `cf task done` was refused while the step's brief was on its way
//! back to the chief, and the chief's own result came back to it to accept). The
//! step is done whenever the chief is at it, queued or working; a brief not yet
//! given is withdrawn, and one on its way goes no further once its paste is
//! refused or given back (the second review, 2026-10-16); its words are its
//! result, received as written, and it is accepted at once, so what needs it
//! goes on. A task the human gives the chief is no such step: its result is the
//! human's to decide.

use cf_ledger::NewTask;

use crate::fixture::{world, World};

/// A task the chief is given by `from`, waiting for the tasks it `needs`.
fn for_the_chief(w: &mut World, from: &str, needs: &[i64], body: &str) -> i64 {
    w.ledger
        .create_task(
            w.project,
            &NewTask {
                from: from.into(),
                to: Some("chief".into()),
                needs: needs.iter().map(|n| u64::try_from(*n).unwrap()).collect(),
                body: body.into(),
                ..NewTask::default()
            },
        )
        .expect("a task for the chief")
        .task
        .number
}

/// T-1, Parser, done by zeus and waiting for the chief's decision.
fn parser_done(w: &mut World) -> i64 {
    let created = w.give("zeus", "Parser");
    let (parser, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    w.ledger
        .record_result(w.project, parser, "Parser done")
        .expect("its result");
    parser
}

/// What waits for the chief: the tasks of the messages on their way to its window.
fn waiting_for_the_chief(w: &World) -> Vec<Option<i64>> {
    let chief = w.id("chief");
    w.ledger
        .pending(chief)
        .expect("a read")
        .into_iter()
        .map(|message| message.task_number)
        .collect()
}

#[test]
fn a_chiefs_own_step_is_done_by_its_word_while_its_brief_is_still_on_its_way_and_is_accepted_at_once(
) {
    let mut w = world();
    let parser = parser_done(&mut w);
    let own = for_the_chief(&mut w, "chief", &[parser], "Check the parser");
    let after = for_the_chief(&mut w, "chief", &[own], "Write the release note");
    assert_eq!(w.state(own), "open", "it waits on the board for T-1");

    // The chief accepts T-1 in its turn, and does its step in the same turn.
    w.ledger
        .accept_task(w.project, parser, "chief")
        .expect("accepted");
    assert_eq!(w.state(own), "queued", "its brief is on its way back");
    let brief = w.next("chief").expect("the step's brief");
    let finished = w
        .ledger
        .record_result(w.project, own, "Checked: it holds")
        .expect("done by its word");
    assert_eq!(finished.task.state, "accepted", "nobody else decides it");
    let result = finished.message.expect("its result");
    assert_eq!(
        (
            result.kind.as_str(),
            result.sender.as_deref(),
            result.recipient.as_str(),
            result.state.as_str(),
            result.body.as_str()
        ),
        (
            "result",
            Some("chief"),
            "chief",
            "read",
            "Checked: it holds"
        ),
        "the words are its result, received as written: nothing to accept"
    );
    let withdrawn = w.message(brief);
    assert_eq!(withdrawn.state, "cancelled");
    assert_eq!(
        withdrawn.reason.as_deref(),
        Some(format!("T-{own} was done before it came back to @chief").as_str())
    );
    assert_eq!(w.state(after), "queued", "what needs it goes on");
    assert_eq!(
        waiting_for_the_chief(&w),
        vec![Some(after)],
        "nothing of the step comes back to it: only the next step's brief"
    );
}

#[test]
fn a_chiefs_own_step_at_work_is_done_and_accepted_and_one_still_waiting_on_the_board_is_not() {
    let mut w = world();
    let parser = parser_done(&mut w);
    let own = for_the_chief(&mut w, "chief", &[parser], "Check the parser");
    let refused = w
        .ledger
        .record_result(w.project, own, "Checked")
        .unwrap_err();
    assert_eq!(refused.code(), Some("invalid-transition"));
    assert_eq!(
        refused.to_string(),
        format!("cannot finish T-{own}: it is open"),
        "it waits for what it needs"
    );

    w.ledger
        .accept_task(w.project, parser, "chief")
        .expect("accepted");
    let brief = w.next("chief").expect("the step's brief");
    w.deliver(brief);
    assert_eq!(w.state(own), "working");
    let finished = w
        .ledger
        .record_result(w.project, own, "Checked: it holds")
        .expect("done by its word");
    assert_eq!(finished.task.state, "accepted");
    assert_eq!(
        w.message(brief).state,
        "delivered",
        "the brief it had stays"
    );
    assert_eq!(waiting_for_the_chief(&w), Vec::<Option<i64>>::new());
}

#[test]
fn a_brief_on_its_way_when_the_step_was_done_goes_no_further_whether_its_paste_is_refused_or_given_back(
) {
    for settled in ["tried again", "given up", "given back"] {
        let mut w = world();
        let parser = parser_done(&mut w);
        let own = for_the_chief(&mut w, "chief", &[parser], "Check the parser");
        w.ledger
            .accept_task(w.project, parser, "chief")
            .expect("accepted");
        let brief = w.next("chief").expect("the step's brief");
        w.begin(brief);
        w.ledger
            .record_result(w.project, own, "Checked: it holds")
            .expect("done by its word");
        assert_eq!(
            w.message(brief).state,
            "delivering",
            "{settled}: its paste is under way, which the finish leaves to it"
        );
        let refused = "refused by the window";
        let answer = match settled {
            "tried again" => w.ledger.retry_delivery(brief, refused, false),
            "given up" => w.ledger.fail_delivery(brief, refused),
            _ => w
                .ledger
                .retry_delivery(brief, "the window went before it landed", true),
        }
        .expect("settled");
        assert_eq!(
            (answer.state.as_str(), answer.reason.as_deref()),
            (
                "cancelled",
                Some(format!("T-{own} was done before it came back to @chief").as_str())
            ),
            "{settled}: withdrawn, in the finish's words"
        );
        assert_eq!(w.next("chief"), None, "{settled}: never given it again");
        assert_eq!(w.state(own), "accepted");
    }
}

#[test]
fn a_task_the_human_gives_the_chief_puts_its_result_to_the_human_as_it_always_did() {
    let mut w = world();
    let given = for_the_chief(&mut w, "human", &[], "Plan the week");
    assert_eq!(w.state(given), "queued");
    let refused = w
        .ledger
        .record_result(w.project, given, "Planned")
        .unwrap_err();
    assert_eq!(
        refused.to_string(),
        format!("cannot record a result for T-{given}: it is queued"),
        "the chief does it once it has it"
    );
    let brief = w.next("chief").expect("its brief");
    w.deliver(brief);
    let finished = w
        .ledger
        .record_result(w.project, given, "Planned")
        .expect("its result");
    assert_eq!(finished.task.state, "done", "the human decides it");
    let result = finished.message.expect("its result");
    assert_eq!(
        (result.recipient.as_str(), result.state.as_str()),
        ("human", "queued")
    );
}
