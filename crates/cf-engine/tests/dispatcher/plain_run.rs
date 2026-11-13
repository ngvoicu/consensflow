//! A project with no backup and nobody on Vacation writes the rows it wrote before backups
//! and Vacation (BKV-03): one run that takes members out for a quota, an account and a busy
//! model, makes tasks wait and goes on, held to the events and the messages every participant
//! was written. The golden `goldens/plain-run.json` was recorded on the tree before the
//! scheduler knew of either; a change that moves it is made in the file by hand, with its
//! reason in the commit.

use std::path::PathBuf;

use cf_engine::testing::Context;
use cf_harness::records::Quota;
use serde_json::{json, Value};

use crate::fixtures::{after, exhausted, now, soon, Tiers};

/// The path of the recorded run.
fn golden() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/goldens/plain-run.json")
}

/// Everything the run wrote: the project's events, and each participant's messages.
fn rows(context: &Context, project: i64) -> Value {
    let events: Vec<Value> = context
        .ledger
        .borrow()
        .events(project, 0, 5_000)
        .unwrap()
        .into_iter()
        .map(|event| json!({ "kind": event.kind, "data": event.data }))
        .collect();
    let mut messages = serde_json::Map::new();
    for participant in context.project(project).participants {
        let mut inbox = context.inbox(participant.id);
        inbox.reverse();
        let inbox: Vec<Value> = inbox
            .iter()
            .map(|message| serde_json::to_value(message).unwrap())
            .collect();
        messages.insert(participant.handle, Value::Array(inbox));
    }
    json!({ "events": events, "messages": messages })
}

#[test]
fn a_project_with_no_backup_and_nobody_on_vacation_writes_the_rows_it_wrote_before() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    // zeus runs out for twenty minutes, its task held with its window.
    let resets_at = after(&context, 20 * 60_000);
    context
        .adapter
        .refuse("zeus", exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    // The next task goes to diana, whose account is then refused for good.
    tiers.open_body("Write the lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.refuse(
        "diana",
        Quota::Unavailable {
            at: Some(now(&context)),
            because: "the login was refused".to_owned(),
        },
    );
    context.pass().unwrap();
    // A third task finds nobody free: its giver is told once, why.
    tiers.open_body("Write the printer");
    context.pass().unwrap();
    context.pass().unwrap();
    // zeus's reset passes: its held task goes on, and the third task is given out.
    context.advance(21 * 60_000);
    context.adapter.quota("zeus", None);
    context.pass().unwrap();
    context.pass().unwrap();
    // diana is back by the human's word; zeus's model is busy, and then diana's.
    context.back_from_quota(tiers.project.id, "diana").unwrap();
    context.pass().unwrap();
    context.mark_out_busy(tiers.id("zeus"), &soon(&context, 1));
    tiers.open_body("Write the checker");
    context.pass().unwrap();
    context.mark_out_busy(tiers.id("diana"), &soon(&context, 1));
    tiers.open_body("Write the docs");
    context.pass().unwrap();
    context.pass().unwrap();

    let found = rows(&context, tiers.project.id);
    let recorded: Value =
        serde_json::from_str(&std::fs::read_to_string(golden()).expect("the recorded run"))
            .unwrap();
    assert_eq!(found, recorded);
}
