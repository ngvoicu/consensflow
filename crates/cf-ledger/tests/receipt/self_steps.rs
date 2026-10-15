//! What a chief's own later step (`cf task add --self --needs T-n`) is, as the
//! field report of 2026-10-15 (batch B, B4) found it: the evidence for a decision
//! that is the owner's (the brain's `issues/field-reports-2026-10-15.md`). The step
//! waits on the board for what it needs, then its brief is queued for the chief's
//! own window, and `cf task done` is refused until that brief has reached it; the
//! chief's result then goes to its requester, the chief itself, to accept, and
//! what needs the step waits for that acceptance.

use cf_ledger::NewTask;

use crate::fixture::{world, World};

/// The chief's own step, waiting for the tasks it `needs`.
fn own_step(w: &mut World, needs: &[u64], body: &str) -> i64 {
    w.ledger
        .create_task(
            w.project,
            &NewTask {
                from: "chief".into(),
                to: Some("chief".into()),
                needs: needs.to_vec(),
                body: body.into(),
                ..NewTask::default()
            },
        )
        .expect("the chief's own step")
        .task
        .number
}

#[test]
fn a_chiefs_own_step_is_refused_done_until_its_brief_reaches_the_chief_and_its_result_is_the_chiefs_to_accept(
) {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (parser, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    w.ledger
        .record_result(w.project, parser, "Parser done")
        .expect("its result");
    let own = own_step(
        &mut w,
        &[u64::try_from(parser).unwrap()],
        "Check the parser against the spec",
    );
    assert_eq!(w.state(own), "open", "it waits on the board for T-1");

    // The chief accepts T-1 in its turn, and goes on with its step in the same turn.
    w.ledger
        .accept_task(w.project, parser, "chief")
        .expect("accepted");
    assert_eq!(
        w.state(own),
        "queued",
        "its brief is queued for the chief's window"
    );
    let refused = w
        .ledger
        .record_result(w.project, own, "Checked: it holds")
        .unwrap_err();
    assert_eq!(refused.code(), Some("invalid-transition"));
    assert_eq!(
        refused.to_string(),
        format!("cannot record a result for T-{own}: it is queued"),
        "what B4's chief was told"
    );

    // Its brief reaches the chief once the chief's turn is over; then the result goes in.
    let step_brief = w.next("chief").expect("the step's brief");
    w.deliver(step_brief);
    assert_eq!(w.state(own), "working");
    let finished = w
        .ledger
        .record_result(w.project, own, "Checked: it holds")
        .expect("its result");
    assert_eq!(
        finished.task.state, "done",
        "not accepted: done, for its requester"
    );
    let result = finished.message.expect("the result message");
    assert_eq!(
        (
            result.kind.as_str(),
            result.sender.as_deref(),
            result.recipient.as_str(),
            result.state.as_str()
        ),
        ("result", Some("chief"), "chief", "queued"),
        "the chief's own result comes back to the chief, to be pasted and accepted"
    );

    // What needs the step waits for the chief to accept its own result.
    let after = own_step(
        &mut w,
        &[u64::try_from(own).unwrap()],
        "Write the release note",
    );
    assert_eq!(w.state(after), "open");
    w.ledger
        .accept_task(w.project, own, "chief")
        .expect("the chief accepts its own work");
    assert_eq!(w.state(after), "queued");
}
